//! Searching the catalogue.
//!
//! Search lives on a different host from the rest of the API and answers only when told
//! which catalogue version, delivery slot and delivery zone to price against. Those come
//! from the website's page settings, which the client caches, so a search is one request
//! once the first one has paid for them.

use serde::{Deserialize, Serialize};

use crate::client::{Backend, Client};
use crate::error::Result;
use crate::types::{Availability, Campaign, ProductId};

/// How many products a search returns when no count is asked for.
pub const DEFAULT_TAKE: u32 = 20;

impl Client {
    /// Search the catalogue. Returns a builder; [`SearchApi::send`] runs it.
    pub fn search(&self) -> SearchApi<'_> {
        SearchApi {
            client: self,
            query: String::new(),
            take: DEFAULT_TAKE,
            skip: 0,
            recipe_count: 0,
            favorites: true,
        }
    }
}

/// A search under construction.
#[derive(Debug, Clone)]
pub struct SearchApi<'a> {
    client: &'a Client,
    query: String,
    take: u32,
    skip: u32,
    recipe_count: u32,
    favorites: bool,
}

impl<'a> SearchApi<'a> {
    /// What to search for. Danish terms match best; the catalogue is in Danish.
    pub fn query(mut self, query: impl Into<String>) -> Self {
        self.query = query.into();
        self
    }

    /// How many products to return.
    pub fn take(mut self, take: u32) -> Self {
        self.take = take;
        self
    }

    /// How many products to skip, for paging through [`ProductPage::num_found`].
    pub fn skip(mut self, skip: u32) -> Self {
        self.skip = skip;
        self
    }

    /// Ask for recipes alongside the products. None are returned by default.
    pub fn recipes(mut self, count: u32) -> Self {
        self.recipe_count = count;
        self
    }

    /// Whether to mark the account's favourites. On by default.
    ///
    /// There is no endpoint that lists favourites; this is the only way to learn which
    /// products are ones the account has marked.
    pub fn favorites(mut self, favorites: bool) -> Self {
        self.favorites = favorites;
        self
    }

    /// Run the search.
    pub async fn send(self) -> Result<SearchResults> {
        let context = self.client.context().await?;
        let req = self
            .client
            .get(Backend::Search, &["search"])?
            .query("query", &self.query)
            .query("take", self.take)
            .query("skip", self.skip)
            .query("recipeCount", self.recipe_count)
            .query("timestamp", &context.timestamp)
            .maybe_query(
                "timeslotUtc",
                context.timeslot_utc.as_ref().map(|t| t.as_str()),
            )
            .query("deliveryZoneId", context.delivery_zone_id)
            .maybe_query(
                "includeFavorites",
                self.favorites.then(|| context.user_id()).flatten(),
            );
        self.client.send_json(req, "the search results").await
    }

    /// Ask for the suggestions the website's search box offers while typing.
    ///
    /// Cheaper than a search and answers with words rather than products, which makes it
    /// the right call for turning a half-remembered term into one the catalogue knows.
    pub async fn suggest(self) -> Result<Suggestions> {
        let context = self.client.context().await?;
        let req = self
            .client
            .get(Backend::Search, &["quick"])?
            .query("query", &self.query)
            // The gateway calls this a correlation id, but the value the website sends is
            // the catalogue timestamp from app settings, not the per-request trace id that
            // goes in the X-Correlation-Id header.
            .query("correlationId", &context.timestamp);
        self.client.send_json(req, "the search suggestions").await
    }
}

/// What a search answers with.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[non_exhaustive]
pub struct SearchResults {
    /// The matching products, and how many there were in total.
    pub products: ProductPage,
    /// Recipes matching the same term, when [`SearchApi::recipes`] asked for any.
    #[serde(default)]
    pub recipes: Vec<RecipeHit>,
    /// The filters and sort orders the website would offer for this search.
    #[serde(default)]
    pub facets: Option<Facets>,
}

/// One page of product results.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[non_exhaustive]
pub struct ProductPage {
    /// The products on this page.
    #[serde(default)]
    pub products: Vec<Product>,
    /// How far into the result set this page starts.
    #[serde(default)]
    pub start: u32,
    /// How many products matched in total, across all pages.
    #[serde(default)]
    pub num_found: u32,
}

