//! The page settings, which search and product lookups cannot do without.
//!
//! nemlig.com's prices depend on when and where an order would be delivered, so the search
//! gateway refuses to answer without a timeslot, a delivery zone and a catalogue timestamp.
//! None of those have an endpoint of their own: they are embedded in the JSON the website
//! serves for its own front page, which is what this module fetches and caches.

use serde::{Deserialize, Serialize};

use crate::client::{Backend, Client};
use crate::error::Result;
use crate::types::TimeslotUtc;

/// What the website's own page JSON says about the current session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[non_exhaustive]
pub struct Context {
    /// Catalogue version, passed to search so it answers from the same one.
    #[serde(rename = "CombinedProductsAndSitecoreTimestamp", default)]
    pub timestamp: String,
    /// The delivery slot prices are quoted for.
    #[serde(default)]
    pub timeslot_utc: Option<TimeslotUtc>,
    /// Which delivery zone the session is browsing in.
    #[serde(default = "default_zone")]
    pub delivery_zone_id: i64,
    /// The signed-in account's identifier, which search wants to mark favourites.
    #[serde(default)]
    pub user_id: String,
    /// The postcode the session is browsing in.
    #[serde(default)]
    pub zip_code: Option<String>,
}

/// Zone 1 is what the website falls back to, and what an anonymous session gets.
fn default_zone() -> i64 {
    1
}

impl Context {
    /// The account id, when the session is signed in as someone.
    pub fn user_id(&self) -> Option<&str> {
        (!self.user_id.is_empty()).then_some(self.user_id.as_str())
    }
}

#[derive(Deserialize)]
struct Page {
    #[serde(rename = "Settings")]
    settings: Context,
}

impl Client {
    /// Fetch the page settings, bypassing the cache.
    pub(crate) async fn fetch_context(&self) -> Result<Context> {
        let req = self
            .get(Backend::Site, &[])?
            // What turns the single-page app's own route into the JSON behind it. `d=1`
            // rides along in the captured traffic; its meaning is the website's business.
            .query("GetAsJson", "1")
            .query("d", "1");
        let page: Page = self.send_json(req, "the page settings").await?;
        tracing::debug!(
            zone = page.settings.delivery_zone_id,
            timeslot = page.settings.timeslot_utc.as_ref().map(|t| t.as_str()),
            "fetched the page settings"
        );
        Ok(page.settings)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn settings_are_read_out_of_the_page_envelope() {
        let page: Page = serde_json::from_str(
            r#"{"MetaData":{"Name":"Forside"},
                "Settings":{"ZipCode":"1620","DeliveryZoneId":1,
                            "TimeslotUtc":"2025120216-180-1020","UserId":"abc",
                            "CombinedProductsAndSitecoreTimestamp":"j0gWvnwu-sP8MfX4u"}}"#,
        )
        .expect("parses");
        assert_eq!(page.settings.timestamp, "j0gWvnwu-sP8MfX4u");
        assert_eq!(page.settings.user_id(), Some("abc"));
        assert_eq!(page.settings.zip_code.as_deref(), Some("1620"));
    }

    #[test]
    fn an_anonymous_page_still_yields_a_zone() {
        let page: Page = serde_json::from_str(r#"{"Settings":{}}"#).expect("parses");
        assert_eq!(page.settings.delivery_zone_id, 1);
        assert_eq!(page.settings.user_id(), None);
    }
}
