//! The account: where and when it shops, and what it has booked.

use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use serde_json::json;

use crate::error::{Subject, failed};
use crate::render;
use crate::server::NemligServer;

#[tool_router(router = account_read_router, vis = "pub(crate)")]
impl NemligServer {
    /// Where and when the account shops.
    #[tool(
        name = "nemlig_get_account",
        description = "Returns the postcode, delivery zone and delivery slot the account's \
                       session is browsing in, and the account identifier. These decide \
                       which prices and which stock the catalogue reports, so two accounts \
                       can see different answers to the same search.",
        annotations(
            title = "Get account",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn get_account(&self) -> CallToolResult {
        match self.client.account().context().await {
            Ok(context) => render::json(&json!({
                "username": self.client.credentials().username(),
                "zip_code": context.zip_code,
                "delivery_zone_id": context.delivery_zone_id,
                "timeslot_utc": context.timeslot_utc,
                "user_id": context.user_id(),
            })),
            Err(err) => failed(&err, Subject::account("the account settings")),
        }
    }

    /// The delivery the account has booked.
    #[tool(
        name = "nemlig_get_delivery_slot",
        description = "Returns the delivery the account currently has booked: the order it \
                       belongs to, the window the driver will arrive in, how far along it \
                       is and the deadline for changing it. Answers with nothing when no \
                       delivery is booked. The slots on offer cannot be listed or chosen \
                       from here; nemlig.com exposes no endpoint for that.",
        annotations(
            title = "Get delivery slot",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn get_delivery_slot(&self) -> CallToolResult {
        match self.client.delivery().spot().await {
            Ok(Some(spot)) => render::json(&spot),
            Ok(None) => render::confirmation("The account has no delivery booked."),
            Err(err) => failed(&err, Subject::account("the delivery slot")),
        }
    }

    /// Where the driver may leave the crates.
    #[tool(
        name = "nemlig_list_delivery_placements",
        description = "Returns the places nemlig.com will leave an order when nobody \
                       answers, as a stable key and the Danish phrase the website shows for \
                       it, such as Frontdoor and \"Foran min hoveddør\".",
        annotations(
            title = "List delivery placements",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn list_delivery_placements(&self) -> CallToolResult {
        match self.client.delivery().placements().await {
            Ok(placements) => render::page(&placements, self.limits),
            Err(err) => failed(&err, Subject::account("the delivery placements")),
        }
    }
}
