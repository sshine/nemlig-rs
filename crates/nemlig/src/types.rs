//! Value types shared across the API modules.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize};

/// A caller-supplied value the API would have rejected.
///
/// Raised before the request is sent, so a mistake costs nothing.
#[derive(Debug, Clone, thiserror::Error)]
#[error("{field} is invalid ({reason}): {value}")]
pub struct InvalidValue {
    /// Which argument was at fault.
    pub field: &'static str,
    /// What was wrong with it.
    pub reason: &'static str,
    /// The value as given.
    pub value: String,
}

/// A product number, such as `5070417`.
///
/// nemlig.com sends these as JSON strings even though they are always digits, and the
/// same number identifies a product in search results, basket lines and order lines.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProductId(String);

impl ProductId {
    /// Longest product number this crate accepts, well beyond the seven digits in use.
    pub const MAX_LEN: usize = 32;

    /// Borrow the number as it goes on the wire.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for ProductId {
    type Err = InvalidValue;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let invalid = |reason| InvalidValue {
            field: "product_id",
            reason,
            value: s.to_owned(),
        };
        if s.is_empty() {
            return Err(invalid("empty"));
        }
        if s.len() > Self::MAX_LEN {
            return Err(invalid("too long"));
        }
        if !s.bytes().all(|b| b.is_ascii_digit()) {
            return Err(invalid("not a number"));
        }
        Ok(Self(s.to_owned()))
    }
}

impl fmt::Display for ProductId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A delivery timeslot, as the opaque string nemlig.com uses to scope prices.
///
/// The shape is `YYYYMMDDHH-<start>-<end>`, for example `2025120216-180-1020`, but the
/// crate never takes it apart: search and product lookups pass it back unchanged, and
/// there is no endpoint that lists the slots to choose from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TimeslotUtc(String);

impl TimeslotUtc {
    /// Borrow the slot as it goes on the wire.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for TimeslotUtc {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl fmt::Display for TimeslotUtc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A number nemlig.com sends as a Danish-formatted string, such as `"249,87"`.
///
/// Basket lines carry both this and a plain float of the same quantity
/// (`UnitPrice` beside `UnitPriceCalc`), and the two can disagree in the last digit.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct DanishNumber(f64);

impl DanishNumber {
    /// The value as a float.
    pub fn get(self) -> f64 {
        self.0
    }
}

impl fmt::Display for DanishNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.2}", self.0)
    }
}

impl<'de> Deserialize<'de> for DanishNumber {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;

        let raw = String::deserialize(de)?;
        // A Danish number writes the thousands separator as a point and the decimal as a
        // comma. Dropping every point unconditionally would turn an ordinary "249.87" into
        // 24987, so the point is only a separator when a comma is there to be the decimal.
        let normalised = if raw.contains(',') {
            raw.replace('.', "").replace(',', ".")
        } else {
            raw.clone()
        };
        normalised
            .trim()
            .parse()
            .map(Self)
            .map_err(|_| D::Error::custom(format!("not a Danish decimal: {raw}")))
    }
}

/// Whether a product can be delivered and whether it is in stock.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[non_exhaustive]
pub struct Availability {
    /// Whether the product can be delivered in the current zone and timeslot.
    pub is_delivery_available: bool,
    /// Whether the warehouse holds it.
    pub is_available_in_stock: bool,
    /// Danish explanation shown when delivery is unavailable.
    #[serde(default)]
    pub delivery_availability_message: Option<String>,
    /// Danish explanation shown when stock is out.
    #[serde(default)]
    pub stock_availability_message: Option<String>,
}

/// A running offer on a product.
///
/// `type` and `code` are nemlig.com's own vocabulary, for example
/// `ProductCampaignMixOffer` / `FM` and `ProductCampaignBuyXForY` / `KF`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[non_exhaustive]
pub struct Campaign {
    /// How many of the product the offer needs.
    #[serde(default)]
    pub min_quantity: Option<u32>,
    /// Upper bound on how many the offer covers; `0` means no bound.
    #[serde(default)]
    pub max_quantity: Option<u32>,
    /// What `min_quantity` of them cost together.
    #[serde(default)]
    pub total_price: Option<f64>,
    /// Per-unit price under the offer.
    #[serde(default)]
    pub campaign_price: Option<f64>,
    /// nemlig.com's name for the kind of offer.
    #[serde(default, rename = "Type")]
    pub kind: Option<String>,
    /// Two-letter offer code.
    #[serde(default)]
    pub code: Option<String>,
    /// What the offer saves against the list price.
    #[serde(default)]
    pub discount_savings: Option<f64>,
    /// When the offer starts.
    #[serde(default)]
    pub interval_start: Option<chrono::DateTime<chrono::Utc>>,
    /// When the offer ends.
    #[serde(default)]
    pub interval_end: Option<chrono::DateTime<chrono::Utc>>,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn a_product_id_is_digits_and_nothing_else() {
        assert_eq!(
            "5070417"
                .parse::<ProductId>()
                .expect("digits parse")
                .as_str(),
            "5070417"
        );
        for bad in ["", "50a7041", "../etc", &"9".repeat(33)] {
            assert!(
                bad.parse::<ProductId>().is_err(),
                "{bad} should be rejected"
            );
        }
    }

