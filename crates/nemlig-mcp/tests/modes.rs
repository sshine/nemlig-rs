//! What each mode serves, and what it refuses to serve.

#![allow(clippy::expect_used)]

mod common;

use common::serve;
use nemlig_mcp::server::Mode;

async fn names(mode: Mode) -> Vec<String> {
    let h = serve(mode).await;
    let mut names: Vec<String> = h.tools().await.into_iter().map(|t| t.name.into()).collect();
    names.sort();
    names
}

#[tokio::test]
async fn the_default_mode_serves_only_reads() {
    assert_eq!(
        names(Mode::default()).await,
        [
            "nemlig_get_account",
            "nemlig_get_basket",
            "nemlig_get_delivery_slot",
            "nemlig_get_order",
            "nemlig_get_product",
            "nemlig_list_delivery_placements",
            "nemlig_list_orders",
            "nemlig_search_products",
            "nemlig_suggest_queries",
        ]
    );
}

#[tokio::test]
async fn allowing_writes_adds_the_basket_tools_and_nothing_else() {
    let reading = names(Mode::default()).await;
    let writing = names(Mode { writes: true }).await;

    let added: Vec<&String> = writing.iter().filter(|n| !reading.contains(n)).collect();
    assert_eq!(added, ["nemlig_empty_basket", "nemlig_set_basket_quantity"]);
}

#[tokio::test]
async fn a_write_tool_cannot_be_called_by_name_when_it_was_not_served() {
    // Composed rather than filtered, so the tool does not exist to be reached: a client
    // that remembers the name from another session gets a refusal, not a write.
    let h = serve(Mode::default()).await;
    let result = h
        .mcp
        .call_tool(rmcp::model::CallToolRequestParams::new(
            "nemlig_set_basket_quantity",
        ))
        .await;
    assert!(result.is_err(), "a tool that was never built answered");
}

#[tokio::test]
async fn the_instructions_tell_the_client_which_tier_is_missing() {
    let h = serve(Mode::default()).await;
    let info = h.mcp.peer_info().expect("the server sent its info");
    let instructions = info.instructions.as_deref().unwrap_or_default();
    assert!(instructions.contains("--allow-writes"), "{instructions}");
    assert!(instructions.contains("place an order"), "{instructions}");
}