/// A product as search describes it.
///
/// Product lookups answer with [`ProductDetails`](super::products::ProductDetails), which
/// carries the same fields plus the declaration, the attributes and the images.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[non_exhaustive]
pub struct Product {
    /// The product number.
    pub id: ProductId,
    /// Danish product name.
    pub name: String,
    /// Manufacturer, when there is one.
    #[serde(default)]
    pub brand: Option<String>,
    /// Top-level Danish category, such as `Drikke`.
    #[serde(default)]
    pub category: Option<String>,
    /// Danish subcategory, such as `Kakaomælk`.
    #[serde(default)]
    pub sub_category: Option<String>,
    /// Slug the product's own page lives at, with no leading slash.
    #[serde(default)]
    pub url: String,
    /// What one costs, in kroner.
    #[serde(default)]
    pub price: f64,
    /// Price per unit, for comparing sizes.
    #[serde(default)]
    pub unit_price_calc: Option<f64>,
    /// Which unit [`Product::unit_price_calc`] is per, such as `kr./l.`.
    #[serde(default)]
    pub unit_price_label: Option<String>,
    /// Size and variant, such as `12 x 0,25 l / Classic`.
    #[serde(default)]
    pub description: Option<String>,
    /// URL of the product photograph.
    #[serde(default)]
    pub primary_image: Option<String>,
    /// Whether it can be delivered and whether it is in stock.
    #[serde(default)]
    pub availability: Option<Availability>,
    /// Whether the price shown is a reduced one.
    #[serde(default)]
    pub discount_item: bool,
    /// Whether the account has marked this product a favourite.
    #[serde(default)]
    pub favorite: bool,
    /// Danish badges, such as `Produceret i Danmark` or `Øko`.
    #[serde(default)]
    pub labels: Vec<String>,
    /// The offer running on it, if any.
    #[serde(default)]
    pub campaign: Option<Campaign>,
}

impl Product {
    /// Whether the product can actually be ordered right now.
    pub fn orderable(&self) -> bool {
        self.availability
            .as_ref()
            .is_none_or(|a| a.is_delivery_available && a.is_available_in_stock)
    }
}

/// A recipe matching the search term.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[non_exhaustive]
pub struct RecipeHit {
    /// The recipe's identifier, a GUID rather than a product number.
    #[serde(default)]
    pub id: String,
    /// Danish recipe name.
    #[serde(default)]
    pub name: String,
    /// Path to the recipe on the website.
    #[serde(default)]
    pub url: String,
    /// Danish preparation time, such as `10 min`.
    #[serde(default)]
    pub total_time: Option<String>,
    /// How many the recipe serves.
    #[serde(default)]
    pub number_of_persons: Option<u32>,
}

/// The filters and sort orders the website offers for a search.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[non_exhaustive]
pub struct Facets {
    /// How many products the facets were computed over.
    #[serde(default)]
    pub num_found: u32,
    /// The sort orders on offer.
    #[serde(default)]
    pub sorting_list: Vec<SortOption>,
    /// The filter groups, kept as sent: the shape varies by category and this crate has
    /// no way to pass a chosen filter back to the gateway yet.
    #[serde(default)]
    pub facet_groups: Vec<serde_json::Value>,
}

/// One of the sort orders a search can be re-run under.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[non_exhaustive]
pub struct SortOption {
    /// Danish label, such as `Billigst`.
    #[serde(default)]
    pub title: String,
    /// The value the website would send back, such as `price`.
    #[serde(default)]
    pub url_name: String,
    /// Whether this is the order the results came in.
    #[serde(default)]
    pub is_selected: bool,
}

/// What the search box suggests while a term is being typed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[non_exhaustive]
pub struct Suggestions {
    /// The term the suggestions were computed for.
    #[serde(default)]
    pub search_query: String,
    /// Catalogue sections matching the term.
    #[serde(default)]
    pub categories: Vec<SuggestedCategory>,
    /// Completions of the term itself.
    #[serde(default)]
    pub suggestions: Vec<String>,
}

/// A catalogue section the search box would offer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[non_exhaustive]
pub struct SuggestedCategory {
    /// Danish section name.
    #[serde(default)]
    pub name: String,
    /// Path to the section on the website.
    #[serde(default)]
    pub url: String,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn results_are_read_out_of_the_doubled_products_key() {
        let results: SearchResults = serde_json::from_str(
            r#"{"Products":{"Products":[{"Id":"5070417","Name":"Cocio kakaomælk (dåse)",
                 "Brand":"Cocio","Url":"cocio-kakaomaelk-daase-5070417","Price":119.0,
                 "Availability":{"IsDeliveryAvailable":true,"IsAvailableInStock":true},
                 "Campaign":null}],"Start":0,"NumFound":4},
                "Recipes":[{"Id":"fd79","Name":"Cocioshake","Url":"/opskrifter/x",
                 "TotalTime":"10 min","NumberOfPersons":2}]}"#,
        )
        .expect("parses");

        assert_eq!(results.products.num_found, 4);
        let product = results.products.products.first().expect("one product");
        assert_eq!(product.id.as_str(), "5070417");
        assert!(product.orderable());
        assert!(product.campaign.is_none());
        assert_eq!(results.recipes.len(), 1);
    }

    #[test]
    fn a_product_out_of_stock_is_not_orderable() {
        let product: Product = serde_json::from_str(
            r#"{"Id":"1","Name":"x","Availability":
                {"IsDeliveryAvailable":true,"IsAvailableInStock":false}}"#,
        )
        .expect("parses");
        assert!(!product.orderable());
    }

    #[test]
    fn suggestions_read_the_three_keys_the_box_sends() {
        let suggestions: Suggestions = serde_json::from_str(
            r#"{"SearchQuery":"cocio","Categories":[{"Name":"Cocio","Url":"/x"}],
                "Suggestions":["cocio","cocopops"]}"#,
        )
        .expect("parses");
        assert_eq!(suggestions.suggestions.len(), 2);
        assert_eq!(
            suggestions.categories.first().map(|c| c.name.as_str()),
            Some("Cocio")
        );
    }
}
