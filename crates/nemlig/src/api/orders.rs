//! Past orders.
//!
//! Orders are read in two halves. The history lists them with their totals, delivery
//! windows and status; a second endpoint gives one order's lines. The two use different
//! field names for the same things, so they are modelled as the different documents they
//! are rather than forced together.
//!
//! There is no endpoint that fetches one order by its number, so [`OrdersApi::get`] walks
//! the history looking for it.

use serde::{Deserialize, Serialize};

use crate::client::{Backend, Client};
use crate::error::{Error, Result};
use crate::types::Address;

/// How many orders a page of history holds when no count is asked for.
pub const DEFAULT_PAGE: u32 = 20;

/// How far back [`OrdersApi::get`] walks before giving up on finding an order.
pub const MAX_SCAN: u32 = 100;

impl Client {
    /// Read the account's past orders.
    pub fn orders(&self) -> OrdersApi<'_> {
        OrdersApi::new(self)
    }
}

/// Reads about past orders.
#[derive(Debug, Clone, Copy)]
pub struct OrdersApi<'a> {
    client: &'a Client,
}

impl<'a> OrdersApi<'a> {
    pub(crate) fn new(client: &'a Client) -> Self {
        Self { client }
    }

    /// One page of the order history, newest first.
    pub async fn history(&self, skip: u32, take: u32) -> Result<OrderHistory> {
        let req = self
            .client
            .get(Backend::Site, &["webapi", "order", "GetBasicOrderHistory"])?
            .query("skip", skip)
            .query("take", take);
        self.client.send_json(req, "the order history").await
    }

    /// What one order contained.
    pub async fn lines(&self, order: i64) -> Result<Vec<OrderLine>> {
        #[derive(Deserialize)]
        struct Response {
            #[serde(default, rename = "Lines")]
            lines: Vec<OrderLine>,
        }

        let id = order.to_string();
        let req = self.client.get(
            Backend::Site,
            &["webapi", "v2", "order", "GetOrderHistory", &id],
        )?;
        let response: Response = self.client.send_json(req, "the order lines").await?;
        Ok(response.lines)
    }

    /// One whole order: what the history says about it, and what it contained.
    ///
    /// Costs a page of history per [`DEFAULT_PAGE`] orders scanned, plus one request for
    /// the lines, because the API offers no way to address an order directly. Gives up
    /// after [`MAX_SCAN`] orders.
    pub async fn get(&self, order: i64) -> Result<Order> {
        let mut scanned = 0;
        while scanned < MAX_SCAN {
            let page = self.history(scanned, DEFAULT_PAGE).await?;
            let found = page.orders.len() as u32;
            if let Some(summary) = page.orders.into_iter().find(|o| o.id == Some(order)) {
                let lines = self.lines(order).await?;
                return Ok(Order { summary, lines });
            }
            if found < DEFAULT_PAGE {
                break;
            }
            scanned += found;
        }
        Err(Error::NotFound {
            kind: "order",
            id: order.to_string(),
        })
    }

    /// The confirmation page for an order, addressed the way the website addresses it.
    ///
    /// All three identifiers come from the redirect the website follows after checkout,
    /// so this is of use to whoever placed the order and to nobody else.
    pub async fn summary(
        &self,
        order_number: &str,
        unique_id: &str,
        basket_id: i64,
    ) -> Result<serde_json::Value> {
        let req = self
            .client
            .get(Backend::Site, &["webapi", "Order", "GetOrderSummary"])?
            .query("orderNumber", order_number)
            .query("uniqueId", unique_id)
            .query("basketId", basket_id);
        self.client.send_json(req, "the order summary").await
    }
}

/// One page of the order history.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[non_exhaustive]
pub struct OrderHistory {
    /// The orders on this page, newest first.
    #[serde(default)]
    pub orders: Vec<OrderSummary>,
    /// How many pages there are at the page size that was asked for.
    #[serde(default)]
    pub number_of_pages: u32,
}

