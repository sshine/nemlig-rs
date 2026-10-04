//! Reading and changing the basket.

use nemlig::ProductId;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use serde_json::json;

use crate::error::{Subject, failed};
use crate::params::{EmptyBasket, SetBasketQuantity};
use crate::render;
use crate::server::NemligServer;

#[tool_router(router = basket_read_router, vis = "pub(crate)")]
impl NemligServer {
    /// What the basket holds.
    #[tool(
        name = "nemlig_get_basket",
        description = "Returns every line in the account's basket with its product number, \
                       quantity, line price and any offer applied, along with the total for \
                       the goods. Delivery is not included in that total. nemlig.com keeps \
                       one basket per account, so this is the same basket the website and \
                       the app show.",
        annotations(
            title = "Get basket",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn get_basket(&self) -> CallToolResult {
        match self.client.basket().get().await {
            Ok(basket) => render::json(&json!({
                "lines": basket.lines,
                "line_count": basket.lines.len(),
                "total_products_price": basket.total(),
                "delivery_address": basket.delivery_address,
                "validation_failures": basket.validation_failures,
            })),
            Err(err) => failed(&err, Subject::account("the basket")),
        }
    }
}

#[tool_router(router = basket_write_router, vis = "pub(crate)")]
impl NemligServer {
    /// Set how many of a product the basket holds.
    #[tool(
        name = "nemlig_set_basket_quantity",
        description = "Sets how many of one product the account's basket holds, and returns \
                       the basket afterwards. The quantity is the resulting number, not an \
                       amount to add: setting 3 leaves 3 whatever was there before. Setting \
                       0 removes the line, which is how nemlig.com removes things, since it \
                       has no separate remove endpoint. This changes the real basket the \
                       account sees on the website.",
        annotations(
            title = "Set basket quantity",
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn set_basket_quantity(
        &self,
        Parameters(p): Parameters<SetBasketQuantity>,
    ) -> CallToolResult {
        let Ok(product) = p.product_id.parse::<ProductId>() else {
            return render::rejected(format!(
                "{} is not a product number. nemlig_search_products returns the numbers \
                 this takes.",
                p.product_id
            ));
        };

        match self
            .client
            .basket()
            .set_quantity(&product, p.quantity)
            .await
        {
            Ok(basket) => render::json(&json!({
                "product_id": product.as_str(),
                "quantity": basket.line(&product).map_or(0, |line| line.quantity),
                "lines": basket.lines,
                "line_count": basket.lines.len(),
                "total_products_price": basket.total(),
            })),
            Err(err) => failed(&err, Subject::new(&p.product_id, "nemlig_search_products")),
        }
    }

    /// Remove every line from the basket.
    #[tool(
        name = "nemlig_empty_basket",
        description = "Removes every line from the account's basket. Takes the number of \
                       lines the basket currently holds and refuses if it holds a different \
                       number, so a basket that changed since it was read is not emptied by \
                       mistake. nemlig.com has no bulk remove, so this is one request per \
                       line and takes proportionally longer on a full basket.",
        annotations(
            title = "Empty basket",
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn empty_basket(&self, Parameters(p): Parameters<EmptyBasket>) -> CallToolResult {
        let before = match self.client.basket().get().await {
            Ok(basket) => basket,
            Err(err) => return failed(&err, Subject::account("the basket")),
        };

        let held = before.lines.len();
        if held != p.confirm_line_count as usize {
            return render::rejected(format!(
                "The basket holds {held} line(s), not {}. Read it with nemlig_get_basket \
                 and call this again with the number it reports.",
                p.confirm_line_count
            ));
        }

        match self.client.basket().empty().await {
            Ok(basket) if basket.is_empty() => {
                render::confirmation(format!("Removed {held} line(s); the basket is empty."))
            }
            Ok(basket) => render::json(&json!({
                "removed": held - basket.lines.len(),
                "remaining": basket.lines,
            })),
            Err(err) => failed(&err, Subject::account("the basket")),
        }
    }
}
