//! Checkout: the payment cards on the account, and what they would cost to use.
//!
//! Everything here reads, with one exception. [`CheckoutApi::place_order`] exists only
//! when the `place-order` feature is on, which it is not by default, and even then it
//! refuses unless the client was built with
//! [`allow_place_order`](crate::ClientBuilder::allow_place_order). Placing an order is
//! irreversible and costs money; two switches is deliberate.

use serde::{Deserialize, Serialize};

use crate::client::{Backend, Client};
use crate::error::Result;

impl Client {
    /// Read the account's payment arrangements.
    pub fn checkout(&self) -> CheckoutApi<'_> {
        CheckoutApi::new(self)
    }
}

/// Reads about payment, and the one write that spends money.
#[derive(Debug, Clone, Copy)]
pub struct CheckoutApi<'a> {
    client: &'a Client,
}

impl<'a> CheckoutApi<'a> {
    pub(crate) fn new(client: &'a Client) -> Self {
        Self { client }
    }

    /// The payment cards stored on the account.
    pub async fn credit_cards(&self) -> Result<Vec<CreditCard>> {
        let req = self
            .client
            .get(Backend::Site, &["webapi", "Checkout", "GetCreditCards"])?;
        self.client.send_json(req, "the payment cards").await
    }

    /// What each of the given cards would add in fees to an order of `total` kroner.
    pub async fn card_fees(&self, total: f64, cards: &[i64]) -> Result<Vec<CardFee>> {
        let mut req = self
            .client
            .get(Backend::Site, &["webapi", "Checkout", "GetCardsFees"])?
            .query("total", total);
        for card in cards {
            // Repeated rather than joined: the endpoint reads `ids` as a list.
            req = req.query("ids", card);
        }
        self.client.send_json(req, "the card fees").await
    }

    /// Which of nemlig.com's own mailings the account is signed up to.
    pub async fn integrations(&self) -> Result<Integrations> {
        let req = self.client.get(
            Backend::Site,
            &["webapi", "ThirdPartyIntegrations", "GetIntegrations"],
        )?;
        self.client.send_json(req, "the integration settings").await
    }
}

/// A payment card stored on the account.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[non_exhaustive]
pub struct CreditCard {
    /// Identifier to pay with, which is what [`CheckoutApi::card_fees`] takes.
    #[serde(default)]
    pub card_id: i64,
    /// Masked number, such as `52340000****1234`.
    #[serde(default)]
    pub card_mask: Option<String>,
    /// Scheme, such as `MasterCard`.
    #[serde(default)]
    pub card_type: Option<String>,
    /// Expiry month, two digits.
    #[serde(default)]
    pub card_expiration_month: Option<String>,
    /// Expiry year, two digits.
    #[serde(default)]
    pub card_expiration_year: Option<String>,
    /// What using it adds, as a percentage of the order.
    #[serde(default)]
    pub fee_in_percent: f64,
    /// Whether the website would reach for this one first.
    #[serde(default)]
    pub is_default: bool,
}

/// What one card would add to one order.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[non_exhaustive]
pub struct CardFee {
    /// Which card.
    #[serde(default)]
    pub card_id: i64,
    /// What it would add, in kroner.
    #[serde(default)]
    pub amount: f64,
    /// Whether the fee could not be worked out.
    #[serde(default)]
    pub is_failed: bool,
    /// Why, when it could not.
    #[serde(default)]
    pub error_message: Option<String>,
}

/// Which of nemlig.com's own mailings the account receives.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[non_exhaustive]
pub struct Integrations {
    /// Newsletters and surveys.
    #[serde(default)]
    pub newsletters_and_surveys_allowed: bool,
    /// Recipe suggestions.
    #[serde(default)]
    pub recipes_allowed: bool,
    /// Meal box mailings.
    #[serde(default)]
    pub mealbox_emails: bool,
    /// Delivery notifications by text message.
    #[serde(default)]
    pub sms_notifications_allowed: bool,
}

#[cfg(feature = "place-order")]
pub use place::{PlaceOrder, PlacedOrder};

#[cfg(feature = "place-order")]
mod place;

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn a_card_is_read_without_its_number() {
        let cards: Vec<CreditCard> = serde_json::from_str(
            r#"[{"CardId":100001,"CardMask":"52340000****1234","CardType":"MasterCard",
                 "CardExpirationMonth":"06","CardExpirationYear":"27",
                 "FeeInPercent":0.0,"IsDefault":true}]"#,
        )
        .expect("parses");
        let card = cards.first().expect("one card");
        assert_eq!(card.card_id, 100001);
        assert!(card.is_default);
        assert!(card.card_mask.as_deref().is_some_and(|m| m.contains('*')));
    }

    #[test]
    fn a_failed_fee_says_why() {
        let fees: Vec<CardFee> = serde_json::from_str(
            r#"[{"CardId":100002,"Amount":0.0,"IsFailed":true,"ErrorMessage":"Kortet er udløbet"}]"#,
        )
        .expect("parses");
        let fee = fees.first().expect("one fee");
        assert!(fee.is_failed);
        assert_eq!(fee.error_message.as_deref(), Some("Kortet er udløbet"));
    }
}