/// An order and its contents.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Order {
    /// What the history says about it.
    pub summary: OrderSummary,
    /// What it contained.
    pub lines: Vec<OrderLine>,
}

/// An order as the history lists it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[non_exhaustive]
pub struct OrderSummary {
    /// Numeric identifier, which is what [`OrdersApi::lines`] takes.
    #[serde(default)]
    pub id: Option<i64>,
    /// Order number as the website and the invoice print it.
    #[serde(default)]
    pub order_number: Option<String>,
    /// Where the order has got to.
    #[serde(default)]
    pub status: OrderStatus,
    /// What was charged, delivery included.
    #[serde(default)]
    pub total: f64,
    /// What the goods came to, before delivery.
    #[serde(default)]
    pub sub_total: f64,
    /// When the order was placed.
    #[serde(default)]
    pub order_date: Option<chrono::DateTime<chrono::Utc>>,
    /// The window it was or will be delivered in.
    #[serde(default)]
    pub delivery_time: Option<TimeWindow>,
    /// Where it went.
    #[serde(default)]
    pub delivery_address: Option<Address>,
    /// Whether an invoice can be downloaded.
    #[serde(default)]
    pub has_invoice: bool,
    /// Whether it is still before the deadline for changing it.
    #[serde(default)]
    pub is_editable: bool,
    /// Whether the website offers to order the same again.
    #[serde(default)]
    pub can_repeat: bool,
    /// Whether the order was a meal box rather than loose goods.
    #[serde(default)]
    pub is_meal_box: bool,
}

impl OrderSummary {
    /// What delivery cost: the difference between the two totals.
    ///
    /// nemlig.com sends no delivery line of its own, so this is derived rather than read.
    pub fn delivery_fee(&self) -> f64 {
        self.total - self.sub_total
    }
}

/// A start and an end.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[non_exhaustive]
pub struct TimeWindow {
    /// When it opens.
    pub start: chrono::DateTime<chrono::Utc>,
    /// When it closes.
    pub end: chrono::DateTime<chrono::Utc>,
}

/// Where an order has got to.
///
/// nemlig.com sends a number and documents none of them; the three named here are the
/// ones that have been observed, and anything else keeps its number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(from = "i64", into = "i64")]
#[non_exhaustive]
pub enum OrderStatus {
    /// Placed, not yet picked.
    Pending,
    /// Being picked or on its way.
    Processing,
    /// Delivered.
    Delivered,
    /// A status this crate has not seen, kept as the number it arrived as.
    #[default]
    Unknown,
    /// A status this crate has not seen, kept as the number it arrived as.
    Other(i64),
}

impl From<i64> for OrderStatus {
    fn from(raw: i64) -> Self {
        match raw {
            1 => Self::Pending,
            2 => Self::Processing,
            4 => Self::Delivered,
            other => Self::Other(other),
        }
    }
}

impl From<OrderStatus> for i64 {
    fn from(status: OrderStatus) -> Self {
        match status {
            OrderStatus::Pending => 1,
            OrderStatus::Processing => 2,
            OrderStatus::Delivered => 4,
            OrderStatus::Unknown => 0,
            OrderStatus::Other(other) => other,
        }
    }
}

impl std::fmt::Display for OrderStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => f.write_str("pending"),
            Self::Processing => f.write_str("processing"),
            Self::Delivered => f.write_str("delivered"),
            Self::Unknown => f.write_str("unknown"),
            Self::Other(other) => write!(f, "status {other}"),
        }
    }
}

