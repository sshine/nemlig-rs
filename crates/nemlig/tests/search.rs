//! Searching, and the page settings search cannot be made without.

#![allow(clippy::expect_used)]

mod common;

use serde_json::json;
use wiremock::matchers::query_param;

use common::{
    TIMESLOT, TIMESTAMP, USER_ID, gateway, hits, mock, ok, product, requests, search_results,
};

#[tokio::test]
async fn results_come_out_of_the_doubled_products_key() {
    let (client, server) = mock().await;
    gateway("search")
        .respond_with(ok(search_results(vec![
            product("5070417", "Cocio kakaomælk", 119.0),
            product("701025", "Cocio kakaomælk lille", 23.75),
        ])))
        .mount(&server)
        .await;

    let results = client
        .search()
        .query("cocio")
        .send()
        .await
        .expect("searches");
    assert_eq!(results.products.num_found, 2);
    assert_eq!(results.products.products.len(), 2);
    assert_eq!(results.products.products[0].id.as_str(), "5070417");
    assert!(results.products.products[0].orderable());
}

#[tokio::test]
async fn the_page_settings_are_passed_through_to_the_gateway() {
    let (client, server) = mock().await;
    gateway("search")
        .and(query_param("query", "mælk"))
        .and(query_param("take", "5"))
        .and(query_param("skip", "10"))
        .and(query_param("recipeCount", "3"))
        .and(query_param("timestamp", TIMESTAMP))
        .and(query_param("timeslotUtc", TIMESLOT))
        .and(query_param("deliveryZoneId", "1"))
        .and(query_param("includeFavorites", USER_ID))
        .respond_with(ok(search_results(vec![])))
        .mount(&server)
        .await;

    client
        .search()
        .query("mælk")
        .take(5)
        .skip(10)
        .recipes(3)
        .send()
        .await
        .expect("the gateway got every parameter");
}

#[tokio::test]
async fn turning_favourites_off_drops_the_account_id_from_the_query() {
    let (client, server) = mock().await;
    gateway("search")
        .respond_with(ok(search_results(vec![])))
        .mount(&server)
        .await;

    client
        .search()
        .query("mælk")
        .favorites(false)
        .send()
        .await
        .expect("searches");

    let query = requests(&server)
        .await
        .into_iter()
        .find(|r| r.url.path().ends_with("/search"))
        .map(|r| r.url.query().unwrap_or_default().to_owned())
        .expect("the search was made");
    assert!(!query.contains("includeFavorites"), "got {query}");
}

#[tokio::test]
async fn the_page_settings_are_fetched_once_and_reused() {
    let (client, server) = mock().await;
    gateway("search")
        .respond_with(ok(search_results(vec![])))
        .mount(&server)
        .await;

    for _ in 0..3 {
        client
            .search()
            .query("mælk")
            .send()
            .await
            .expect("searches");
    }

    assert_eq!(
        hits(&server, "/").await,
        1,
        "three searches, one read of the settings"
    );
}

#[tokio::test]
async fn a_cold_search_costs_two_requests_and_a_warm_one_costs_one() {
    let (client, server) = mock().await;
    gateway("search")
        .respond_with(ok(search_results(vec![])))
        .mount(&server)
        .await;

    client
        .search()
        .query("mælk")
        .send()
        .await
        .expect("searches");
    let after_first = client.requests_made();
    client
        .search()
        .query("brød")
        .send()
        .await
        .expect("searches again");

    // Five for the sign-in, one for the settings, one for the search.
    assert_eq!(after_first, 7);
    assert_eq!(client.requests_made() - after_first, 1);
}

#[tokio::test]
async fn suggestions_pass_the_catalogue_version_as_the_correlation_id() {
    let (client, server) = mock().await;
    gateway("quick")
        .and(query_param("query", "coc"))
        .and(query_param("correlationId", TIMESTAMP))
        .respond_with(ok(json!({
            "SearchQuery": "coc",
            "Categories": [{"Name": "Cocio", "Url": "/dagligvarer/brands/cocio"}],
            "Suggestions": ["cocio", "cocopops"],
        })))
        .mount(&server)
        .await;

    let suggestions = client
        .search()
        .query("coc")
        .suggest()
        .await
        .expect("suggests");
    assert_eq!(suggestions.suggestions, ["cocio", "cocopops"]);
    assert_eq!(suggestions.categories.len(), 1);
}

#[tokio::test]
async fn the_gateway_is_not_sent_the_websites_own_headers() {
    let (client, server) = mock().await;
    gateway("search")
        .respond_with(ok(search_results(vec![])))
        .mount(&server)
        .await;

    client
        .search()
        .query("mælk")
        .send()
        .await
        .expect("searches");

    let request = requests(&server)
        .await
        .into_iter()
        .find(|r| r.url.path().ends_with("/search"))
        .expect("the search was made");
    for absent in ["x-xsrf-token", "version", "platform", "device-size"] {
        assert!(
            request.headers.get(absent).is_none(),
            "{absent} should not reach the search gateway"
        );
    }
    assert!(request.headers.get("authorization").is_some());
}
