//! The basket tools, which change the account's real basket.

#![allow(clippy::expect_used)]

mod common;

use nemlig_mcp::server::Mode;
use serde_json::{Value, json};

use common::{basket, basket_line, ok, serve, site};

fn writing() -> Mode {
    Mode { writes: true }
}

/// The body of every AddToBasket the mocked site saw.
async fn writes(http: &wiremock::MockServer) -> Vec<Value> {
    http.received_requests()
        .await
        .expect("requests were recorded")
        .into_iter()
        .filter(|r| r.url.path() == "/webapi/basket/AddToBasket")
        .map(|r| serde_json::from_slice(&r.body).expect("the body is JSON"))
        .collect()
}

#[tokio::test]
async fn setting_a_quantity_sends_the_absolute_number_and_reports_what_is_there() {
    let h = serve(writing()).await;
    site("POST", "/webapi/basket/AddToBasket")
        .respond_with(ok(basket(vec![basket_line("5070417", "Cocio", 3, 59.5)])))
        .mount(&h.http)
        .await;

    let body = h
        .read(
            "nemlig_set_basket_quantity",
            json!({"product_id": "5070417", "quantity": 3}),
        )
        .await;

    assert_eq!(body["quantity"], json!(3));
    assert_eq!(body["line_count"], json!(1));

    let sent = writes(&h.http).await;
    assert_eq!(sent[0]["ProductId"], "5070417");
    assert_eq!(sent[0]["quantity"], json!(3));
}

#[tokio::test]
async fn setting_a_quantity_to_zero_is_how_a_line_is_removed() {
    let h = serve(writing()).await;
    site("POST", "/webapi/basket/AddToBasket")
        .respond_with(ok(basket(vec![])))
        .mount(&h.http)
        .await;

    let body = h
        .read(
            "nemlig_set_basket_quantity",
            json!({"product_id": "5070417", "quantity": 0}),
        )
        .await;

    assert_eq!(body["quantity"], json!(0));
    assert_eq!(body["line_count"], json!(0));
    assert_eq!(writes(&h.http).await[0]["quantity"], json!(0));
}

#[tokio::test]
async fn a_product_number_that_is_not_a_number_is_refused_before_a_request() {
    let h = serve(writing()).await;
    let message = h
        .error(
            "nemlig_set_basket_quantity",
            json!({"product_id": "5070417x", "quantity": 1}),
        )
        .await;

    assert!(message.contains("nemlig_search_products"), "{message}");
    assert!(
        writes(&h.http).await.is_empty(),
        "nothing should have been sent"
    );
}

#[tokio::test]
async fn emptying_the_basket_zeroes_every_line_and_says_how_many() {
    let h = serve(writing()).await;
    site("GET", "/webapi/basket/GetBasket")
        .respond_with(ok(basket(vec![
            basket_line("1", "Et", 1, 10.0),
            basket_line("2", "To", 2, 20.0),
        ])))
        .mount(&h.http)
        .await;
    site("POST", "/webapi/basket/AddToBasket")
        .respond_with(ok(basket(vec![])))
        .mount(&h.http)
        .await;

    let result = h
        .call("nemlig_empty_basket", json!({"confirm_line_count": 2}))
        .await;
    assert_ne!(result.is_error, Some(true), "{result:?}");
    assert!(common::text(&result).contains("Removed 2 line(s)"));

    let sent = writes(&h.http).await;
    assert_eq!(sent.len(), 2, "one write per line; there is no bulk remove");
    assert!(sent.iter().all(|body| body["quantity"] == json!(0)));
}

#[tokio::test]
async fn emptying_a_basket_that_changed_since_it_was_read_is_refused() {
    // The account's basket is shared with the website and the app, so it can change between
    // a read and the call that acts on it. The line count is what notices.
    let h = serve(writing()).await;
    site("GET", "/webapi/basket/GetBasket")
        .respond_with(ok(basket(vec![basket_line("1", "Et", 1, 10.0)])))
        .mount(&h.http)
        .await;

    let message = h
        .error("nemlig_empty_basket", json!({"confirm_line_count": 5}))
        .await;

    assert!(message.contains("holds 1 line(s), not 5"), "{message}");
    assert!(
        writes(&h.http).await.is_empty(),
        "nothing should have been removed"
    );
}

#[tokio::test]
async fn emptying_an_already_empty_basket_is_a_confirmation_rather_than_a_failure() {
    let h = serve(writing()).await;
    site("GET", "/webapi/basket/GetBasket")
        .respond_with(ok(basket(vec![])))
        .mount(&h.http)
        .await;

    let result = h
        .call("nemlig_empty_basket", json!({"confirm_line_count": 0}))
        .await;
    assert_ne!(result.is_error, Some(true), "{result:?}");
    assert!(writes(&h.http).await.is_empty());
}

#[tokio::test]
async fn a_write_that_the_site_refuses_names_the_product_and_the_search_tool() {
    let h = serve(writing()).await;
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/webapi/basket/AddToBasket"))
        .respond_with(wiremock::ResponseTemplate::new(404))
        .mount(&h.http)
        .await;

    let message = h
        .error(
            "nemlig_set_basket_quantity",
            json!({"product_id": "9999999", "quantity": 1}),
        )
        .await;
    assert!(message.contains("9999999"), "{message}");
    assert!(message.contains("nemlig_search_products"), "{message}");
}
