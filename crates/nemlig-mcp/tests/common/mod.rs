//! Shared harness: a mocked nemlig.com underneath, a real MCP client and server on top.
//!
//! The lower half duplicates `crates/nemlig/tests/common/mod.rs`. A `tests/common` module
//! belongs to one crate and cannot be reached from another, and the alternatives are a
//! third crate published for nobody or a test-support feature on a public API that has
//! been kept deliberately small. Copying the fixtures is the cheaper of the three.

#![allow(clippy::expect_used, dead_code)]

use std::time::Duration;

use nemlig::{Client, Credentials};
use nemlig_mcp::render::OutputLimits;
use nemlig_mcp::server::{Mode, NemligServer};
use rmcp::model::{CallToolRequestParams, CallToolResult, Tool};
use rmcp::service::RunningService;
use rmcp::{RoleClient, ServiceExt as _};
use serde_json::{Value, json};
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockBuilder, MockServer, ResponseTemplate};

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

/// A mocked API, and an MCP client talking to a server that talks to it.
pub struct Harness {
    /// Mount expectations here.
    pub http: MockServer,
    /// Call tools through here.
    pub mcp: RunningService<RoleClient, ()>,
}

impl Harness {
    /// Call a tool and return whatever it answered, error results included.
    pub async fn call(&self, name: &'static str, arguments: Value) -> CallToolResult {
        let mut request = CallToolRequestParams::new(name);
        if let Value::Object(map) = arguments {
            request = request.with_arguments(map);
        }
        self.mcp
            .call_tool(request)
            .await
            .expect("the call itself should reach the tool")
    }

    /// The structured half of a successful result.
    pub async fn read(&self, name: &'static str, arguments: Value) -> Value {
        let result = self.call(name, arguments).await;
        assert_ne!(result.is_error, Some(true), "{name} failed: {result:?}");
        result
            .structured_content
            .expect("a read tool should answer with structured content")
    }

    /// The text of a failed result.
    pub async fn error(&self, name: &'static str, arguments: Value) -> String {
        let result = self.call(name, arguments).await;
        assert_eq!(result.is_error, Some(true), "{name} unexpectedly succeeded");
        text(&result)
    }

    /// Every tool this server serves.
    pub async fn tools(&self) -> Vec<Tool> {
        self.mcp
            .list_all_tools()
            .await
            .expect("the server should list its tools")
    }
}

/// Start a mocked nemlig.com and a server serving `mode` against it.
pub async fn serve(mode: Mode) -> Harness {
    serve_with(mode, OutputLimits::default()).await
}

/// As [`serve`], with the output budget under test.
pub async fn serve_with(mode: Mode, limits: OutputLimits) -> Harness {
    let http = MockServer::start().await;
    mount_handshake(&http).await;
    mount_context(&http).await;
    let client = client_for(&http);

    // A pipe rather than a subprocess: the same transport the binary uses over stdio, so the
    // framing is exercised, without paying to spawn and shut down a process per test.
    let (server_io, client_io) = tokio::io::duplex(64 * 1024);
    tokio::spawn(async move {
        if let Ok(service) = NemligServer::new(client, mode, limits)
            .serve(server_io)
            .await
        {
            let _ = service.waiting().await;
        }
    });

    let mcp = ().serve(client_io).await.expect("the client should complete the handshake");

    Harness { http, mcp }
}

/// A client pointed at `server`, with no pacing and no retries so nothing sleeps.
pub fn client_for(server: &MockServer) -> Client {
    Client::builder()
        .credentials(Credentials::new("someone@example.com", PASSWORD))
        .base_url(server.uri())
        .search_base_url(format!("{}/searchgateway/api", server.uri()))
        .min_interval(Duration::ZERO)
        .max_retries(0)
        .build()
        .expect("valid configuration")
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
        })))
        .mount(server)
        .await;

    Mock::given(method("POST"))
        .and(path("/webapi/login"))
        .respond_with(ok(json!({"RedirectUrl": "/", "DeliveryZoneId": 1})))
        .mount(server)
        .await;
}

/// Mount the front page, whose JSON carries the settings search cannot do without.
pub async fn mount_context(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/"))
        .and(query_param("GetAsJson", "1"))
        .respond_with(ok(json!({
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

/// The concatenated text of a result, which is where a tool error's message lives.
pub fn text(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|block| block.as_text().map(|text| text.text.clone()))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A search response wrapping the given products.
pub fn search_results(products: Vec<Value>, num_found: usize) -> Value {
    json!({
        "Products": {"Products": products, "Start": 0, "NumFound": num_found},
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
        "Url": format!("produkt-{id}"),
        "Price": price,
        "Description": "12 x 0,25 l / Classic",
        "Availability": {"IsDeliveryAvailable": true, "IsAvailableInStock": true},
        "Labels": ["Produceret i Danmark"],
        "Campaign": null,
    })
}

/// A basket line.
pub fn basket_line(id: &str, name: &str, quantity: u32, price: f64) -> Value {
    json!({
        "Id": id,
        "Name": name,
        "Url": format!("produkt-{id}"),
        "Quantity": quantity,
        "ItemPrice": price,
        "Price": price * f64::from(quantity),
        "UnitPrice": "249,87",
        "UnitPriceLabel": "kr./Kg.",
    })
}

/// A basket holding the given lines.
pub fn basket(lines: Vec<Value>) -> Value {
    json!({
        "BasketGuid": "00000000-0000-0000-0000-000000000001",
        "Id": 1,
        "Lines": lines,
        "Recipes": [],
        "ValidationFailures": [],
    })
}
