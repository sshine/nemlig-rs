//! The `nemlig` binary, driven the way a user drives it.
//!
//! The binary is spawned for real against a mocked nemlig.com, so these cover the parts
//! the library tests cannot: argument handling, where the password is allowed to come
//! from, exit codes and what lands on stdout.

#![cfg(feature = "cli")]
#![allow(clippy::expect_used)]

mod common;

use std::process::Command;

use wiremock::MockServer;

use common::{PASSWORD, basket, basket_line, gateway, ok, product, search_results, site};

/// Run the binary against a mock server, with the credentials in the environment.
fn nemlig(server: &MockServer, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_nemlig"))
        .args(args)
        .arg("--base-url")
        .arg(server.uri())
        .arg("--search-base-url")
        .arg(format!("{}/searchgateway/api", server.uri()))
        // Nothing is kept between runs: a test must not write to the developer's own cache,
        // and each one wants to start from no session anyway.
        .arg("--no-session-file")
        .env("NEMLIG_USERNAME", "someone@example.com")
        .env("NEMLIG_PASSWORD", PASSWORD)
        .env_remove("NEMLIG_SESSION_FILE")
        .output()
        .expect("the binary runs")
}

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

async fn server() -> MockServer {
    let server = MockServer::start().await;
    common::mount_handshake(&server).await;
    common::mount_context(&server).await;
    server
}

#[tokio::test]
async fn search_prints_a_row_for_each_product() {
    let server = server().await;
    gateway("search")
        .respond_with(ok(search_results(vec![
            product("5070417", "Cocio kakaomælk", 119.0),
            product("701025", "Cocio kakaomælk lille", 23.75),
        ])))
        .mount(&server)
        .await;

    let output = nemlig(&server, &["search", "cocio"]);
    assert!(output.status.success(), "{}", stderr(&output));

    let printed = stdout(&output);
    assert!(printed.contains("5070417"), "{printed}");
    assert!(printed.contains("Cocio kakaomælk"), "{printed}");
    assert!(printed.contains("119.00 kr"), "{printed}");
    assert!(printed.contains("2 of 2 shown"), "{printed}");
}

#[tokio::test]
async fn json_prints_the_wire_types_and_nothing_else() {
    let server = server().await;
    gateway("search")
        .respond_with(ok(search_results(vec![product(
            "5070417",
            "Cocio kakaomælk",
            119.0,
        )])))
        .mount(&server)
        .await;

    let output = nemlig(&server, &["search", "cocio", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));

    let parsed: serde_json::Value =
        serde_json::from_str(&stdout(&output)).expect("stdout is one JSON document");
    assert_eq!(parsed["Products"]["Products"][0]["Id"], "5070417");
}

#[tokio::test]
async fn the_basket_is_printed_with_its_total() {
    let server = server().await;
    site("GET", "/webapi/basket/GetBasket")
        .respond_with(ok(basket(vec![
            basket_line("5064654", "Black Coffee", 1, 99.95),
            basket_line("5070417", "Cocio", 2, 59.5),
        ])))
        .mount(&server)
        .await;

    let output = nemlig(&server, &["basket"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let printed = stdout(&output);
    assert!(printed.contains("Black Coffee"), "{printed}");
    assert!(printed.contains("218.95 kr"), "{printed}");
}

#[tokio::test]
async fn setting_a_quantity_reports_the_basket_it_left_behind() {
    let server = server().await;
    site("POST", "/webapi/basket/AddToBasket")
        .respond_with(ok(basket(vec![basket_line("5070417", "Cocio", 3, 59.5)])))
        .mount(&server)
        .await;

    let output = nemlig(&server, &["basket", "set", "5070417", "3"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("3 x"), "{}", stdout(&output));
}

#[tokio::test]
async fn emptying_the_basket_without_confirming_refuses() {
    let server = server().await;
    let output = nemlig(&server, &["basket", "empty"]);

    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("--yes"), "{}", stderr(&output));
    assert_eq!(
        server.received_requests().await.map(|r| r.len()),
        Some(0),
        "nothing should reach the server before the confirmation"
    );
}

#[tokio::test]
async fn a_missing_password_is_a_setup_failure_rather_than_a_call() {
    let server = server().await;
    let output = Command::new(env!("CARGO_BIN_EXE_nemlig"))
        .args(["whoami", "--base-url"])
        .arg(server.uri())
        .arg("--no-session-file")
        .env("NEMLIG_USERNAME", "someone@example.com")
        .env_remove("NEMLIG_PASSWORD")
        .env_remove("NEMLIG_PASSWORD_FILE")
        .output()
        .expect("the binary runs");

    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("NEMLIG_PASSWORD"),
        "{}",
        stderr(&output)
    );
}

#[tokio::test]
async fn the_password_can_come_from_a_file() {
    let server = server().await;
    let path = std::env::temp_dir().join(format!("nemlig-cli-password-{}", std::process::id()));
    // A trailing newline is what an editor or `secretspec get > file` leaves behind, and
    // it must not become part of the password.
    std::fs::write(&path, format!("{PASSWORD}\n")).expect("writes the file");

    let output = Command::new(env!("CARGO_BIN_EXE_nemlig"))
        .args(["whoami", "--base-url"])
        .arg(server.uri())
        .arg("--no-session-file")
        .arg("--password-file")
        .arg(&path)
        .env("NEMLIG_USERNAME", "someone@example.com")
        .env_remove("NEMLIG_PASSWORD")
        .output()
        .expect("the binary runs");

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("1620"), "{}", stdout(&output));
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn a_rejected_call_exits_one_rather_than_two() {
    let server = server().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/webapi/basket/GetBasket"))
        .respond_with(wiremock::ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let output = nemlig(&server, &["basket"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("500"), "{}", stderr(&output));
}

#[tokio::test]
async fn a_product_number_is_told_apart_from_a_slug() {
    let server = server().await;
    site("GET", "/cocio-kakaomaelk-701025")
        .respond_with(ok(serde_json::json!({
            "content": [{
                "TemplateName": "productdetailspot",
                "Id": "701025",
                "Name": "Cocio kakaomælk",
                "Url": "cocio-kakaomaelk-701025",
                "Price": 23.75,
            }],
        })))
        .mount(&server)
        .await;

    // A slug goes straight to the page; a number would have searched for it first.
    let output = nemlig(&server, &["product", "cocio-kakaomaelk-701025"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        stdout(&output).contains("701025 Cocio kakaomælk"),
        "{}",
        stdout(&output)
    );

    let searches = server
        .received_requests()
        .await
        .expect("requests were recorded")
        .iter()
        .filter(|r| r.url.path().ends_with("/search"))
        .count();
    assert_eq!(searches, 0);
}

#[tokio::test]
async fn an_unparseable_product_number_is_treated_as_a_slug() {
    let server = server().await;
    site("GET", "/ikke-et-produkt")
        .respond_with(ok(serde_json::json!({"content": []})))
        .mount(&server)
        .await;

    let output = nemlig(&server, &["product", "ikke-et-produkt"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("no product"),
        "{}",
        stderr(&output)
    );
}
