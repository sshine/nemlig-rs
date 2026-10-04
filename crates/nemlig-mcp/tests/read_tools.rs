//! The read tools, driven through a real MCP client against a mocked nemlig.com.

#![allow(clippy::expect_used)]

mod common;

use nemlig_mcp::render::OutputLimits;
use nemlig_mcp::server::Mode;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

use common::{basket, basket_line, gateway, ok, product, search_results, serve, serve_with, site};

#[tokio::test]
async fn searching_returns_the_products_and_how_many_matched() {
    let h = serve(Mode::default()).await;
    gateway("search")
        .respond_with(ok(search_results(
            vec![
                product("5070417", "Cocio kakaomælk", 119.0),
                product("701025", "Cocio kakaomælk lille", 23.75),
            ],
            412,
        )))
        .mount(&h.http)
        .await;

    let body = h
        .read("nemlig_search_products", json!({"query": "cocio"}))
        .await;

    // The total is what the gateway found, not what it sent, so a caller can tell "there
    // are two of these" from "there are four hundred, here are two".
    assert_eq!(body["total"], json!(412));
    assert_eq!(body["returned"], json!(2));
    assert_eq!(body["truncated"], json!(true));
    assert_eq!(body["items"][0]["Id"], "5070417");
    assert_eq!(body["items"][0]["Labels"][0], "Produceret i Danmark");
}

#[tokio::test]
async fn a_listing_is_shortened_to_the_output_budget() {
    let limits = OutputLimits {
        max_items: 3,
        ..OutputLimits::default()
    };
    let h = serve_with(Mode::default(), limits).await;
    let products: Vec<_> = (0..20)
        .map(|i| product(&format!("100000{i}"), "Mælk", 10.0))
        .collect();
    gateway("search")
        .respond_with(ok(search_results(products, 20)))
        .mount(&h.http)
        .await;

    let body = h
        .read("nemlig_search_products", json!({"query": "mælk"}))
        .await;
    assert_eq!(body["returned"], json!(3));
    assert_eq!(body["truncated"], json!(true));
}

#[tokio::test]
async fn suggestions_come_back_as_words() {
    let h = serve(Mode::default()).await;
    gateway("quick")
        .respond_with(ok(json!({
            "SearchQuery": "mæl",
            "Categories": [{"Name": "Mælk", "Url": "/dagligvarer/mejeri"}],
            "Suggestions": ["mælk", "mælkesnitte"],
        })))
        .mount(&h.http)
        .await;

    let body = h
        .read("nemlig_suggest_queries", json!({"query": "mæl"}))
        .await;
    assert_eq!(body["Suggestions"][1], "mælkesnitte");
}

#[tokio::test]
async fn a_product_slug_is_read_without_a_search() {
    let h = serve(Mode::default()).await;
    site("GET", "/produkt-701025")
        .respond_with(ok(json!({
            "content": [{
                "TemplateName": "productdetailspot",
                "Id": "701025",
                "Name": "Cocio kakaomælk",
                "Price": 23.75,
                "Attributes": [{"Name": "Allergener", "Value": "Mælk"}],
            }],
        })))
        .mount(&h.http)
        .await;

    let body = h
        .read("nemlig_get_product", json!({"product": "produkt-701025"}))
        .await;
    assert_eq!(body["Id"], "701025");
    assert_eq!(body["Attributes"][0]["Name"], "Allergener");
}

#[tokio::test]
async fn a_product_number_the_catalogue_does_not_know_names_the_tool_that_would_find_it() {
    let h = serve(Mode::default()).await;
    gateway("search")
        .respond_with(ok(search_results(vec![], 0)))
        .mount(&h.http)
        .await;

    let message = h
        .error("nemlig_get_product", json!({"product": "9999999"}))
        .await;
    assert!(message.contains("nemlig_search_products"), "{message}");
}

#[tokio::test]
async fn the_basket_comes_back_with_its_total() {
    let h = serve(Mode::default()).await;
    site("GET", "/webapi/basket/GetBasket")
        .respond_with(ok(basket(vec![
            basket_line("5064654", "Black Coffee", 1, 99.95),
            basket_line("5070417", "Cocio", 2, 59.5),
        ])))
        .mount(&h.http)
        .await;

    let body = h.read("nemlig_get_basket", json!({})).await;
    assert_eq!(body["line_count"], json!(2));
    assert_eq!(body["total_products_price"], json!(218.95));
}

