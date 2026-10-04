//! Searching the catalogue and reading one product.

use nemlig::ProductId;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};

use crate::error::{Subject, failed};
use crate::params::{GetProduct, SearchProducts, SuggestQueries};
use crate::render;
use crate::server::NemligServer;

#[tool_router(router = catalogue_read_router, vis = "pub(crate)")]
impl NemligServer {
    /// Products matching a Danish search term.
    #[tool(
        name = "nemlig_search_products",
        description = "Searches nemlig.com's catalogue and returns matching products with \
                       their number, name, price, unit price, size, stock and any running \
                       offer. Prices are quoted for the account's current delivery slot. \
                       The total says how many matched, which can be far more than were \
                       returned. Product numbers from here are what the basket tools take.",
        annotations(
            title = "Search products",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    pub async fn search_products(
        &self,
        Parameters(p): Parameters<SearchProducts>,
    ) -> CallToolResult {
        let results = match self
            .client
            .search()
            .query(p.query.clone())
            .take(p.limit.unwrap_or(20))
            .skip(p.skip.unwrap_or(0))
            .recipes(p.recipes.unwrap_or(0))
            .send()
            .await
        {
            Ok(results) => results,
            Err(err) => return failed(&err, Subject::account("the catalogue")),
        };

        // The envelope's total is what the gateway found, not what it sent, so a caller can
        // tell "there are three of these" from "there are three hundred, here are twenty".
        render::page_of(
            &results.products.products,
            results.products.num_found as usize,
            self.limits,
        )
    }

    /// What the website's search box would offer for a half-typed term.
    #[tool(
        name = "nemlig_suggest_queries",
        description = "Returns the completions and catalogue sections nemlig.com's own \
                       search box offers for a partial Danish term. Cheaper than a search \
                       and answers with words rather than products, which is what turns a \
                       half-remembered or misspelled term into one the catalogue knows.",
        annotations(
            title = "Suggest search terms",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    pub async fn suggest_queries(
        &self,
        Parameters(p): Parameters<SuggestQueries>,
    ) -> CallToolResult {
        match self.client.search().query(p.query).suggest().await {
            Ok(suggestions) => render::json(&suggestions),
            Err(err) => failed(&err, Subject::account("the search suggestions")),
        }
    }

    /// One product in full.
    #[tool(
        name = "nemlig_get_product",
        description = "Returns one product's full page: price, unit price, stock, Danish \
                       labels and attributes such as Allergener and Oprindelse, the \
                       ingredient list, the nutrition table, any running offer, and the \
                       products nemlig.com suggests alongside or instead. Takes a product \
                       number or the slug its page lives at.",
        annotations(
            title = "Get product",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    pub async fn get_product(&self, Parameters(p): Parameters<GetProduct>) -> CallToolResult {
        let subject = format!("product {}", p.product);
        // A bare number is a product number; anything else is already a slug, which saves
        // the search that resolving a number costs.
        let details = match p.product.parse::<ProductId>() {
            Ok(id) => self.client.products().get(&id).await,
            Err(_) => self.client.products().get_by_url(&p.product).await,
        };

        match details {
            Ok(details) => render::json(&details),
            Err(err) => failed(&err, Subject::new(&subject, "nemlig_search_products")),
        }
    }
}
