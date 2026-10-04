//! Tests that talk to the real nemlig.com.
//!
//! Every test is `#[ignore]`d rather than feature-gated, so `--all-features` cannot switch
//! them on by accident; `just live-test` is what runs them, and it resolves the credentials
//! through secretspec. A missing credential panics rather than skipping, because a live
//! suite that quietly runs nothing is the same as no live suite.
//!
//! Everything here reads, except the one test that changes the basket. That one is gated on
//! `NEMLIG_TEST_BASKET`, operates on `NEMLIG_TEST_PRODUCT_ID`, and puts the quantity back
//! where it found it. Nothing here can place an order: the `place-order` feature is off and
//! the client is not built with `allow_place_order`.

#![allow(clippy::expect_used)]

use std::path::PathBuf;
use std::time::Duration;

use nemlig::{Client, Credentials, ProductId};

/// A client for the real API, sharing one session with the rest of the suite.
///
/// The session file means the suite signs in once rather than once per test, which is both
/// faster and closer to how the crate is meant to be used.
fn client() -> Client {
    Client::builder()
        .credentials(Credentials::new(
            require("NEMLIG_USERNAME"),
            require("NEMLIG_PASSWORD"),
        ))
        .session_file(session_file())
        .timeout(Duration::from_secs(30))
        .build()
        .expect("the live client builds")
}

fn session_file() -> PathBuf {
    std::env::temp_dir().join("nemlig-live-test-session.json")
}

fn require(name: &str) -> String {
    match std::env::var(name) {
        Ok(value) if !value.is_empty() => value,
        _ => panic!(
            "{name} is not set. Run the live suite with `just live-test`, which resolves \
             the credentials named in secretspec.toml through your configured provider."
        ),
    }
}

fn opt(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

#[tokio::test]
#[ignore = "talks to the real API"]
async fn signing_in_is_accepted() {
    let outcome = client()
        .sign_in()
        .await
        .expect("the credentials are accepted");
    println!(
        "redirect: {}, zone: {:?}",
        outcome.redirect_url, outcome.delivery_zone_id
    );
}

#[tokio::test]
#[ignore = "talks to the real API"]
async fn the_page_settings_name_a_delivery_zone() {
    let context = client()
        .account()
        .context()
        .await
        .expect("reads the settings");
    println!(
        "zone {}, postcode {:?}, slot {:?}",
        context.delivery_zone_id, context.zip_code, context.timeslot_utc
    );
    assert!(
        context.user_id().is_some(),
        "a signed-in session has an account id"
    );
}

#[tokio::test]
#[ignore = "talks to the real API"]
async fn a_staple_is_in_the_catalogue() {
    let results = client()
        .search()
        .query("mælk")
        .take(5)
        .send()
        .await
        .expect("searches");

    assert!(
        results.products.num_found > 0,
        "the catalogue has milk in it"
    );
    for product in &results.products.products {
        println!(
            "{:>9}  {:<50} {:>8.2} kr",
            product.id, product.name, product.price
        );
    }
}

#[tokio::test]
#[ignore = "talks to the real API"]
async fn the_search_box_suggests_completions() {
    let suggestions = client()
        .search()
        .query("mæl")
        .suggest()
        .await
        .expect("suggests");
    println!("{:?}", suggestions.suggestions);
}

#[tokio::test]
#[ignore = "talks to the real API"]
async fn a_product_found_by_search_can_be_read_by_number() {
    let client = client();
    let results = client
        .search()
        .query("mælk")
        .take(1)
        .send()
        .await
        .expect("searches");
    let first = results
        .products
        .products
        .first()
        .expect("at least one match");

    let details = client
        .products()
        .get_by_url(&first.url)
        .await
        .expect("reads the product page");
    assert_eq!(details.id, first.id);
    println!("{} — {:?}", details.name, details.declarations.is_some());
}

#[tokio::test]
#[ignore = "talks to the real API"]
async fn the_basket_is_readable() {
    let basket = client().basket().get().await.expect("reads the basket");
    println!("{} lines, {:.2} kr", basket.lines.len(), basket.total());
}

#[tokio::test]
#[ignore = "talks to the real API"]
async fn past_orders_are_readable() {
    let client = client();
    let history = client
        .orders()
        .history(0, 5)
        .await
        .expect("reads the history");
    println!("{} pages of orders", history.number_of_pages);

    let Some(order) = history.orders.first().and_then(|o| o.id) else {
        println!("the account has no orders yet");
        return;
    };
    let lines = client.orders().lines(order).await.expect("reads the lines");
    println!("order {order} held {} lines", lines.len());
}

#[tokio::test]
#[ignore = "talks to the real API"]
async fn the_delivery_arrangements_are_readable() {
    let client = client();
    match client.delivery().spot().await.expect("reads the slot") {
        Some(spot) => println!("{:?} {:?}", spot.state, spot.time_slot),
        None => println!("nothing booked"),
    }
    let placements = client
        .delivery()
        .placements()
        .await
        .expect("reads the placements");
    assert!(
        !placements.is_empty(),
        "there is always somewhere to leave the crates"
    );
}

#[tokio::test]
#[ignore = "talks to the real API"]
async fn the_payment_cards_are_readable() {
    let cards = client()
        .checkout()
        .credit_cards()
        .await
        .expect("reads the cards");
    for card in &cards {
        println!("{:?} {:?}", card.card_type, card.card_mask);
    }
}

#[tokio::test]
#[ignore = "talks to the real API"]
async fn a_product_can_be_added_to_the_basket_and_taken_out_again() {
    let Some(raw) = opt("NEMLIG_TEST_BASKET").and(opt("NEMLIG_TEST_PRODUCT_ID")) else {
        println!(
            "skipped: set NEMLIG_TEST_BASKET and NEMLIG_TEST_PRODUCT_ID to run the test \
             that changes the real basket"
        );
        return;
    };
    let product: ProductId = raw
        .parse()
        .expect("NEMLIG_TEST_PRODUCT_ID is a product number");

    let client = client();
    let before = client.basket().get().await.expect("reads the basket");
    let original = before.line(&product).map(|line| line.quantity).unwrap_or(0);

    let added = client
        .basket()
        .set_quantity(&product, original + 1)
        .await
        .expect("adds one");
    assert_eq!(
        added.line(&product).map(|line| line.quantity),
        Some(original + 1),
        "the quantity is absolute, not a delta"
    );

    let restored = client
        .basket()
        .set_quantity(&product, original)
        .await
        .expect("puts it back");
    assert_eq!(
        restored
            .line(&product)
            .map(|line| line.quantity)
            .unwrap_or(0),
        original,
        "the basket is as it was found"
    );
}
