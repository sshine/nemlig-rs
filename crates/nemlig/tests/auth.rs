//! The sign-in handshake, and keeping the session alive afterwards.

#![allow(clippy::expect_used)]

mod common;

use std::time::Duration;

use nemlig::{Client, Credentials};
use serde_json::json;
use wiremock::matchers::{body_json_string, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use common::{BEARER, PASSWORD, XSRF, hits, mock, ok, requests, site};

#[tokio::test]
async fn signing_in_takes_the_five_requests_in_order() {
    let (client, server) = mock().await;

    let outcome = client.sign_in().await.expect("signs in");
    assert_eq!(outcome.redirect_url, "/");
    assert!(outcome.merge_successful);

    let paths: Vec<String> = requests(&server)
        .await
        .iter()
        .map(|r| r.url.path().to_owned())
        .collect();
    assert_eq!(
        paths,
        [
            "/webapi/AntiForgery",
            "/webapi/Token",
            "/webapi/login",
            "/webapi/Token",
            "/webapi/AntiForgery",
        ],
        "the anonymous pair authorises the login, then both are fetched again as the account"
    );
}

#[tokio::test]
async fn the_login_body_is_the_one_the_site_expects() {
    let (client, server) = mock().await;
    client.sign_in().await.expect("signs in");

    let login = requests(&server)
        .await
        .into_iter()
        .find(|r| r.url.path() == "/webapi/login")
        .expect("the login request was made");

    let body: serde_json::Value = serde_json::from_slice(&login.body).expect("the body is JSON");
    assert_eq!(body["Username"], "someone@example.com");
    assert_eq!(body["Password"], PASSWORD);
    assert_eq!(body["DoMerge"], true);
    assert_eq!(body["CheckForExistingProducts"], true);
    assert_eq!(body["SaveExistingBasket"], false);

    let referer = login
        .headers
        .get("referer")
        .and_then(|v| v.to_str().ok())
        .expect("a referer was sent");
    assert!(referer.ends_with("/login?returnUrl=%2F"), "got {referer}");
}

#[tokio::test]
async fn the_first_call_signs_in_without_being_asked() {
    let (client, server) = mock().await;
    site("GET", "/webapi/basket/GetBasket")
        .respond_with(ok(common::basket(vec![])))
        .mount(&server)
        .await;

    client.basket().get().await.expect("reads the basket");
    assert_eq!(hits(&server, "/webapi/login").await, 1);
}

#[tokio::test]
async fn a_second_call_reuses_the_session() {
    let (client, server) = mock().await;
    site("GET", "/webapi/basket/GetBasket")
        .respond_with(ok(common::basket(vec![])))
        .mount(&server)
        .await;

    client.basket().get().await.expect("reads the basket");
    client.basket().get().await.expect("reads it again");

    assert_eq!(
        hits(&server, "/webapi/login").await,
        1,
        "one sign-in, not two"
    );
    assert_eq!(hits(&server, "/webapi/basket/GetBasket").await, 2);
}

#[tokio::test]
async fn a_token_about_to_expire_is_renewed_without_the_password() {
    let server = MockServer::start().await;
    common::mount_context(&server).await;

    Mock::given(method("GET"))
        .and(path("/webapi/AntiForgery"))
        .respond_with(ok(json!({"Header": "X-XSRF-TOKEN", "Value": XSRF})))
        .mount(&server)
        .await;
    // Ten seconds is inside the refresh margin, so every request finds the token stale
    // and renews before sending.
    Mock::given(method("GET"))
        .and(path("/webapi/Token"))
        .respond_with(ok(json!({
            "access_token": BEARER.trim_start_matches("Bearer "),
            "expires_in": 10,
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/webapi/login"))
        .respond_with(ok(json!({"RedirectUrl": "/"})))
        .mount(&server)
        .await;

    let client = Client::builder()
        .credentials(Credentials::new("someone@example.com", PASSWORD))
        .base_url(server.uri())
        .search_base_url(format!("{}/searchgateway/api", server.uri()))
        .min_interval(Duration::ZERO)
        .max_retries(0)
        .build()
        .expect("builds");

    site("GET", "/webapi/basket/GetBasket")
        .respond_with(ok(common::basket(vec![])))
        .mount(&server)
        .await;

    client.basket().get().await.expect("first read");
    client.basket().get().await.expect("second read");

    assert_eq!(
        hits(&server, "/webapi/login").await,
        1,
        "the password is used once; the cookies carry the rest"
    );
    assert!(
        hits(&server, "/webapi/Token").await > 2,
        "the token was renewed"
    );
}

#[tokio::test]
async fn a_rejected_session_is_signed_in_again_and_the_request_retried() {
    let (client, server) = mock().await;

    // Refused once, then served. A 401 means the request was never processed, so
    // resending it after signing in again is safe.
    Mock::given(method("GET"))
        .and(path("/webapi/basket/GetBasket"))
        .respond_with(ResponseTemplate::new(401))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    site("GET", "/webapi/basket/GetBasket")
        .respond_with(ok(common::basket(vec![])))
        .mount(&server)
        .await;

    client
        .basket()
        .get()
        .await
        .expect("recovers from the refusal");
    assert_eq!(hits(&server, "/webapi/login").await, 2);
    assert_eq!(hits(&server, "/webapi/basket/GetBasket").await, 2);
}

#[tokio::test]
async fn a_session_refused_twice_gives_up_rather_than_looping() {
    let (client, server) = mock().await;
    Mock::given(method("GET"))
        .and(path("/webapi/basket/GetBasket"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let err = client.basket().get().await.expect_err("gives up");
    assert!(err.is_unauthorized());
    assert_eq!(hits(&server, "/webapi/basket/GetBasket").await, 2);
}

#[tokio::test]
async fn a_wrong_password_is_reported_as_a_rejection_not_a_decode_failure() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/webapi/AntiForgery"))
        .respond_with(ok(json!({"Header": "X-XSRF-TOKEN", "Value": XSRF})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/webapi/Token"))
        .respond_with(ok(json!({"access_token": "anon", "expires_in": 300})))
        .mount(&server)
        .await;
    // What nemlig.com actually sends for a bad password: 200, and no RedirectUrl.
    Mock::given(method("POST"))
        .and(path("/webapi/login"))
        .respond_with(ok(json!({"MergeSuccessful": false})))
        .mount(&server)
        .await;

    let client = Client::builder()
        .credentials(Credentials::new("someone@example.com", PASSWORD))
        .base_url(server.uri())
        .min_interval(Duration::ZERO)
        .build()
        .expect("builds");

    let err = client.sign_in().await.expect_err("is rejected");
    assert!(err.is_unauthorized());
    assert!(
        err.to_string().contains("username and password"),
        "got {err}"
    );
}

#[tokio::test]
async fn an_anti_forgery_response_without_a_token_names_the_step_that_failed() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/webapi/AntiForgery"))
        .respond_with(ok(json!({"Header": "X-XSRF-TOKEN"})))
        .mount(&server)
        .await;

    let client = Client::builder()
        .credentials(Credentials::new("someone@example.com", PASSWORD))
        .base_url(server.uri())
        .min_interval(Duration::ZERO)
        .build()
        .expect("builds");

    let err = client.sign_in().await.expect_err("fails");
    assert!(err.to_string().contains("anti-forgery"), "got {err}");
}

#[tokio::test]
async fn signing_out_forgets_the_session() {
    let (client, server) = mock().await;
    site("GET", "/webapi/basket/GetBasket")
        .respond_with(ok(common::basket(vec![])))
        .mount(&server)
        .await;

    client.basket().get().await.expect("reads the basket");
    client.sign_out().await.expect("signs out");
    client.basket().get().await.expect("reads it again");

    assert_eq!(hits(&server, "/webapi/login").await, 2);
}

#[tokio::test]
async fn ten_calls_at_once_sign_in_once() {
    let (client, server) = mock().await;
    site("GET", "/webapi/basket/GetBasket")
        .respond_with(ok(common::basket(vec![])))
        .mount(&server)
        .await;

    let calls = (0..10).map(|_| {
        let client = client.clone();
        tokio::spawn(async move { client.basket().get().await.map(|_| ()) })
    });
    for call in calls {
        call.await
            .expect("the task ran")
            .expect("the call succeeded");
    }

    assert_eq!(hits(&server, "/webapi/login").await, 1);
}

#[tokio::test]
async fn the_handshake_sends_the_app_headers_the_site_looks_for() {
    let (client, server) = mock().await;
    client.sign_in().await.expect("signs in");

    let first = requests(&server).await.remove(0);
    for (name, value) in [
        ("device-size", "desktop"),
        ("platform", "web"),
        ("version", nemlig::WEB_APP_VERSION),
    ] {
        assert_eq!(
            first.headers.get(name).and_then(|v| v.to_str().ok()),
            Some(value),
            "{name} is missing from the handshake"
        );
    }
    assert!(
        first.headers.get("x-correlation-id").is_some(),
        "every request carries a correlation id"
    );
}

#[tokio::test]
async fn the_session_is_cached_between_clients_when_a_file_is_given() {
    let server = MockServer::start().await;
    common::mount_handshake(&server).await;
    common::mount_context(&server).await;
    site("GET", "/webapi/basket/GetBasket")
        .respond_with(ok(common::basket(vec![])))
        .mount(&server)
        .await;

    let dir = std::env::temp_dir().join(format!("nemlig-auth-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let file = dir.join("session.json");

    let build = || {
        Client::builder()
            .credentials(Credentials::new("someone@example.com", PASSWORD))
            .base_url(server.uri())
            .session_file(file.clone())
            .min_interval(Duration::ZERO)
            .build()
            .expect("builds")
    };

    build().basket().get().await.expect("first process");
    build().basket().get().await.expect("second process");

    assert_eq!(
        hits(&server, "/webapi/login").await,
        1,
        "the second client found the session on disk"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a_request_without_the_anti_forgery_token_does_not_match() {
    // Guards the harness itself: every endpoint mock insists on both tokens, so a client
    // that stopped sending one would fail here rather than pass silently.
    let (client, server) = mock().await;
    Mock::given(method("POST"))
        .and(path("/webapi/basket/AddToBasket"))
        .and(body_json_string(r#"{"ProductId":"1","quantity":1,"AffectPartialQuantity":false,"disableQuantityValidation":false}"#))
        .and(wiremock::matchers::header("x-xsrf-token", XSRF))
        .respond_with(ok(common::basket(vec![])))
        .mount(&server)
        .await;

    let id = "1".parse().expect("valid");
    client
        .basket()
        .set_quantity(&id, 1)
        .await
        .expect("the write carries both tokens");
}

#[tokio::test]
async fn a_cached_session_the_server_refuses_is_not_read_back_off_disk() {
    // The cache holds exactly the session that was just rejected, so restoring it would
    // retry with what the server has already refused and fail the same way. The sign-in has
    // to win over the cache.
    let server = MockServer::start().await;
    common::mount_handshake(&server).await;
    common::mount_context(&server).await;

    Mock::given(method("GET"))
        .and(path("/webapi/basket/GetBasket"))
        .respond_with(ResponseTemplate::new(401))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    site("GET", "/webapi/basket/GetBasket")
        .respond_with(ok(common::basket(vec![])))
        .mount(&server)
        .await;

    let dir = std::env::temp_dir().join(format!("nemlig-refused-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    let client = Client::builder()
        .credentials(Credentials::new("someone@example.com", PASSWORD))
        .base_url(server.uri())
        .session_file(dir.join("session.json"))
        .min_interval(Duration::ZERO)
        .max_retries(0)
        .build()
        .expect("builds");

    client
        .basket()
        .get()
        .await
        .expect("the refusal is answered with a fresh sign-in, not the cached session");
    assert_eq!(hits(&server, "/webapi/login").await, 2);

    let _ = std::fs::remove_dir_all(&dir);
}
