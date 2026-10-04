//! Reading and changing the basket.
//!
//! nemlig.com has one basket per account and one verb for changing it. `AddToBasket` sets
//! an absolute quantity rather than adding to the one already there, and a quantity of
//! zero is how a line is removed; there is no remove endpoint. The API here is shaped the
//! same way, so the surprise lives in one method's documentation rather than in every
//! caller.

use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::error::Result;
use crate::types::{Address, Campaign, DanishNumber, ProductId};

impl Client {
    /// Read or change the account's basket.
    pub fn basket(&self) -> BasketApi<'_> {
        BasketApi::new(self)
    }
}

/// The account's basket.
#[derive(Debug, Clone, Copy)]
pub struct BasketApi<'a> {
    client: &'a Client,
}

impl<'a> BasketApi<'a> {
    pub(crate) fn new(client: &'a Client) -> Self {
        Self { client }
    }

    /// Read the basket.
    pub async fn get(&self) -> Result<Basket> {
        let req = self.client.get(
            crate::client::Backend::Site,
            &["webapi", "basket", "GetBasket"],
        )?;
        self.client.send_json(req, "the basket").await
    }

    /// Set how many of a product the basket holds, and answer with the basket after.
    ///
    /// The quantity is absolute. Sending 3 for a line already holding 1 leaves 3, not 4,
    /// and sending 0 removes the line. Removing something the basket does not hold
    /// succeeds and changes nothing.
    pub async fn set_quantity(&self, product: &ProductId, quantity: u32) -> Result<Basket> {
        let req = self
            .client
            .post(&["webapi", "basket", "AddToBasket"])?
            .referrer("")
            .json(&SetQuantity {
                product_id: product.as_str(),
                quantity,
                affect_partial_quantity: false,
                disable_quantity_validation: false,
            })?;
        let basket: Basket = self.client.send_json(req, "the basket").await?;
        tracing::info!(product = %product, quantity, lines = basket.lines.len(), "basket changed");
        Ok(basket)
    }

    /// Remove every line, and answer with the empty basket.
    ///
    /// One request per line, because that is the only verb there is. An already empty
    /// basket costs the one read.
    pub async fn empty(&self) -> Result<Basket> {
        let mut basket = self.get().await?;
        let ids: Vec<ProductId> = basket.lines.iter().map(|line| line.id.clone()).collect();
        for id in &ids {
            basket = self.set_quantity(id, 0).await?;
        }
        Ok(basket)
    }
}

/// The body `AddToBasket` wants.
///
/// The casing is nemlig.com's, not a mistake: `ProductId` is capitalised and `quantity`
/// is not, so the fields are renamed one at a time rather than through a convention.
#[derive(Debug, Serialize)]
struct SetQuantity<'a> {
    #[serde(rename = "ProductId")]
    product_id: &'a str,
    #[serde(rename = "quantity")]
    quantity: u32,
    #[serde(rename = "AffectPartialQuantity")]
    affect_partial_quantity: bool,
    #[serde(rename = "disableQuantityValidation")]
    disable_quantity_validation: bool,
}

/// The account's basket.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[non_exhaustive]
pub struct Basket {
    /// Identifier of this basket, which becomes the order's on checkout.
    #[serde(default)]
    pub basket_guid: Option<String>,
    /// Numeric identifier of this basket.
    #[serde(default)]
    pub id: Option<i64>,
    /// Order number, once the basket has become an order.
    #[serde(default)]
    pub order_number: Option<String>,
    /// What it holds.
    #[serde(default)]
    pub lines: Vec<BasketLine>,
    /// Recipes added as a unit, kept as sent: this crate has no verb for them yet.
    #[serde(default)]
    pub recipes: Vec<serde_json::Value>,
    /// How many items in total, which can be fractional for goods sold by weight.
    #[serde(default)]
    pub number_of_products: Option<f64>,
    /// What the goods cost, before delivery.
    #[serde(default)]
    pub total_products_price: Option<f64>,
    /// Where it would be delivered.
    #[serde(default)]
    pub delivery_address: Option<Address>,
    /// Where the invoice would go.
    #[serde(default)]
    pub invoice_address: Option<Address>,
    /// Whether the two addresses are the same.
    #[serde(default)]
    pub addresses_are_equal: Option<bool>,
    /// Reasons checkout would refuse, kept as sent.
    #[serde(default)]
    pub validation_failures: Vec<serde_json::Value>,
}

impl Basket {
    /// What the goods cost.
    ///
    /// nemlig.com sends this as `TotalProductsPrice` on some responses and leaves it out
    /// of others, so an absent total is added up from the lines rather than reported as
    /// zero. Delivery is not included either way.
    pub fn total(&self) -> f64 {
        self.total_products_price
            .unwrap_or_else(|| self.lines.iter().map(|line| line.price).sum())
    }

    /// Whether the basket holds nothing.
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// The line holding a given product, if there is one.
    pub fn line(&self, product: &ProductId) -> Option<&BasketLine> {
        self.lines.iter().find(|line| line.id == *product)
    }
}

