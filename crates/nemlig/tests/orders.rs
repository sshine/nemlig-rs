//! Past orders: the history, one order's lines, and the scan that joins them.

#![allow(clippy::expect_used)]

mod common;

use serde_json::{Value, json};
use wiremock::matchers::query_param;

use common::{hits, mock, ok, site};

fn history_entry(id: i64, total: f64, sub_total: f64) -> Value {
    json!({
        "Status": 4,
        "OrderNumber": format!("2025-{id}"),
        "Total": total,
        "SubTotal": sub_total,
        "OrderDate": "2025-11-25T06:07:18Z",
        "Id": id,
        "DeliveryTime": {"Start": "2025-11-27T17:00:00Z", "End": "2025-11-27T20:00:00Z"},
        "HasInvoice": true,
        "IsEditable": false,
        "CanRepeat": true,
        "IsMealBox": false,
    })
}

fn lines() -> Value {
    json!({"Lines": [{
        "ProductNumber": "5064654",
        "ProductName": "Black Coffee Blend Supreme",
        "Quantity": 2.0,
        "Unit": "Stk.",
        "AverageItemPrice": 90.0,
        "Amount": 180.0,
        "OriginalAmount": 199.9,
        "GroupName": "Kaffe, hele bønner",
        "CampaignName": "Fast mixtilbud",
        "HasCampaign": true,
    }]})
}

#[tokio::test]
async fn the_history_is_paged_with_skip_and_take() {
    let (client, server) = mock().await;
    site("GET", "/webapi/order/GetBasicOrderHistory")
        .and(query_param("skip", "20"))
        .and(query_param("take", "10"))
        .respond_with(ok(json!({
            "Orders": [history_entry(12345678, 604.48, 574.48)],
            "NumberOfPages": 20,
        })))
        .mount(&server)
        .await;

    let history = client
        .orders()
        .history(20, 10)
        .await
        .expect("reads the page");
    assert_eq!(history.number_of_pages, 20);
    let order = history.orders.first().expect("one order");
    assert_eq!(order.id, Some(12345678));
    assert!((order.delivery_fee() - 30.0).abs() < 0.001);
    assert_eq!(order.status.to_string(), "delivered");
}

#[tokio::test]
async fn an_orders_lines_use_the_other_endpoints_field_names() {
    let (client, server) = mock().await;
    site("GET", "/webapi/v2/order/GetOrderHistory/12345678")
        .respond_with(ok(lines()))
        .mount(&server)
        .await;

    let lines = client
        .orders()
        .lines(12345678)
        .await
        .expect("reads the lines");
    let line = lines.first().expect("one line");
    assert_eq!(line.product_number, "5064654");
    assert_eq!(line.unit.as_deref(), Some("Stk."));
    assert!((line.quantity - 2.0).abs() < f64::EPSILON);
    assert!((line.savings() - 19.9).abs() < 0.001);
}

#[tokio::test]
async fn one_order_is_found_by_walking_the_history() {
    let (client, server) = mock().await;
    site("GET", "/webapi/order/GetBasicOrderHistory")
        .respond_with(ok(json!({
            "Orders": [
                history_entry(99999999, 100.0, 90.0),
                history_entry(12345678, 604.48, 574.48),
            ],
            "NumberOfPages": 1,
        })))
        .mount(&server)
        .await;
    site("GET", "/webapi/v2/order/GetOrderHistory/12345678")
        .respond_with(ok(lines()))
        .mount(&server)
        .await;

    let order = client
        .orders()
        .get(12345678)
        .await
        .expect("finds the order");
    assert_eq!(order.summary.id, Some(12345678));
    assert_eq!(order.lines.len(), 1);
}

#[tokio::test]
async fn the_walk_stops_at_the_last_page_rather_than_asking_forever() {
    let (client, server) = mock().await;
    // A short page is the last page, so the scan must not ask for another.
    site("GET", "/webapi/order/GetBasicOrderHistory")
        .respond_with(ok(json!({
            "Orders": [history_entry(99999999, 100.0, 90.0)],
            "NumberOfPages": 1,
        })))
        .mount(&server)
        .await;

    let err = client
        .orders()
        .get(12345678)
        .await
        .expect_err("finds nothing");
    assert!(err.is_not_found());
    assert_eq!(hits(&server, "/webapi/order/GetBasicOrderHistory").await, 1);
}

#[tokio::test]
async fn an_order_with_no_lines_is_an_order_with_no_lines() {
    let (client, server) = mock().await;
    site("GET", "/webapi/v2/order/GetOrderHistory/1")
        .respond_with(ok(json!({})))
        .mount(&server)
        .await;

    assert!(client.orders().lines(1).await.expect("reads").is_empty());
}
