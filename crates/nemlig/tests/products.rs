//! Reading one product, and the slug lookup that costs the extra request.

#![allow(clippy::expect_used)]

mod common;

use serde_json::json;
use wiremock::matchers::query_param;

use common::{TIMESLOT, gateway, hits, mock, ok, product, search_results, site};

fn product_page(id: &str) -> serde_json::Value {
    json!({
        "MetaData": {"Name": "Cocio"},
        "content": [
            {"TemplateName": "breadcrumbspot", "Items": []},
            {
                "TemplateName": "productdetailspot",
                "Id": id,
                "Name": "Cocio kakaomælk",
                "Brand": "Cocio",
                "Url": format!("cocio-kakaomaelk-{id}"),
                "Price": 23.75,
                "Text": "<p>Cocio Classic er den originale…</p>",
                "Labels": ["Produceret i Danmark"],
                "Attributes": [
                    {"Name": "Smag", "Value": "Classic"},
                    {"Name": "Allergener", "Value": "Mælk"},
                ],
                "Declarations": {
                    "Ingredients": "Mælk, sukker, kakao",
                    "NutritionFacts": [{"Name": "Energi", "Value": "276 kJ / 65 kcal"}],
                },
            },
        ],
    })
}

#[tokio::test]
async fn a_known_slug_is_read_in_one_request() {
    let (client, server) = mock().await;
    site("GET", "/cocio-kakaomaelk-701025")
        .and(query_param("GetAsJson", "1"))
        .and(query_param("t", TIMESLOT))
        .respond_with(ok(product_page("701025")))
        .mount(&server)
        .await;

    let details = client
        .products()
        .get_by_url("cocio-kakaomaelk-701025")
        .await
        .expect("reads the product");

    assert_eq!(details.id.as_str(), "701025");
    assert_eq!(details.attributes.len(), 2);
    assert_eq!(hits(&server, "/searchgateway/api/search").await, 0);
}

#[tokio::test]
async fn a_leading_slash_on_the_slug_is_tolerated() {
    let (client, server) = mock().await;
    site("GET", "/cocio-kakaomaelk-701025")
        .respond_with(ok(product_page("701025")))
        .mount(&server)
        .await;

    client
        .products()
        .get_by_url("/cocio-kakaomaelk-701025")
        .await
        .expect("reads the product");
}

#[tokio::test]
async fn a_product_number_is_resolved_through_search_first() {
    let (client, server) = mock().await;
    gateway("search")
        .and(query_param("query", "701025"))
        .respond_with(ok(search_results(vec![
            product("999999", "Noget andet", 10.0),
            product("701025", "Cocio kakaomaelk", 23.75),
        ])))
        .mount(&server)
        .await;
    site("GET", "/cocio-kakaomaelk-701025")
        .respond_with(ok(product_page("701025")))
        .mount(&server)
        .await;

    let id = "701025".parse().expect("valid");
    let details = client.products().get(&id).await.expect("reads the product");

    assert_eq!(details.id.as_str(), "701025");
    assert_eq!(hits(&server, "/searchgateway/api/search").await, 1);
    assert_eq!(hits(&server, "/cocio-kakaomaelk-701025").await, 1);
}

#[tokio::test]
async fn a_number_the_catalogue_does_not_know_is_a_not_found() {
    let (client, server) = mock().await;
    gateway("search")
        .respond_with(ok(search_results(vec![product(
            "999999",
            "Noget andet",
            10.0,
        )])))
        .mount(&server)
        .await;

    let id = "701025".parse().expect("valid");
    let err = client.products().get(&id).await.expect_err("finds nothing");
    assert!(err.is_not_found());
    assert!(err.to_string().contains("701025"), "got {err}");
}

#[tokio::test]
async fn a_page_that_is_not_a_product_page_is_a_not_found() {
    let (client, server) = mock().await;
    site("GET", "/en-artikel")
        .respond_with(ok(json!({"content": [{"TemplateName": "articlespot"}]})))
        .mount(&server)
        .await;

    let err = client
        .products()
        .get_by_url("en-artikel")
        .await
        .expect_err("is not a product");
    assert!(err.is_not_found());
}

#[tokio::test]
async fn the_danish_declaration_arrives_unchanged() {
    let (client, server) = mock().await;
    site("GET", "/cocio-kakaomaelk-701025")
        .respond_with(ok(product_page("701025")))
        .mount(&server)
        .await;

    let details = client
        .products()
        .get_by_url("cocio-kakaomaelk-701025")
        .await
        .expect("reads the product");

    let declarations = details.declarations.expect("there is a declaration");
    assert_eq!(
        declarations.ingredients.as_deref(),
        Some("Mælk, sukker, kakao")
    );
    assert_eq!(
        declarations
            .nutrition_facts
            .first()
            .map(|n| n.name.as_str()),
        Some("Energi")
    );
    assert_eq!(details.labels, ["Produceret i Danmark"]);
    assert!(
        details
            .text
            .as_deref()
            .is_some_and(|t| t.starts_with("<p>"))
    );
}