/// One product in the basket, with however many of it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[non_exhaustive]
pub struct BasketLine {
    /// The product number.
    pub id: ProductId,
    /// Danish product name.
    #[serde(default)]
    pub name: String,
    /// Manufacturer, when there is one.
    #[serde(default)]
    pub brand: Option<String>,
    /// Slug the product's page lives at.
    #[serde(default)]
    pub url: String,
    /// How many of it.
    #[serde(default)]
    pub quantity: u32,
    /// What one costs.
    #[serde(default)]
    pub item_price: f64,
    /// What this line costs in total.
    #[serde(default)]
    pub price: f64,
    /// Price per unit, as the Danish-formatted string the website prints.
    #[serde(default)]
    pub unit_price: Option<DanishNumber>,
    /// The same price as a float, which can differ from the string in the last digit.
    #[serde(default)]
    pub unit_price_calc: Option<f64>,
    /// Which unit the unit price is per.
    #[serde(default)]
    pub unit_price_label: Option<String>,
    /// Size and variant.
    #[serde(default)]
    pub description: Option<String>,
    /// URL of the product photograph.
    #[serde(default)]
    pub primary_image: Option<String>,
    /// Whether the price shown is a reduced one.
    #[serde(default)]
    pub discount_item: bool,
    /// What the offer on this line saved.
    #[serde(default)]
    pub discount_savings: f64,
    /// Whether the account has marked this product a favourite.
    #[serde(default)]
    pub favorite: bool,
    /// Danish badges.
    #[serde(default)]
    pub labels: Vec<String>,
    /// Where the product sits in the catalogue, as `number#name` pairs joined by `;`.
    #[serde(default)]
    pub category_path: Option<String>,
    /// Danish name of the group the product belongs to.
    #[serde(default)]
    pub product_sub_group_name: Option<String>,
    /// Danish name of the department the product belongs to.
    #[serde(default)]
    pub product_main_group_name: Option<String>,
    /// Danish name of the offer, such as `Fast mixtilbud`.
    #[serde(default)]
    pub campaign_attribute: Option<String>,
    /// The offer running on it, if any.
    #[serde(default)]
    pub campaign: Option<Campaign>,
    /// Products the website would put in its place if it runs out.
    #[serde(default)]
    pub alternative_products: Vec<serde_json::Value>,
    /// How many are left, where the website says.
    #[serde(default)]
    pub remaining_stock: Option<f64>,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    const LINE: &str = r#"{
        "Id": "5064654", "Name": "Black Coffee Blend Supreme",
        "Brand": "Black Coffee Roasters", "Url": "black-coffee-blend-supreme-5064654",
        "Quantity": 1, "ItemPrice": 99.95, "Price": 99.95,
        "UnitPrice": "249,87", "UnitPriceCalc": 249.88, "UnitPriceLabel": "kr./Kg.",
        "CategoryPath": "0700000000#Drikke;0701000008#Kaffe, hele bønner",
        "CampaignAttribute": "Fast mixtilbud",
        "Campaign": {"MinQuantity": 2, "TotalPrice": 180.0, "Type": "ProductCampaignBuyXForY", "Code": "KF"}
    }"#;

    #[test]
    fn the_add_body_keeps_nemligs_own_casing() {
        let json = serde_json::to_value(SetQuantity {
            product_id: "5070417",
            quantity: 3,
            affect_partial_quantity: false,
            disable_quantity_validation: false,
        })
        .expect("serializes");
        assert_eq!(
            json.get("ProductId").and_then(|v| v.as_str()),
            Some("5070417")
        );
        assert_eq!(json.get("quantity").and_then(|v| v.as_u64()), Some(3));
        assert!(json.get("AffectPartialQuantity").is_some());
        assert!(json.get("disableQuantityValidation").is_some());
    }

    #[test]
    fn a_line_reads_both_spellings_of_the_unit_price() {
        let line: BasketLine = serde_json::from_str(LINE).expect("parses");
        let danish = line.unit_price.expect("the string form is there").get();
        let float = line.unit_price_calc.expect("the float form is there");
        assert!((danish - 249.87).abs() < f64::EPSILON);
        assert!((float - 249.88).abs() < f64::EPSILON);
    }

    #[test]
    fn a_total_is_added_up_when_the_server_leaves_it_out() {
        let with_lines: Basket =
            serde_json::from_str(&format!(r#"{{"Lines":[{LINE},{LINE}]}}"#)).expect("parses");
        assert!((with_lines.total() - 199.90).abs() < 0.001);

        let with_total: Basket =
            serde_json::from_str(r#"{"Lines":[],"TotalProductsPrice":42.0}"#).expect("parses");
        assert!((with_total.total() - 42.0).abs() < f64::EPSILON);
        assert!(with_total.is_empty());
    }

    #[test]
    fn a_line_is_found_by_product_number() {
        let basket: Basket =
            serde_json::from_str(&format!(r#"{{"Lines":[{LINE}]}}"#)).expect("parses");
        let id: ProductId = "5064654".parse().expect("valid");
        assert!(basket.line(&id).is_some());
        let absent: ProductId = "1".parse().expect("valid");
        assert!(basket.line(&absent).is_none());
    }

    #[test]
    fn an_empty_basket_still_parses() {
        let basket: Basket = serde_json::from_str(
            r#"{"BasketGuid":"abc","Id":1,"NumberOfProducts":0,"TotalProductsPrice":0.0,
                "Lines":[],"Recipes":[],"ValidationFailures":[]}"#,
        )
        .expect("parses");
        assert!(basket.is_empty());
        assert_eq!(basket.basket_guid.as_deref(), Some("abc"));
    }
}
