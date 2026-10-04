//! Past orders.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use serde_json::json;

use crate::error::{Subject, failed};
use crate::params::{GetOrder, ListOrders};
use crate::render;
use crate::server::NemligServer;

#[tool_router(router = orders_read_router, vis = "pub(crate)")]
impl NemligServer {
    /// The account's past orders, newest first.
    #[tool(
        name = "nemlig_list_orders",
        description = "Returns the account's past orders newest first, each with its numeric \
                       id, the order number the website prints, the status, the delivery \
                       window and the totals with and without delivery. The numeric id is \
                       what nemlig_get_order takes; the order number is not.",
        annotations(
            title = "List orders",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn list_orders(&self, Parameters(p): Parameters<ListOrders>) -> CallToolResult {
        match self
            .client
            .orders()
            .history(p.skip.unwrap_or(0), p.limit.unwrap_or(20))
            .await
        {
            Ok(history) => render::page(&history.orders, self.limits),
            Err(err) => failed(&err, Subject::account("the order history")),
        }
    }

    /// One past order and what it contained.
    #[tool(
        name = "nemlig_get_order",
        description = "Returns one past order with every line it contained: product number \
                       and Danish name, the quantity delivered, the Danish unit, what the \
                       line came to and what it would have come to without its offer. \
                       Quantities can be fractional for goods sold by weight, because the \
                       picker delivers what they found. nemlig.com cannot address an order \
                       directly, so this walks the history to find it.",
        annotations(
            title = "Get order",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn get_order(&self, Parameters(p): Parameters<GetOrder>) -> CallToolResult {
        let subject = format!("order {}", p.order_id);
        match self.client.orders().get(p.order_id).await {
            Ok(order) => render::json(&json!({
                "summary": order.summary,
                "delivery_fee": order.summary.delivery_fee(),
                "lines": order.lines,
                "line_count": order.lines.len(),
            })),
            Err(err) => failed(&err, Subject::new(&subject, "nemlig_list_orders")),
        }
    }
}
