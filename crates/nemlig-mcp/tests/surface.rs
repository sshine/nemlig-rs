//! The rules the tool surface has to keep, as assertions rather than as a checklist.

#![allow(clippy::expect_used)]
//!
//! These are the MCP directory's review criteria plus the ones this server sets for itself.
//! Every one of them is the kind of thing that is easy to forget on the tool added next
//! month and invisible until a client behaves oddly in front of a user.

mod common;

use common::serve;
use nemlig_mcp::server::{Mode, NemligServer};
use rmcp::model::Tool;

fn everything() -> Vec<Tool> {
    NemligServer::router(Mode { writes: true }).list_all()
}

#[test]
fn every_tool_states_its_annotations_rather_than_leaving_them_to_be_assumed() {
    // rmcp leaves each hint as None unless it is given, and a host then assumes the worst:
    // an unannotated read is treated as destructive and prompts the user for nothing.
    for tool in everything() {
        let name = &tool.name;
        let annotations = tool
            .annotations
            .unwrap_or_else(|| panic!("{name} has no annotations at all"));

        assert!(annotations.title.is_some(), "{name} has no title");
        assert!(
            annotations.read_only_hint.is_some(),
            "{name} does not say whether it reads only"
        );
        assert!(
            annotations.destructive_hint.is_some(),
            "{name} does not say whether it is destructive"
        );
        assert!(
            annotations.open_world_hint.is_some(),
            "{name} does not say whether it reaches an open world"
        );
    }
}

#[test]
fn nothing_claims_to_be_both_read_only_and_destructive() {
    for tool in everything() {
        let Some(annotations) = tool.annotations else {
            continue;
        };
        if annotations.read_only_hint == Some(true) {
            assert_eq!(
                annotations.destructive_hint,
                Some(false),
                "{} is read-only but claims to be destructive",
                tool.name
            );
        }
    }
}

#[test]
fn every_name_is_prefixed_and_within_the_length_a_client_will_accept() {
    for tool in everything() {
        let name = tool.name.as_ref();
        assert!(name.len() <= 64, "{name} is over the 64-character limit");
        assert!(
            name.starts_with("nemlig_"),
            "{name} is not prefixed, so it collides in a client that does not namespace"
        );
        assert!(
            name.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'),
            "{name} is not lowercase_with_underscores"
        );
    }
}

#[test]
fn every_tool_and_every_argument_is_described() {
    for tool in everything() {
        let name = &tool.name;
        let description = tool
            .description
            .as_deref()
            .unwrap_or_else(|| panic!("{name} has no description"));
        assert!(
            !description.trim().is_empty(),
            "{name} has an empty description"
        );

        let Some(properties) = tool
            .input_schema
            .get("properties")
            .and_then(|p| p.as_object())
        else {
            continue;
        };
        for (argument, schema) in properties {
            let described = schema
                .get("description")
                .and_then(|d| d.as_str())
                .is_some_and(|d| !d.trim().is_empty());
            assert!(described, "{name}.{argument} has no description");
        }
    }
}

#[test]
fn no_description_tries_to_tell_the_model_how_to_behave() {
    // A description is a manpage entry, not an instruction. Directives here are read as
    // prompt injection at review, and they are what the server's own instructions are for.
    const DIRECTIVES: [&str; 7] = [
        "always ",
        "never ",
        "you should",
        "you must",
        "make sure to",
        "be sure to",
        "do not use",
    ];

    for tool in everything() {
        let lowered = tool
            .description
            .as_deref()
            .unwrap_or_default()
            .to_lowercase();
        for directive in DIRECTIVES {
            assert!(
                !lowered.contains(directive),
                "{} instructs the model: {directive:?}",
                tool.name
            );
        }
    }
}

#[test]
fn the_write_tools_say_in_their_description_that_they_change_the_real_basket() {
    // The account has one basket, shared with the website and the app, so a write here is
    // visible to whoever opens it next. A caller deciding whether to ask the user first
    // reads the description, and it has to say so.
    for tool in everything() {
        let Some(annotations) = &tool.annotations else {
            continue;
        };
        if annotations.read_only_hint == Some(true) {
            continue;
        }
        let description = tool.description.as_deref().unwrap_or_default();
        assert!(
            description.contains("basket"),
            "{} does not say what it changes",
            tool.name
        );
    }
}

#[test]
fn nothing_on_the_surface_can_place_an_order() {
    // Order placement is not compiled into this crate: the library keeps it behind a
    // feature this crate never enables. This asserts the surface agrees.
    const CHECKOUT: [&str; 5] = ["place", "checkout", "pay", "payment", "cards"];

    for tool in everything() {
        let name = tool.name.as_ref();
        // Whole segments rather than substrings: "placements" is where the driver leaves
        // the crates, which has nothing to do with paying for them.
        for segment in name.split('_') {
            assert!(
                !CHECKOUT.contains(&segment),
                "{name} looks like it reaches checkout"
            );
        }
    }
}

#[test]
fn the_whole_surface_stays_inside_what_a_context_window_should_carry() {
    // Past roughly thirty, one tool per operation stops paying for itself and the surface
    // wants a search-and-execute pair instead. Crossing it should be a decision, not a drift.
    let total = everything().len();
    assert!(total <= 32, "the surface has grown to {total} tools");
}

#[tokio::test]
async fn the_served_names_are_exactly_these() {
    // Pinned so that adding or renaming a tool shows up as a deliberate diff here, next to
    // the rules above, rather than only in whatever a client happens to display.
    let h = serve(Mode { writes: true }).await;

    let mut names: Vec<String> = h.tools().await.into_iter().map(|t| t.name.into()).collect();
    names.sort();

    assert_eq!(
        names,
        [
            "nemlig_empty_basket",
            "nemlig_get_account",
            "nemlig_get_basket",
            "nemlig_get_delivery_slot",
            "nemlig_get_order",
            "nemlig_get_product",
            "nemlig_list_delivery_placements",
            "nemlig_list_orders",
            "nemlig_search_products",
            "nemlig_set_basket_quantity",
            "nemlig_suggest_queries",
        ]
    );
}