/// One product in a past order.
///
/// Shares nothing but the meaning with [`BasketLine`](super::basket::BasketLine): the
/// product number is `ProductNumber` rather than `Id`, the total is `Amount` rather than
/// `Price`, and the quantity is fractional, because goods sold by weight are delivered in
/// whatever the picker found.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[non_exhaustive]
pub struct OrderLine {
    /// The product number.
    #[serde(default)]
    pub product_number: String,
    /// Danish product name.
    #[serde(default)]
    pub product_name: String,
    /// How many were delivered.
    #[serde(default)]
    pub quantity: f64,
    /// Danish unit, such as `Stk.` or `Kg.`.
    #[serde(default)]
    pub unit: Option<String>,
    /// Size and variant.
    #[serde(default)]
    pub description: Option<String>,
    /// What one came to on average.
    #[serde(default)]
    pub average_item_price: f64,
    /// What the line came to.
    #[serde(default)]
    pub amount: f64,
    /// What it would have come to without the offer.
    #[serde(default)]
    pub original_amount: f64,
    /// Danish name of the group the product belongs to.
    #[serde(default)]
    pub group_name: Option<String>,
    /// Danish name of the department the product belongs to.
    #[serde(default)]
    pub main_group_name: Option<String>,
    /// URL of the product photograph.
    #[serde(default)]
    pub image_url: Option<String>,
    /// Slug the product's page lives at.
    #[serde(default)]
    pub product_url: Option<String>,
    /// Danish name of the offer applied.
    #[serde(default)]
    pub campaign_name: Option<String>,
    /// Whether an offer applied at all.
    #[serde(default)]
    pub has_campaign: bool,
    /// Whether the picker delivered a different quantity than ordered.
    #[serde(default)]
    pub is_adjusted: bool,
    /// What was refunded for this line.
    #[serde(default)]
    pub refunded_amount: f64,
    /// Whether the line came from a meal box.
    #[serde(default)]
    pub is_meal_box: bool,
}

impl OrderLine {
    /// What the offer saved on this line.
    pub fn savings(&self) -> f64 {
        (self.original_amount - self.amount).max(0.0)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn history_reads_the_totals_and_the_window() {
        let history: OrderHistory = serde_json::from_str(
            r#"{"Orders":[{"Status":4,"OrderNumber":"2024-12345678","Total":604.48,
                 "SubTotal":574.48,"OrderDate":"2025-11-25T06:07:18Z","Id":12345678,
                 "DeliveryTime":{"Start":"2025-11-27T17:00:00Z","End":"2025-11-27T20:00:00Z"},
                 "HasInvoice":true,"CanRepeat":true}],"NumberOfPages":20}"#,
        )
        .expect("parses");

        let order = history.orders.first().expect("one order");
        assert_eq!(order.status, OrderStatus::Delivered);
        assert_eq!(order.id, Some(12345678));
        assert!((order.delivery_fee() - 30.0).abs() < 0.001);
        assert!(order.delivery_time.is_some());
        assert_eq!(history.number_of_pages, 20);
    }

    #[test]
    fn an_unknown_status_keeps_its_number() {
        let status: OrderStatus = serde_json::from_str("7").expect("parses");
        assert_eq!(status, OrderStatus::Other(7));
        assert_eq!(status.to_string(), "status 7");
        assert_eq!(serde_json::to_string(&status).expect("serializes"), "7");
    }

    #[test]
    fn an_order_line_uses_its_own_field_names() {
        let line: OrderLine = serde_json::from_str(
            r#"{"ProductNumber":"5064654","ProductName":"Black Coffee Blend Supreme",
                "Quantity":2.0,"Unit":"Stk.","AverageItemPrice":90.0,"Amount":180.0,
                "OriginalAmount":199.9,"CampaignName":"Fast mixtilbud","HasCampaign":true}"#,
        )
        .expect("parses");
        assert_eq!(line.product_number, "5064654");
        assert!((line.quantity - 2.0).abs() < f64::EPSILON);
        assert!((line.savings() - 19.9).abs() < 0.001);
    }

    #[test]
    fn a_line_that_cost_more_than_its_list_price_saved_nothing() {
        let line: OrderLine =
            serde_json::from_str(r#"{"Amount":10.0,"OriginalAmount":0.0}"#).expect("parses");
        assert_eq!(line.savings(), 0.0);
    }
}
