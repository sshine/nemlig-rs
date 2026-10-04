//! Reading one product.
//!
//! A product's own page is addressed by a slug that ends in its number, such as
//! `cocio-kakaomaelk-daase-5070417`, and there is no endpoint that maps a number to a
//! slug. [`ProductsApi::get`] therefore searches for the number first and costs two
//! requests; [`ProductsApi::get_by_url`] costs one, and the slug is on every product a
//! search, a basket or an order already returned.

use serde::{Deserialize, Serialize};

use crate::client::{Backend, Client};
use crate::error::{Error, Result};
use crate::types::{Availability, Campaign, ProductId};

/// Which entry of the page JSON carries the product.
///
/// The website serves one document per page containing every block it renders; the
/// product is the block with this template.
const PRODUCT_TEMPLATE: &str = "productdetailspot";

impl Client {
    /// Read a single product.
    pub fn products(&self) -> ProductsApi<'_> {
        ProductsApi::new(self)
    }
}

/// Reads about one product.
#[derive(Debug, Clone, Copy)]
pub struct ProductsApi<'a> {
    client: &'a Client,
}

impl<'a> ProductsApi<'a> {
    pub(crate) fn new(client: &'a Client) -> Self {
        Self { client }
    }

    /// Read a product by its number.
    ///
    /// Two requests: one to find the slug the product's page lives at, one to read it.
    /// Where the slug is already known, [`ProductsApi::get_by_url`] skips the first.
    pub async fn get(&self, id: &ProductId) -> Result<ProductDetails> {
        let results = self
            .client
            .search()
            .query(id.as_str())
            .take(5)
            .favorites(false)
            .send()
            .await?;

        let url = results
            .products
            .products
            .iter()
            .find(|p| p.id == *id)
            .map(|p| p.url.clone())
            .ok_or_else(|| Error::NotFound {
                kind: "product",
                id: id.to_string(),
            })?;

        self.get_by_url(&url).await
    }

    /// Read a product by the slug its page lives at, with or without a leading slash.
    pub async fn get_by_url(&self, url: &str) -> Result<ProductDetails> {
        let context = self.client.context().await?;
        let slug = url.trim_start_matches('/');

        let req = self
            .client
            .get(Backend::Site, &[slug])?
            .query("GetAsJson", "1")
            .maybe_query("t", context.timeslot_utc.as_ref().map(|t| t.as_str()))
            .query("d", "1");

        let page: ProductPageJson = self.client.send_json(req, "the product page").await?;
        page.content
            .into_iter()
            .find(|block| block.template_name.as_deref() == Some(PRODUCT_TEMPLATE))
            .ok_or_else(|| Error::NotFound {
                kind: "product",
                id: slug.to_owned(),
            })
            .and_then(|block| {
                serde_json::from_value(block.rest).map_err(|source| Error::Decode {
                    expected: "the product details",
                    body: slug.to_owned(),
                    source,
                })
            })
    }
}

#[derive(Deserialize)]
struct ProductPageJson {
    #[serde(default, rename = "content")]
    content: Vec<Block>,
}

/// One block of a rendered page, identified by which template drew it.
#[derive(Deserialize)]
struct Block {
    #[serde(rename = "TemplateName")]
    template_name: Option<String>,
    #[serde(flatten)]
    rest: serde_json::Value,
}

/// Everything a product's own page says about it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[non_exhaustive]
pub struct ProductDetails {
    /// The product number.
    pub id: ProductId,
    /// Danish product name.
    pub name: String,
    /// Manufacturer, when there is one.
    #[serde(default)]
    pub brand: Option<String>,
    /// Top-level Danish category.
    #[serde(default)]
    pub category: Option<String>,
    /// Danish subcategory.
    #[serde(default)]
    pub sub_category: Option<String>,
    /// Slug this page lives at.
    #[serde(default)]
    pub url: String,
    /// What one costs, in kroner.
    #[serde(default)]
    pub price: f64,
    /// Price per unit.
    #[serde(default)]
    pub unit_price_calc: Option<f64>,
    /// Which unit the unit price is per.
    #[serde(default)]
    pub unit_price_label: Option<String>,
    /// Size and variant.
    #[serde(default)]
    pub description: Option<String>,
    /// Marketing copy, as HTML.
    #[serde(default)]
    pub text: Option<String>,
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
    /// Danish badges.
    #[serde(default)]
    pub labels: Vec<String>,
    /// The offer running on it, if any.
    #[serde(default)]
    pub campaign: Option<Campaign>,
    /// Danish key/value facts, such as `Allergener` or `Oprindelse`.
    #[serde(default)]
    pub attributes: Vec<Attribute>,
    /// Every image and video on the page.
    #[serde(default)]
    pub media: Vec<Media>,
    /// Ingredients and nutrition, where the product has them.
    #[serde(default)]
    pub declarations: Option<Declarations>,
    /// Products the website offers alongside this one.
    #[serde(default)]
    pub related_products: Vec<super::search::Product>,
    /// Products the website offers in its place.
    #[serde(default)]
    pub alternative_products: Vec<super::search::Product>,
}

