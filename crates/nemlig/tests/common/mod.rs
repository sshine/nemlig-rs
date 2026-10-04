//! A mocked nemlig.com, and a client pointed at it.
//!
//! Both of nemlig.com's hosts are served by the one mock, which works because their paths
//! do not overlap: the website is under `/webapi` and the root, the search gateway under
//! `/searchgateway/api`.
//!
//! Every endpoint mock insists on the tokens the handshake hands out, so a request that
//! forgets one does not quietly match: it fails as an unmatched request naming what it
//! sent.

#![allow(clippy::expect_used, dead_code)]

use std::time::Duration;

use nemlig::{Client, Credentials};
use serde_json::{Value, json};
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockBuilder, MockServer, Request, ResponseTemplate};

/// The bearer token the mocked handshake issues.
pub const BEARER: &str = "Bearer test-access-token";

/// The anti-forgery token the mocked handshake issues.
pub const XSRF: &str = "test-xsrf-token";

/// Not a password anyone's account uses; it exists so the harness looks like a client.
pub const PASSWORD: &str = "hunter2-not-a-real-password";

/// The delivery slot the mocked page settings report.
pub const TIMESLOT: &str = "2025120216-180-1020";

/// The catalogue version the mocked page settings report.
pub const TIMESTAMP: &str = "j0gWvnwu-sP8MfX4u";

/// The account id the mocked page settings report.
pub const USER_ID: &str = "test-user-id";

/// A mock server with the sign-in handshake and page settings already mounted, and a
/// client pointed at it.
///
/// Retries are off and pacing is off, so a test never sleeps.
pub async fn mock() -> (Client, MockServer) {
    let server = MockServer::start().await;
    mount_handshake(&server).await;
    mount_context(&server).await;
    (client(&server), server)
}

/// A mock server with nothing mounted, for tests that want to control the handshake.
pub async fn bare() -> (Client, MockServer) {
    let server = MockServer::start().await;
    (client(&server), server)
}

fn client(server: &MockServer) -> Client {
    Client::builder()
        .credentials(Credentials::new("someone@example.com", PASSWORD))
        .base_url(server.uri())
        .search_base_url(format!("{}/searchgateway/api", server.uri()))
        .min_interval(Duration::ZERO)
        .max_retries(0)
        .build()
        .expect("the test client builds")
}

/// Mount the five requests a sign-in makes.
pub async fn mount_handshake(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/webapi/AntiForgery"))
        .respond_with(ok(json!({"Header": "X-XSRF-TOKEN", "Value": XSRF})))
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path("/webapi/Token"))
        .respond_with(ok(json!({
            "access_token": BEARER.trim_start_matches("Bearer "),
            "expires_in": 300,
            "token_type": "Bearer",
        })))
        .mount(server)
        .await;

    Mock::given(method("POST"))
        .and(path("/webapi/login"))
        .respond_with(ok(json!({
            "RedirectUrl": "/",
            "MergeSuccessful": true,
            "ZipCodeDiffers": false,
            "TimeslotUtc": TIMESLOT,
            "DeliveryZoneId": 1,
            "IsFirstLogin": false,
            "RemovedMealBoxes": [],
        })))
        .mount(server)
        .await;
}

/// Mount the front page, whose JSON carries the settings search cannot do without.
pub async fn mount_context(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/"))
        .and(query_param("GetAsJson", "1"))
        .respond_with(ok(json!({
            "MetaData": {"Name": "Forside"},
            "Settings": {
                "ZipCode": "1620",
                "DeliveryZoneId": 1,
                "TimeslotUtc": TIMESLOT,
                "UserId": USER_ID,
                "CombinedProductsAndSitecoreTimestamp": TIMESTAMP,
            },
        })))
        .mount(server)
        .await;
}

/// A website endpoint, which must carry both tokens and the app headers.
pub fn site(verb: &str, endpoint: &str) -> MockBuilder {
    Mock::given(method(verb))
        .and(path(endpoint.to_owned()))
        .and(header("authorization", BEARER))
        .and(header("x-xsrf-token", XSRF))
        .and(header("version", nemlig::WEB_APP_VERSION))
        .and(header("platform", "web"))
}

/// A search-gateway endpoint, which carries the bearer token and nothing else.
pub fn gateway(endpoint: &str) -> MockBuilder {
    Mock::given(method("GET"))
        .and(path(format!("/searchgateway/api/{endpoint}")))
        .and(header("authorization", BEARER))
}

/// A 200 with a JSON body.
pub fn ok(body: Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(body)
}

/// Every request the server saw, in order.
pub async fn requests(server: &MockServer) -> Vec<Request> {
    server
        .received_requests()
        .await
        .expect("the mock server records requests")
}

/// How many requests reached a given path.
pub async fn hits(server: &MockServer, endpoint: &str) -> usize {
    requests(server)
        .await
        .iter()
        .filter(|r| r.url.path() == endpoint)
        .count()
}

/// A search response wrapping the given products.
pub fn search_results(products: Vec<Value>) -> Value {
    let found = products.len();
    json!({
        "Products": {"Products": products, "Start": 0, "NumFound": found},
        "Facets": {"NumFound": found, "SortingList": [], "FacetGroups": []},
        "Recipes": [],
    })
}

/// A product as search describes it.
pub fn product(id: &str, name: &str, price: f64) -> Value {
    json!({
        "Id": id,
        "Name": name,
        "Brand": "Cocio",
        "Category": "Drikke",
        "SubCategory": "Kakaomælk",
        "Url": format!("{}-{id}", slug(name)),
        "Price": price,
        "UnitPriceCalc": 9.92,
        "UnitPriceLabel": "kr/stk",
        "Description": "12 x 0,25 l / Classic",
        "Availability": {"IsDeliveryAvailable": true, "IsAvailableInStock": true},
        "DiscountItem": false,
        "Favorite": false,
        "Labels": [],
        "Campaign": null,
    })
}

/// A basket line.
pub fn basket_line(id: &str, name: &str, quantity: u32, price: f64) -> Value {
    json!({
        "Id": id,
        "Name": name,
        "Brand": "Cocio",
        "Url": format!("{}-{id}", slug(name)),
        "Quantity": quantity,
        "ItemPrice": price,
        "Price": price * f64::from(quantity),
        "UnitPrice": "249,87",
        "UnitPriceCalc": 249.88,
        "UnitPriceLabel": "kr./Kg.",
        "Labels": ["Produceret i Danmark"],
    })
}

/// A basket holding the given lines.
pub fn basket(lines: Vec<Value>) -> Value {
    json!({
        "BasketGuid": "00000000-0000-0000-0000-000000000001",
        "Id": 1,
        "NumberOfProducts": lines.len(),
        "Lines": lines,
        "Recipes": [],
        "ValidationFailures": [],
    })
}

fn slug(name: &str) -> String {
    name.to_lowercase()
        .replace(['æ', 'å'], "aa")
        .replace('ø', "oe")
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}
