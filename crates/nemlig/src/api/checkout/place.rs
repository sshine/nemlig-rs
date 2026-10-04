//! Placing an order, which spends real money.
//!
//! Compiled only with the `place-order` feature. Everything here is irreversible once the
//! deadline on the delivery passes, so the module asks for the same thing twice in two
//! different forms: the client has to have been built with
//! [`allow_place_order`](crate::ClientBuilder::allow_place_order), and the caller has to
//! state the total it expects, which is checked against the basket before anything is sent.
//!
//! This crate has no live test for it, and the MCP server does not compile it in at all.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::types::InvalidValue;

use super::CheckoutApi;

/// How far the basket may differ from the stated total and still be accepted.
///
/// Half an øre: enough to absorb a float round trip, not enough to absorb a price change.
const TOTAL_TOLERANCE: f64 = 0.005;

/// An order about to be placed.
#[derive(Debug, Clone)]
pub struct PlaceOrder {
    payment_card: i64,
    confirm_total: f64,
    terms_accepted: bool,
    placement: String,
    notes: String,
    unattended_notes: String,
    door_code: String,
    use_mobile_pay: bool,
    return_of_bottles: bool,
}

impl PlaceOrder {
    /// Name the card to charge and the total expected, in kroner.
    ///
    /// The total is checked against the basket immediately before the order is sent, so a
    /// price that moved between reading the basket and paying for it stops the order
    /// rather than quietly charging the new one.
    pub fn new(payment_card: i64, confirm_total: f64) -> Self {
        Self {
            payment_card,
            confirm_total,
            terms_accepted: false,
            placement: String::new(),
            notes: String::new(),
            unattended_notes: String::new(),
            door_code: String::new(),
            use_mobile_pay: false,
            return_of_bottles: false,
        }
    }

    /// Accept nemlig.com's terms, which the order cannot be placed without.
    pub fn terms_accepted(mut self, accepted: bool) -> Self {
        self.terms_accepted = accepted;
        self
    }

    /// Where the driver may leave the crates, as a key from
    /// [`DeliveryApi::placements`](super::delivery::DeliveryApi::placements).
    pub fn placement(mut self, placement: impl Into<String>) -> Self {
        self.placement = placement.into();
        self
    }

    /// A note for the picker.
    pub fn notes(mut self, notes: impl Into<String>) -> Self {
        self.notes = notes.into();
        self
    }

    /// A note for the driver, for when nobody answers.
    pub fn unattended_notes(mut self, notes: impl Into<String>) -> Self {
        self.unattended_notes = notes.into();
        self
    }

    /// The door code the driver needs to get in.
    pub fn door_code(mut self, code: impl Into<String>) -> Self {
        self.door_code = code.into();
        self
    }

    /// Pay with MobilePay rather than the card.
    pub fn use_mobile_pay(mut self, use_mobile_pay: bool) -> Self {
        self.use_mobile_pay = use_mobile_pay;
        self
    }

    /// Ask the driver to take empty bottles back.
    pub fn return_of_bottles(mut self, requested: bool) -> Self {
        self.return_of_bottles = requested;
        self
    }
}

/// What nemlig.com answers with once an order is placed.
///
/// The website follows a redirect at this point rather than reading a document, so the
/// response is kept as sent and the fields worth naming are pulled out of it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PlacedOrder {
    /// The order number, where the response carries one.
    pub order_number: Option<String>,
    /// The whole response.
    pub response: serde_json::Value,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct PlaceOrderBody<'a> {
    notes: &'a str,
    unattended_notes: &'a str,
    placement_message: &'a str,
    door_code: &'a str,
    password: &'a str,
    return_of_bottles_requested: bool,
    terms_and_conditions_accepted: bool,
    check_for_orders_to_merge: bool,
    payment_card: i64,
    use_mobile_pay: bool,
}

