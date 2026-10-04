//! Delivery: the slot the account has, and where the driver may leave the crates.
//!
//! There is no endpoint that lists the slots on offer. The website picks one through a
//! flow this crate does not cover, and the choice then shows up as the opaque
//! [`TimeslotUtc`](crate::TimeslotUtc) in the page settings. What is readable is the slot
//! already booked, which is what [`DeliveryApi::spot`] answers with.

use serde::{Deserialize, Serialize};

use crate::client::{Backend, Client};
use crate::error::Result;

use super::orders::TimeWindow;

impl Client {
    /// Read the account's delivery arrangements.
    pub fn delivery(&self) -> DeliveryApi<'_> {
        DeliveryApi::new(self)
    }
}

/// Reads about delivery.
#[derive(Debug, Clone, Copy)]
pub struct DeliveryApi<'a> {
    client: &'a Client,
}

impl<'a> DeliveryApi<'a> {
    pub(crate) fn new(client: &'a Client) -> Self {
        Self { client }
    }

    /// The delivery the account currently has booked, if it has one.
    pub async fn spot(&self) -> Result<Option<DeliverySpot>> {
        let req = self
            .client
            .get(Backend::Site, &["webapi", "Order", "DeliverySpot"])?;
        self.client.send_json_opt(req, "the delivery slot").await
    }

    /// Where the driver may leave the crates when nobody answers.
    ///
    /// The keys are stable identifiers; the values are the Danish phrases the website
    /// shows, such as `Foran min hoveddør`.
    pub async fn placements(&self) -> Result<Vec<DeliveryPlacement>> {
        let req = self
            .client
            .get(
                Backend::Site,
                &["webapi", "Checkout", "GetDeliveryPlacements"],
            )?
            // Private customers. The website sends 0 for everyone who is not a business.
            .query("memberType", 0);
        self.client.send_json(req, "the delivery placements").await
    }
}

/// The delivery the account has booked.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[non_exhaustive]
pub struct DeliverySpot {
    /// Identifier of the order being delivered.
    #[serde(default)]
    pub id: Option<String>,
    /// Who it is for.
    #[serde(default)]
    pub customer_name: Option<String>,
    /// Order number as the website prints it.
    #[serde(default)]
    pub order_number: Option<String>,
    /// Where the order has got to, in nemlig.com's own words, such as `Reorder`.
    #[serde(default)]
    pub state: Option<String>,
    /// The window the driver will arrive in.
    #[serde(default)]
    pub time_slot: Option<TimeWindow>,
    /// The last moment the order can still be changed.
    #[serde(default)]
    pub edit_deadline: Option<chrono::DateTime<chrono::Utc>>,
    /// How far along the delivery is, from zero to one.
    #[serde(default)]
    pub progress: Option<f64>,
    /// When the driver actually arrived.
    #[serde(default)]
    pub delivery_time: Option<chrono::DateTime<chrono::Utc>>,
}

/// One of the places the driver may leave the crates.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[non_exhaustive]
pub struct DeliveryPlacement {
    /// Stable identifier, such as `Frontdoor`.
    #[serde(default)]
    pub key: String,
    /// The Danish phrase the website shows.
    #[serde(default)]
    pub value: String,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn a_booked_slot_carries_its_window_and_deadline() {
        let spot: DeliverySpot = serde_json::from_str(
            r#"{"Id":"12345678","CustomerName":"Anders And","OrderNumber":"1050001234",
                "State":"Reorder",
                "TimeSlot":{"Start":"2025-11-25T16:00:00Z","End":"2025-11-25T19:00:00Z"},
                "EditDeadline":"2025-11-24T23:00:00Z","Progress":1.0,"DeliveryTime":null}"#,
        )
        .expect("parses");
        assert_eq!(spot.state.as_deref(), Some("Reorder"));
        assert!(spot.time_slot.is_some());
        assert!(spot.edit_deadline.is_some());
        assert!(spot.delivery_time.is_none());
    }

    #[test]
    fn placements_keep_their_danish_wording() {
        let placements: Vec<DeliveryPlacement> = serde_json::from_str(
            r#"[{"Key":"Frontdoor","Value":"Foran min hoveddør"},
                {"Key":"Carport","Value":"I carporten"}]"#,
        )
        .expect("parses");
        assert_eq!(placements.len(), 2);
        assert_eq!(placements[0].value, "Foran min hoveddør");
    }
}