    #[test]
    fn a_danish_decimal_reads_the_comma_as_the_point() {
        let n: DanishNumber = serde_json::from_str(r#""249,87""#).expect("parses");
        assert!((n.get() - 249.87).abs() < f64::EPSILON);
    }

    #[test]
    fn a_danish_thousands_separator_is_dropped() {
        let n: DanishNumber = serde_json::from_str(r#""1.249,50""#).expect("parses");
        assert!((n.get() - 1249.50).abs() < f64::EPSILON);
    }

    #[test]
    fn a_plain_decimal_point_is_not_mistaken_for_a_thousands_separator() {
        let n: DanishNumber = serde_json::from_str(r#""249.87""#).expect("parses");
        assert!((n.get() - 249.87).abs() < f64::EPSILON);
    }

    #[test]
    fn a_number_that_is_not_danish_is_an_error_not_a_zero() {
        assert!(serde_json::from_str::<DanishNumber>(r#""kr. 12""#).is_err());
    }
}

/// A Danish postal address, as the basket and the orders carry it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[non_exhaustive]
pub struct Address {
    /// Given name.
    #[serde(default)]
    pub first_name: Option<String>,
    /// Middle name, which Danish addresses often carry.
    #[serde(default)]
    pub middle_name: Option<String>,
    /// Family name.
    #[serde(default)]
    pub last_name: Option<String>,
    /// The whole name as the website would print it.
    #[serde(default)]
    pub name: Option<String>,
    /// Street.
    #[serde(default)]
    pub street_name: Option<String>,
    /// House number.
    #[serde(default, deserialize_with = "de::opt_u32")]
    pub house_number: Option<u32>,
    /// Letter after the house number, where there is one.
    #[serde(default)]
    pub house_number_letter: Option<String>,
    /// Floor, as `st`, `1`, `2` and so on.
    #[serde(default)]
    pub floor: Option<String>,
    /// Which side of the landing: `tv`, `th`, `mf`.
    #[serde(default)]
    pub side: Option<String>,
    /// Door, where the building numbers them.
    #[serde(default)]
    pub door: Option<String>,
    /// Postcode.
    #[serde(default, deserialize_with = "de::opt_u32")]
    pub postal_code: Option<u32>,
    /// The town the postcode names.
    #[serde(default)]
    pub postal_district: Option<String>,
    /// Company, for a business address.
    #[serde(default)]
    pub company_name: Option<String>,
    /// Who to ask for on arrival.
    #[serde(default)]
    pub contact_person: Option<String>,
    /// Mobile number the driver would call.
    #[serde(default)]
    pub mobile_number: Option<String>,
    /// Landline.
    #[serde(default)]
    pub phone_number: Option<String>,
    /// Whether the account has filled this address in at all.
    #[serde(default)]
    pub is_empty_address: bool,
}

/// Deserializers for fields nemlig.com is inconsistent about.
///
/// The same value arrives as a number from one endpoint and as a string from another --
/// a postcode is `1620` in the basket and `"1620"` in the page settings -- so the ones
/// that have been seen to vary are read either way rather than pinned to one.
pub(crate) mod de {
    use serde::{Deserialize, Deserializer};

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum NumberOrString {
        Number(u64),
        Text(String),
    }

    /// Read an optional whole number that may arrive quoted.
    pub(crate) fn opt_u32<'de, D: Deserializer<'de>>(de: D) -> Result<Option<u32>, D::Error> {
        let Some(raw) = Option::<NumberOrString>::deserialize(de)? else {
            return Ok(None);
        };
        Ok(match raw {
            NumberOrString::Number(n) => u32::try_from(n).ok(),
            NumberOrString::Text(s) => s.trim().parse().ok(),
        })
    }

    #[cfg(test)]
    mod tests {
        #![allow(clippy::expect_used)]

        use serde::Deserialize;

        #[derive(Deserialize)]
        struct Holder {
            #[serde(default, deserialize_with = "super::opt_u32")]
            postal_code: Option<u32>,
        }

        #[test]
        fn a_postcode_reads_quoted_or_bare() {
            for raw in [r#"{"postal_code":1620}"#, r#"{"postal_code":"1620"}"#] {
                let holder: Holder = serde_json::from_str(raw).expect("parses");
                assert_eq!(holder.postal_code, Some(1620));
            }
        }

        #[test]
        fn a_missing_or_unreadable_postcode_is_absent_rather_than_fatal() {
            for raw in [
                r#"{}"#,
                r#"{"postal_code":null}"#,
                r#"{"postal_code":"n/a"}"#,
            ] {
                let holder: Holder = serde_json::from_str(raw).expect("parses");
                assert_eq!(holder.postal_code, None);
            }
        }
    }
}