impl<'a> CheckoutApi<'a> {
    /// Place the basket as an order, charging the card.
    ///
    /// Refuses, without sending anything, unless the client was built with
    /// [`allow_place_order`](crate::ClientBuilder::allow_place_order), the terms were
    /// accepted, and the basket's total matches the one stated.
    ///
    /// The account password goes in the body, which is what nemlig.com requires; it is
    /// taken from the client's own credentials rather than asked for again.
    pub async fn place_order(&self, order: PlaceOrder) -> Result<PlacedOrder> {
        let client = self.client;
        if !client.place_order_allowed() {
            return Err(Error::PlaceOrderDisabled);
        }
        if !order.terms_accepted {
            return Err(InvalidValue {
                field: "terms_accepted",
                reason: "nemlig.com will not place an order without it",
                value: "false".to_owned(),
            }
            .into());
        }

        let basket = client.basket().get().await?;
        let total = basket.total();
        if (total - order.confirm_total).abs() > TOTAL_TOLERANCE {
            return Err(InvalidValue {
                field: "confirm_total",
                reason: "does not match what the basket now costs",
                value: format!(
                    "{:.2} stated, {total:.2} in the basket",
                    order.confirm_total
                ),
            }
            .into());
        }

        let body = PlaceOrderBody {
            notes: &order.notes,
            unattended_notes: &order.unattended_notes,
            placement_message: &order.placement,
            door_code: &order.door_code,
            password: client.credentials().password.expose(),
            return_of_bottles_requested: order.return_of_bottles,
            terms_and_conditions_accepted: true,
            check_for_orders_to_merge: true,
            payment_card: order.payment_card,
            use_mobile_pay: order.use_mobile_pay,
        };

        let req = client
            .post(&["webapi", "Order", "PlaceOrderLoggedIn"])?
            .referrer("basket")
            .json(&body)?;

        tracing::warn!(total, card = order.payment_card, "placing a real order");
        let response: serde_json::Value = client.send_json(req, "the placed order").await?;

        let order_number = response
            .get("OrderNumber")
            .or_else(|| response.get("orderNumber"))
            .and_then(|v| v.as_str())
            .map(str::to_owned);

        Ok(PlacedOrder {
            order_number,
            response,
        })
    }

    /// Register the payment for an order just placed.
    ///
    /// A GET that has an effect: the website calls it straight after checkout, and
    /// nothing about the method says so.
    pub async fn register_payment(&self, use_mobile_pay: bool) -> Result<serde_json::Value> {
        let client = self.client;
        if !client.place_order_allowed() {
            return Err(Error::PlaceOrderDisabled);
        }
        let req = client
            .get(
                crate::client::Backend::Site,
                &["webapi", "Checkout", "RegisterNewPaymentTransaction"],
            )?
            .query("useMobilePay", use_mobile_pay);
        client.send_json(req, "the payment registration").await
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::{Client, Credentials};

    fn client(allow: bool) -> Client {
        Client::builder()
            .credentials(Credentials::new("a@b.c", "hunter2-not-a-real-password"))
            .allow_place_order(allow)
            .build()
            .expect("builds")
    }

    #[tokio::test]
    async fn a_client_that_was_not_asked_refuses_before_sending_anything() {
        let order = PlaceOrder::new(100001, 10.0).terms_accepted(true);
        let err = client(false)
            .checkout()
            .place_order(order)
            .await
            .expect_err("refuses");
        assert!(matches!(err, Error::PlaceOrderDisabled));
        assert_eq!(client(false).requests_made(), 0);
    }

    #[tokio::test]
    async fn unaccepted_terms_stop_the_order_before_the_basket_is_read() {
        let client = client(true);
        let err = client
            .checkout()
            .place_order(PlaceOrder::new(100001, 10.0))
            .await
            .expect_err("refuses");
        assert!(matches!(err, Error::Invalid(_)));
        assert_eq!(client.requests_made(), 0);
    }

    #[test]
    fn the_body_keeps_the_casing_the_site_expects() {
        let json = serde_json::to_value(PlaceOrderBody {
            notes: "",
            unattended_notes: "",
            placement_message: "Frontdoor",
            door_code: "",
            password: "hunter2-not-a-real-password",
            return_of_bottles_requested: false,
            terms_and_conditions_accepted: true,
            check_for_orders_to_merge: true,
            payment_card: 100001,
            use_mobile_pay: false,
        })
        .expect("serializes");
        for key in [
            "Notes",
            "UnattendedNotes",
            "PlacementMessage",
            "DoorCode",
            "Password",
            "TermsAndConditionsAccepted",
            "PaymentCard",
            "UseMobilePay",
        ] {
            assert!(json.get(key).is_some(), "{key} is missing");
        }
    }
}
