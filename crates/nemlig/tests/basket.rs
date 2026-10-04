//! Reading and changing the basket, where quantities are absolute.

#![allow(clippy::expect_used)]

mod common;

use serde_json::Value;
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

use common::{basket, basket_line, hits, mock, ok, requests, site};

/// The body of every AddToBasket the server saw.
async fn writes(server: &wiremock::MockServer) -> Vec<Value> {
    requests(server)
        .await
        .into_iter()
        .filter(|r| r.url.path() == "/webapi/basket/AddToBasket")
        .map(|r| serde_json::from_slice(&r.body).expect("the body is JSON"))
        .collect()
}

#[tokio::test]
async fn a_basket_is_read_with_its_lines_and_its_total() {
    let (client, server) = mock().await;
    site("GET", "/webapi/basket/GetBasket")
        .respond_with(ok(basket(vec![
            basket_line("5064654", "Black Coffee", 1, 99.95),
            basket_line("5070417", "Cocio", 2, 59.5),
        ])))
        .mount(&server)
        .await;

    let basket = client.basket().get().await.expect("reads the basket");
    assert_eq!(basket.lines.len(), 2);
    assert!(!basket.is_empty());
    assert!((basket.total() - 218.95).abs() < 0.001);

    let id = "5064654".parse().expect("valid");
    assert_eq!(basket.line(&id).map(|l| l.quantity), Some(1));
}

#[tokio::test]
async fn setting_a_quantity_sends_the_absolute_number() {
    let (client, server) = mock().await;
    site("POST", "/webapi/basket/AddToBasket")
        .respond_with(ok(basket(vec![basket_line("5070417", "Cocio", 3, 59.5)])))
        .mount(&server)
        .await;

    let id = "5070417".parse().expect("valid");
    let after = client.basket().set_quantity(&id, 3).await.expect("sets it");

    assert_eq!(after.line(&id).map(|l| l.quantity), Some(3));
    let body = writes(&server).await.remove(0);
    assert_eq!(body["ProductId"], "5070417");
    assert_eq!(body["quantity"], 3);
    assert_eq!(body["AffectPartialQuantity"], false);
    assert_eq!(body["disableQuantityValidation"], false);
}

#[tokio::test]
async fn a_quantity_of_zero_is_how_a_line_is_removed() {
    let (client, server) = mock().await;
    site("POST", "/webapi/basket/AddToBasket")
        .respond_with(ok(basket(vec![])))
        .mount(&server)
        .await;

    let id = "5070417".parse().expect("valid");
    let after = client
        .basket()
        .set_quantity(&id, 0)
        .await
        .expect("removes it");

    assert!(after.is_empty());
    assert_eq!(writes(&server).await.remove(0)["quantity"], 0);
}

#[tokio::test]
async fn emptying_the_basket_zeroes_every_line() {
    let (client, server) = mock().await;
    site("GET", "/webapi/basket/GetBasket")
        .respond_with(ok(basket(vec![
            basket_line("1", "Et", 1, 10.0),
            basket_line("2", "To", 2, 20.0),
            basket_line("3", "Tre", 3, 30.0),
        ])))
        .mount(&server)
        .await;
    site("POST", "/webapi/basket/AddToBasket")
        .respond_with(ok(basket(vec![])))
        .mount(&server)
        .await;

    let after = client.basket().empty().await.expect("empties the basket");
    assert!(after.is_empty());

    let bodies = writes(&server).await;
    assert_eq!(
        bodies.len(),
        3,
        "one write per line, since there is no remove verb"
    );
    let ids: Vec<&str> = bodies
        .iter()
        .map(|b| b["ProductId"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(ids, ["1", "2", "3"]);
    assert!(bodies.iter().all(|b| b["quantity"] == 0));
}

#[tokio::test]
async fn emptying_a_basket_that_is_already_empty_costs_one_read() {
    let (client, server) = mock().await;
    site("GET", "/webapi/basket/GetBasket")
        .respond_with(ok(basket(vec![])))
        .mount(&server)
        .await;

    client.basket().empty().await.expect("nothing to do");
    assert_eq!(hits(&server, "/webapi/basket/AddToBasket").await, 0);
}

#[tokio::test]
async fn a_write_is_not_retried_after_a_server_error() {
    let (client, server) = mock().await;
    Mock::given(method("POST"))
        .and(path("/webapi/basket/AddToBasket"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let id = "5070417".parse().expect("valid");
    let err = client
        .basket()
        .set_quantity(&id, 1)
        .await
        .expect_err("gives up");
    assert!(err.is_busy());
    assert_eq!(
        hits(&server, "/webapi/basket/AddToBasket").await,
        1,
        "the client cannot tell whether the first one went through"
    );
}

#[tokio::test]
async fn the_write_carries_the_referer_the_site_looks_for() {
    let (client, server) = mock().await;
    site("POST", "/webapi/basket/AddToBasket")
        .respond_with(ok(basket(vec![])))
        .mount(&server)
        .await;

    let id = "5070417".parse().expect("valid");
    client.basket().set_quantity(&id, 1).await.expect("writes");

    let write = requests(&server)
        .await
        .into_iter()
        .find(|r| r.url.path() == "/webapi/basket/AddToBasket")
        .expect("the write was made");
    assert!(write.headers.get("referer").is_some());
}

#[tokio::test]
async fn a_product_number_that_is_not_a_number_never_reaches_the_server() {
    let (client, server) = mock().await;
    assert!("abc".parse::<nemlig::ProductId>().is_err());
    assert_eq!(hits(&server, "/webapi/basket/AddToBasket").await, 0);
    let _ = client;
}
