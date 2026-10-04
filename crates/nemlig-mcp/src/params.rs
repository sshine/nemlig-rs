//! The input schema for each tool.
//!
//! One struct per tool, with the doc comments becoming the argument descriptions a client
//! shows. The constraints are as tight as nemlig.com's own rules allow, because a call
//! rejected here costs nothing while one rejected by the website costs a request.
//!
//! The library's own types are not reused: that would put a `schemars` dependency on a
//! crate that has no business carrying one.

use schemars::JsonSchema;
use serde::Deserialize;

/// Arguments for searching the catalogue.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SearchProducts {
    /// What to look for. The catalogue is Danish, so Danish terms match best.
    #[schemars(length(min = 1, max = 200))]
    pub query: String,
    /// How many products to return. Defaults to 20.
    #[schemars(range(min = 1, max = 100))]
    pub limit: Option<u32>,
    /// How many products to skip, for reading past the first page.
    #[schemars(range(min = 0, max = 10000))]
    pub skip: Option<u32>,
    /// How many matching recipes to return alongside the products. Defaults to none.
    #[schemars(range(min = 0, max = 20))]
    pub recipes: Option<u32>,
}

/// Arguments for the search box's own suggestions.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SuggestQueries {
    /// A partial Danish term.
    #[schemars(length(min = 1, max = 200))]
    pub query: String,
}

/// Arguments for reading one product.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GetProduct {
    /// A product number such as "5070417", or the slug its page lives at such as
    /// "cocio-kakaomaelk-daase-5070417". The slug costs one request and the number costs
    /// two, and every product a search or a basket returns carries its slug.
    #[schemars(length(min = 1, max = 200))]
    pub product: String,
}

/// Arguments for listing past orders.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ListOrders {
    /// How many orders to return. Defaults to 20.
    #[schemars(range(min = 1, max = 100))]
    pub limit: Option<u32>,
    /// How many orders to skip, newest first.
    #[schemars(range(min = 0, max = 10000))]
    pub skip: Option<u32>,
}

/// Arguments for reading one past order.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GetOrder {
    /// The order's numeric id, as nemlig_list_orders reports it. This is not the order
    /// number the website prints.
    pub order_id: i64,
}

/// Arguments for changing how many of a product the basket holds.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SetBasketQuantity {
    /// The product number, digits only.
    #[schemars(regex(pattern = r"^[0-9]{1,32}$"))]
    pub product_id: String,
    /// How many the basket should hold afterwards. This is the resulting quantity, not an
    /// amount to add, and 0 removes the line.
    #[schemars(range(min = 0, max = 999))]
    pub quantity: u32,
}

/// Arguments for removing every line from the basket.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct EmptyBasket {
    /// How many lines the basket currently holds, as nemlig_get_basket reports. The call
    /// is refused if the basket holds a different number, so a basket that changed since
    /// it was read is not emptied by mistake.
    #[schemars(range(min = 0, max = 999))]
    pub confirm_line_count: u32,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn a_product_number_is_constrained_to_digits_in_the_schema() {
        let schema = serde_json::to_value(schemars::schema_for!(SetBasketQuantity))
            .expect("the schema serializes");
        let pattern = schema["properties"]["product_id"]["pattern"]
            .as_str()
            .expect("the pattern is in the schema");
        assert_eq!(pattern, r"^[0-9]{1,32}$");
    }

    #[test]
    fn every_argument_carries_a_description() {
        for schema in [
            serde_json::to_value(schemars::schema_for!(SearchProducts)),
            serde_json::to_value(schemars::schema_for!(SetBasketQuantity)),
            serde_json::to_value(schemars::schema_for!(EmptyBasket)),
        ] {
            let schema = schema.expect("the schema serializes");
            let properties = schema["properties"]
                .as_object()
                .expect("there are properties");
            for (name, property) in properties {
                assert!(
                    property["description"]
                        .as_str()
                        .is_some_and(|d| !d.is_empty()),
                    "{name} has no description"
                );
            }
        }
    }
}
