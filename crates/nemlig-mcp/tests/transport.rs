//! The binary itself: what it writes where, and what it refuses to start without.

#![allow(clippy::expect_used)]

use std::process::{Command, Stdio};

/// `--list-tools` needs no credentials, so it is the one mode that can be driven by hand.
fn list_tools(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_nemlig-mcp"))
        .arg("--list-tools")
        .args(args)
        .env_remove("NEMLIG_USERNAME")
        .env_remove("NEMLIG_PASSWORD")
        .env_remove("NEMLIG_MCP_ALLOW_WRITES")
        .output()
        .expect("the binary runs")
}

#[test]
fn listing_the_tools_needs_no_credentials_and_prints_one_json_document() {
    let output = list_tools(&[]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let tools: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout is one JSON document");
    let names: Vec<&str> = tools
        .as_array()
        .expect("an array of tools")
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();

    assert_eq!(names.len(), 9);
    assert!(names.contains(&"nemlig_search_products"));
    assert!(!names.contains(&"nemlig_set_basket_quantity"));
}

#[test]
fn the_write_tier_appears_only_when_it_is_asked_for() {
    let output = list_tools(&["--allow-writes"]);
    let tools: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout is one JSON document");
    assert_eq!(tools.as_array().map(Vec::len), Some(11));
}

#[test]
fn a_refused_start_exits_two_and_says_what_to_set() {
    let output = Command::new(env!("CARGO_BIN_EXE_nemlig-mcp"))
        .env_remove("NEMLIG_USERNAME")
        .env_remove("NEMLIG_PASSWORD")
        .stdin(Stdio::null())
        .output()
        .expect("the binary runs");

    // Two rather than one, so a supervisor can tell a refused start from a crash.
    assert_eq!(output.status.code(), Some(2));
    assert!(
        output.stdout.is_empty(),
        "stdout is the protocol and must stay clean: {:?}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("NEMLIG_USERNAME"), "{stderr}");
}

#[test]
fn an_empty_credential_counts_as_an_absent_one() {
    // A client that substitutes configuration into the environment sends the variable
    // whether or not the user filled it in, so an empty string arrives where nothing was
    // meant. Starting up with it would fail later, in the middle of a conversation.
    let output = Command::new(env!("CARGO_BIN_EXE_nemlig-mcp"))
        .env("NEMLIG_USERNAME", "")
        .env("NEMLIG_PASSWORD", "")
        .stdin(Stdio::null())
        .output()
        .expect("the binary runs");

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("NEMLIG_USERNAME"));
}

#[test]
fn a_password_on_the_command_line_is_not_accepted() {
    let output = Command::new(env!("CARGO_BIN_EXE_nemlig-mcp"))
        .args(["--password", "hunter2-not-a-real-password"])
        .stdin(Stdio::null())
        .output()
        .expect("the binary runs");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("unexpected argument"),
        "--password must not be a flag; a command line is readable by every process"
    );
}
