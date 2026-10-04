//! How requests go out: pacing, retries and what the two hosts are each told.

#![allow(clippy::expect_used)]

mod common;

use std::time::Duration;

use nemlig::{Client, Credentials};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use common::{PASSWORD, basket, hits, mock, ok, requests, site};

fn client(server: &MockServer, retries: u32) -> Client {
    Client::builder()
        .credentials(Credentials::new("someone@example.com", PASSWORD))
        .base_url(server.uri())
        .search_base_url(format!("{}/searchgateway/api", server.uri()))
        .min_interval(Duration::ZERO)
        .max_retries(retries)
        .max_retry_delay(Duration::from_millis(1))
        .build()
        .expect("builds")
}

#[tokio::test]
async fn a_read_is_retried_after_a_server_error() {
    let server = MockServer::start().await;
    common::mount_handshake(&server).await;
    Mock::given(method("GET"))
        .and(path("/webapi/basket/GetBasket"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    site("GET", "/webapi/basket/GetBasket")
        .respond_with(ok(basket(vec![])))
        .mount(&server)
        .await;

    client(&server, 2)
        .basket()
        .get()
        .await
        .expect("the second attempt succeeds");
    assert_eq!(hits(&server, "/webapi/basket/GetBasket").await, 2);
}

#[tokio::test]
async fn a_read_gives_up_once_the_retries_run_out() {
    let server = MockServer::start().await;
    common::mount_handshake(&server).await;
    Mock::given(method("GET"))
        .and(path("/webapi/basket/GetBasket"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;

    let err = client(&server, 2)
        .basket()
        .get()
        .await
        .expect_err("gives up");
    assert!(err.is_busy());
    assert_eq!(err.status(), Some(503));
    assert_eq!(hits(&server, "/webapi/basket/GetBasket").await, 3);
}

#[tokio::test]
async fn a_rate_limit_is_waited_out_for_as_long_as_the_server_asks() {
    let server = MockServer::start().await;
    common::mount_handshake(&server).await;
    Mock::given(method("GET"))
        .and(path("/webapi/basket/GetBasket"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "0"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    site("GET", "/webapi/basket/GetBasket")
        .respond_with(ok(basket(vec![])))
        .mount(&server)
        .await;

    client(&server, 1)
        .basket()
        .get()
        .await
        .expect("waits and retries");
    assert_eq!(hits(&server, "/webapi/basket/GetBasket").await, 2);
}

#[tokio::test]
async fn a_rate_limit_asking_for_longer_than_allowed_is_an_error_rather_than_a_sleep() {
    let server = MockServer::start().await;
    common::mount_handshake(&server).await;
    Mock::given(method("GET"))
        .and(path("/webapi/basket/GetBasket"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "3600"))
        .mount(&server)
        .await;

    let err = client(&server, 3)
        .basket()
        .get()
        .await
        .expect_err("refuses to wait an hour");
    assert!(err.is_rate_limited());
    assert_eq!(hits(&server, "/webapi/basket/GetBasket").await, 1);
}

#[tokio::test]
async fn an_error_body_is_kept_but_the_query_string_is_not() {
    let (client, server) = mock().await;
    Mock::given(method("GET"))
        .and(path("/webapi/basket/GetBasket"))
        .respond_with(ResponseTemplate::new(400).set_body_string("Ugyldig anmodning"))
        .mount(&server)
        .await;

    let err = client.basket().get().await.expect_err("fails");
    let message = err.to_string();
    assert!(message.contains("Ugyldig anmodning"), "got {message}");
    assert!(
        message.contains("/webapi/basket/GetBasket"),
        "got {message}"
    );
    assert!(
        !message.contains('?'),
        "the query string must not appear: {message}"
    );
}

#[tokio::test]
async fn a_body_that_is_not_what_was_expected_says_what_was_expected() {
    let (client, server) = mock().await;
    site("GET", "/webapi/basket/GetBasket")
        .respond_with(ok(serde_json::json!({"Lines": "ikke en liste"})))
        .mount(&server)
        .await;

    let err = client.basket().get().await.expect_err("fails to decode");
    assert!(err.to_string().contains("the basket"), "got {err}");
}

#[tokio::test]
async fn every_request_gets_its_own_correlation_id() {
    let (client, server) = mock().await;
    site("GET", "/webapi/basket/GetBasket")
        .respond_with(ok(basket(vec![])))
        .mount(&server)
        .await;

    client.basket().get().await.expect("reads");
    client.basket().get().await.expect("reads again");

    let ids: Vec<String> = requests(&server)
        .await
        .iter()
        .filter_map(|r| r.headers.get("x-correlation-id"))
        .filter_map(|v| v.to_str().ok())
        .map(str::to_owned)
        .collect();
    let unique: std::collections::HashSet<&String> = ids.iter().collect();
    assert_eq!(
        ids.len(),
        unique.len(),
        "a correlation id was reused: {ids:?}"
    );
}

#[tokio::test]
async fn the_agent_string_goes_out_on_every_request() {
    let (client, server) = mock().await;
    site("GET", "/webapi/basket/GetBasket")
        .respond_with(ok(basket(vec![])))
        .mount(&server)
        .await;
    client.basket().get().await.expect("reads");

    for request in requests(&server).await {
        assert_eq!(
            request
                .headers
                .get("user-agent")
                .and_then(|v| v.to_str().ok()),
            Some(nemlig::DEFAULT_USER_AGENT),
            "{} went out without an agent string",
            request.url.path()
        );
    }
}

#[tokio::test]
async fn a_custom_agent_string_replaces_the_browser_one() {
    let server = MockServer::start().await;
    common::mount_handshake(&server).await;
    site("GET", "/webapi/basket/GetBasket")
        .respond_with(ok(basket(vec![])))
        .mount(&server)
        .await;

    Client::builder()
        .credentials(Credentials::new("someone@example.com", PASSWORD))
        .base_url(server.uri())
        .user_agent("nemlig-rs-test/1.0")
        .min_interval(Duration::ZERO)
        .build()
        .expect("builds")
        .basket()
        .get()
        .await
        .expect("reads");

    let agents: Vec<String> = requests(&server)
        .await
        .iter()
        .filter_map(|r| r.headers.get("user-agent"))
        .filter_map(|v| v.to_str().ok())
        .map(str::to_owned)
        .collect();
    assert!(
        agents.iter().all(|a| a == "nemlig-rs-test/1.0"),
        "got {agents:?}"
    );
}

#[tokio::test]
async fn an_agent_string_that_cannot_be_a_header_fails_at_build_time() {
    let err = Client::builder()
        .credentials(Credentials::new("a@b.c", PASSWORD))
        .user_agent("line\nbreak")
        .build()
        .expect_err("is refused");
    assert!(err.to_string().contains("user_agent"), "got {err}");
}

#[tokio::test]
async fn a_timeout_is_reported_without_the_url() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/webapi/AntiForgery"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(5)))
        .mount(&server)
        .await;

    let err = Client::builder()
        .credentials(Credentials::new("a@b.c", PASSWORD))
        .base_url(server.uri())
        .timeout(Duration::from_millis(50))
        .min_interval(Duration::ZERO)
        .build()
        .expect("builds")
        .sign_in()
        .await
        .expect_err("times out");

    let message = err.to_string();
    assert!(message.contains("/webapi/AntiForgery"), "got {message}");
    assert!(
        !message.contains(&server.uri()),
        "the full url leaked: {message}"
    );
}

#[tokio::test]
async fn pacing_spaces_requests_out() {
    let server = MockServer::start().await;
    common::mount_handshake(&server).await;
    common::mount_context(&server).await;
    site("GET", "/webapi/basket/GetBasket")
        .respond_with(ok(basket(vec![])))
        .mount(&server)
        .await;

    let client = Client::builder()
        .credentials(Credentials::new("a@b.c", PASSWORD))
        .base_url(server.uri())
        .min_interval(Duration::from_millis(20))
        .build()
        .expect("builds");

    let start = std::time::Instant::now();
    client.basket().get().await.expect("reads");
    // Five handshake requests plus the read, so five gaps at twenty milliseconds.
    assert!(
        start.elapsed() >= Duration::from_millis(100),
        "finished in {:?}, which is faster than pacing allows",
        start.elapsed()
    );
}