/// One Danish key/value fact about a product.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[non_exhaustive]
pub struct Attribute {
    /// Danish key, such as `Smag`, `Allergener` or `Opbevaring`.
    #[serde(default)]
    pub name: String,
    /// Danish value.
    #[serde(default)]
    pub value: String,
}

/// An image or video on a product page.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[non_exhaustive]
pub struct Media {
    /// Where to fetch it.
    #[serde(default)]
    pub url: String,
    /// Whether this is the one shown first.
    #[serde(default)]
    pub is_primary: bool,
    /// What kind of media it is, such as `image`.
    #[serde(default)]
    pub media_type: Option<String>,
    /// Danish alt text.
    #[serde(default)]
    pub alt_text: Option<String>,
}

/// The legally required declaration: what is in it and what that amounts to.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[non_exhaustive]
pub struct Declarations {
    /// Danish ingredient list.
    #[serde(default)]
    pub ingredients: Option<String>,
    /// Danish nutrition table, per 100 g or 100 ml.
    #[serde(default)]
    pub nutrition_facts: Vec<Attribute>,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    const PAGE: &str = r#"{
        "MetaData": {"Name": "Cocio"},
        "content": [
            {"TemplateName": "breadcrumbspot", "Items": []},
            {"TemplateName": "productdetailspot", "Id": "701025", "Name": "Cocio kakaomælk",
             "Url": "cocio-kakaomaelk-701025", "Price": 23.75,
             "Attributes": [{"Name": "Allergener", "Value": "Mælk"}],
             "Declarations": {"Ingredients": "Mælk, sukker",
                              "NutritionFacts": [{"Name": "Energi", "Value": "276 kJ"}]},
             "Campaign": {"MinQuantity": 3, "TotalPrice": 68.0, "Type": "ProductCampaignMixOffer", "Code": "FM"}}
        ]}"#;

    fn details() -> ProductDetails {
        let page: ProductPageJson = serde_json::from_str(PAGE).expect("the page parses");
        let block = page
            .content
            .into_iter()
            .find(|b| b.template_name.as_deref() == Some(PRODUCT_TEMPLATE))
            .expect("the product block is there");
        serde_json::from_value(block.rest).expect("the block parses")
    }

    #[test]
    fn the_product_block_is_picked_out_of_the_page() {
        let product = details();
        assert_eq!(product.id.as_str(), "701025");
        assert_eq!(product.name, "Cocio kakaomælk");
    }

    #[test]
    fn the_danish_declaration_survives_intact() {
        let product = details();
        assert_eq!(
            product.attributes.first().map(|a| a.name.as_str()),
            Some("Allergener")
        );
        let declarations = product.declarations.expect("there is a declaration");
        assert_eq!(declarations.ingredients.as_deref(), Some("Mælk, sukker"));
        assert_eq!(
            declarations
                .nutrition_facts
                .first()
                .map(|n| n.value.as_str()),
            Some("276 kJ")
        );
    }

    #[test]
    fn a_campaign_keeps_the_sites_own_vocabulary() {
        let campaign = details().campaign.expect("there is a campaign");
        assert_eq!(campaign.kind.as_deref(), Some("ProductCampaignMixOffer"));
        assert_eq!(campaign.code.as_deref(), Some("FM"));
        assert_eq!(campaign.min_quantity, Some(3));
    }

    #[test]
    fn a_page_without_a_product_block_is_not_a_product() {
        let page: ProductPageJson =
            serde_json::from_str(r#"{"content":[{"TemplateName":"articlespot"}]}"#)
                .expect("parses");
        assert!(
            page.content
                .iter()
                .all(|b| b.template_name.as_deref() != Some(PRODUCT_TEMPLATE))
        );
    }
}