#[tokio::test]
async fn orders_come_back_newest_first_with_both_identifiers() {
    let h = serve(Mode::default()).await;
    site("GET", "/webapi/order/GetBasicOrderHistory")
        .respond_with(ok(json!({
            "Orders": [{
                "Status": 4,
                "Id": 12345678,
                "OrderNumber": "2025-12345678",
                "Total": 604.48,
                "SubTotal": 574.48,
                "OrderDate": "2025-11-25T06:07:18Z",
            }],
            "NumberOfPages": 3,
        })))
        .mount(&h.http)
        .await;

    let body = h.read("nemlig_list_orders", json!({})).await;
    assert_eq!(body["items"][0]["Id"], json!(12345678));
    assert_eq!(body["items"][0]["OrderNumber"], "2025-12345678");
    assert_eq!(body["items"][0]["Status"], json!(4));
}

#[tokio::test]
async fn one_order_comes_back_with_its_lines_and_the_derived_delivery_fee() {
    let h = serve(Mode::default()).await;
    site("GET", "/webapi/order/GetBasicOrderHistory")
        .respond_with(ok(json!({
            "Orders": [{"Id": 12345678, "Total": 604.48, "SubTotal": 574.48, "Status": 4}],
            "NumberOfPages": 1,
        })))
        .mount(&h.http)
        .await;
    site("GET", "/webapi/v2/order/GetOrderHistory/12345678")
        .respond_with(ok(json!({"Lines": [{
            "ProductNumber": "5064654",
            "ProductName": "Black Coffee Blend Supreme",
            "Quantity": 2.0,
            "Unit": "Stk.",
            "Amount": 180.0,
            "OriginalAmount": 199.9,
        }]})))
        .mount(&h.http)
        .await;

    let body = h
        .read("nemlig_get_order", json!({"order_id": 12345678}))
        .await;
    assert_eq!(body["line_count"], json!(1));
    assert_eq!(body["lines"][0]["Unit"], "Stk.");
    assert!((body["delivery_fee"].as_f64().unwrap_or_default() - 30.0).abs() < 0.001);
}

#[tokio::test]
async fn an_order_that_is_not_in_the_history_names_the_lister() {
    let h = serve(Mode::default()).await;
    site("GET", "/webapi/order/GetBasicOrderHistory")
        .respond_with(ok(json!({"Orders": [], "NumberOfPages": 0})))
        .mount(&h.http)
        .await;

    let message = h.error("nemlig_get_order", json!({"order_id": 1})).await;
    assert!(message.contains("nemlig_list_orders"), "{message}");
}

#[tokio::test]
async fn the_account_reports_where_and_when_it_shops() {
    let h = serve(Mode::default()).await;
    let body = h.read("nemlig_get_account", json!({})).await;
    assert_eq!(body["zip_code"], "1620");
    assert_eq!(body["delivery_zone_id"], json!(1));
    assert_eq!(body["timeslot_utc"], common::TIMESLOT);
    assert_eq!(body["username"], "someone@example.com");
}

#[tokio::test]
async fn no_booked_delivery_is_said_in_words_rather_than_as_an_error() {
    let h = serve(Mode::default()).await;
    Mock::given(method("GET"))
        .and(path("/webapi/Order/DeliverySpot"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&h.http)
        .await;

    let result = h.call("nemlig_get_delivery_slot", json!({})).await;
    assert_ne!(result.is_error, Some(true));
    assert!(common::text(&result).contains("no delivery booked"));
}

#[tokio::test]
async fn placements_keep_their_danish_wording() {
    let h = serve(Mode::default()).await;
    site("GET", "/webapi/Checkout/GetDeliveryPlacements")
        .respond_with(ok(json!([
            {"Key": "Frontdoor", "Value": "Foran min hoveddør"},
        ])))
        .mount(&h.http)
        .await;

    let body = h.read("nemlig_list_delivery_placements", json!({})).await;
    assert_eq!(body["items"][0]["Value"], "Foran min hoveddør");
}

#[tokio::test]
async fn a_rejected_sign_in_says_which_variables_to_check() {
    let h = serve(Mode::default()).await;
    Mock::given(method("GET"))
        .and(path("/webapi/basket/GetBasket"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&h.http)
        .await;

    let message = h.error("nemlig_get_basket", json!({})).await;
    assert!(message.contains("NEMLIG_USERNAME"), "{message}");
    assert!(message.contains("NEMLIG_PASSWORD"), "{message}");
}

#[tokio::test]
async fn a_throttled_call_says_the_wait_has_already_been_taken() {
    let h = serve(Mode::default()).await;
    Mock::given(method("GET"))
        .and(path("/webapi/basket/GetBasket"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "1"))
        .mount(&h.http)
        .await;

    let message = h.error("nemlig_get_basket", json!({})).await;
    assert!(message.contains("throttling"), "{message}");
}
