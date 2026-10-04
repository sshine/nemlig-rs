//! Turning the API's answers into something to read.
//!
//! Two shapes for everything: the wire types as JSON when `--json` was given, and columns
//! otherwise. Names are left in Danish, since that is what they are on the shelf and in
//! the app.

use std::io::Write as _;

use serde::Serialize;

use nemlig::api::basket::Basket;
use nemlig::api::checkout::CreditCard;
use nemlig::api::delivery::{DeliveryPlacement, DeliverySpot};
use nemlig::api::orders::{Order, OrderHistory};
use nemlig::api::products::ProductDetails;
use nemlig::api::search::{SearchResults, Suggestions};
use nemlig::{Context, SignIn};

use super::Result;

/// Print a value as JSON, or run the fallback that prints it as columns.
fn emit<T: Serialize>(value: &T, json: bool, text: impl FnOnce() -> Result<()>) -> Result<()> {
    if !json {
        return text();
    }
    let mut out = std::io::stdout().lock();
    serde_json::to_writer_pretty(&mut out, value)
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    out.write_all(b"\n")?;
    Ok(())
}

pub(super) fn sign_in(outcome: &SignIn, json: bool) -> Result<()> {
    emit(outcome, json, || {
        println!("signed in");
        if outcome.zip_code_differs {
            println!("note: the account's postcode differs from the one being browsed");
        }
        if !outcome.removed_meal_boxes.is_empty() {
            println!(
                "note: {} meal box(es) were dropped from the basket",
                outcome.removed_meal_boxes.len()
            );
        }
        Ok(())
    })
}

pub(super) fn context(context: &Context, username: &str, json: bool) -> Result<()> {
    emit(context, json, || {
        println!("account    {username}");
        println!("postcode   {}", context.zip_code.as_deref().unwrap_or("-"));
        println!("zone       {}", context.delivery_zone_id);
        println!(
            "timeslot   {}",
            context
                .timeslot_utc
                .as_ref()
                .map_or("-", |slot| slot.as_str())
        );
        Ok(())
    })
}

pub(super) fn search(results: &SearchResults, json: bool) -> Result<()> {
    emit(results, json, || {
        for product in &results.products.products {
            println!(
                "{:>9}  {:<44}  {:>8.2} kr  {}",
                product.id,
                truncate(&product.name, 44),
                product.price,
                if product.orderable() { "" } else { "udsolgt" },
            );
            if let Some(description) = &product.description {
                println!("{:>9}  {}", "", description);
            }
        }
        for recipe in &results.recipes {
            println!(
                "{:>9}  {}  ({})",
                "opskrift",
                recipe.name,
                recipe.total_time.as_deref().unwrap_or("-")
            );
        }
        println!(
            "\n{} of {} shown",
            results.products.products.len(),
            results.products.num_found
        );
        Ok(())
    })
}

pub(super) fn suggestions(suggestions: &Suggestions, json: bool) -> Result<()> {
    emit(suggestions, json, || {
        for suggestion in &suggestions.suggestions {
            println!("{suggestion}");
        }
        for category in &suggestions.categories {
            println!("{:<30} {}", category.name, category.url);
        }
        Ok(())
    })
}

pub(super) fn product(details: &ProductDetails, json: bool) -> Result<()> {
    emit(details, json, || {
        println!("{} {}", details.id, details.name);
        if let Some(brand) = &details.brand {
            println!("brand      {brand}");
        }
        if let Some(description) = &details.description {
            println!("size       {description}");
        }
        println!("price      {:.2} kr", details.price);
        if let (Some(unit), Some(label)) = (details.unit_price_calc, &details.unit_price_label) {
            println!("per unit   {unit:.2} {label}");
        }
        println!(
            "stock      {}",
            match &details.availability {
                Some(a) if a.is_delivery_available && a.is_available_in_stock => "på lager",
                Some(_) => "udsolgt",
                None => "-",
            }
        );
        if !details.labels.is_empty() {
            println!("labels     {}", details.labels.join(", "));
        }
        if let Some(campaign) = &details.campaign {
            println!(
                "tilbud     {} for {:.2} kr ({})",
                campaign.min_quantity.unwrap_or(1),
                campaign.total_price.unwrap_or(details.price),
                campaign.kind.as_deref().unwrap_or("-"),
            );
        }
        for attribute in &details.attributes {
            println!("{:<10} {}", attribute.name, attribute.value);
        }
        if let Some(declarations) = &details.declarations {
            if let Some(ingredients) = &declarations.ingredients {
                println!("\ningredienser\n{ingredients}");
            }
            if !declarations.nutrition_facts.is_empty() {
                println!("\nnæringsindhold");
                for fact in &declarations.nutrition_facts {
                    println!("{:<10} {}", fact.name, fact.value);
                }
            }
        }
        if let Some(text) = &details.text {
            println!("\n{}", strip_html(text));
        }
        Ok(())
    })
}

pub(super) fn basket(basket: &Basket, json: bool) -> Result<()> {
    emit(basket, json, || {
        if basket.is_empty() {
            println!("the basket is empty");
            return Ok(());
        }
        for line in &basket.lines {
            println!(
                "{:>9}  {:>3} x  {:<44}  {:>8.2} kr",
                line.id,
                line.quantity,
                truncate(&line.name, 44),
                line.price,
            );
        }
        println!("\n{:>62.2} kr", basket.total());
        Ok(())
    })
}

pub(super) fn orders(history: &OrderHistory, json: bool) -> Result<()> {
    emit(history, json, || {
        for order in &history.orders {
            println!(
                "{:>10}  {:<12}  {:<11}  {:>9.2} kr  {}",
                order.id.unwrap_or_default(),
                order.order_number.as_deref().unwrap_or("-"),
                order.status.to_string(),
                order.total,
                order
                    .order_date
                    .map(|date| date.format("%Y-%m-%d").to_string())
                    .unwrap_or_else(|| "-".to_owned()),
            );
        }
        Ok(())
    })
}

pub(super) fn order(order: &Order, json: bool) -> Result<()> {
    emit(order, json, || {
        let summary = &order.summary;
        println!(
            "order      {} ({})",
            summary.order_number.as_deref().unwrap_or("-"),
            summary.status
        );
        if let Some(window) = &summary.delivery_time {
            println!(
                "delivered  {} to {}",
                window.start.format("%Y-%m-%d %H:%M"),
                window.end.format("%H:%M")
            );
        }
        println!("goods      {:.2} kr", summary.sub_total);
        println!("delivery   {:.2} kr", summary.delivery_fee());
        println!("total      {:.2} kr\n", summary.total);

        for line in &order.lines {
            println!(
                "{:>9}  {:>6} {:<4}  {:<40}  {:>9.2} kr{}",
                line.product_number,
                format_quantity(line.quantity),
                line.unit.as_deref().unwrap_or(""),
                truncate(&line.product_name, 40),
                line.amount,
                if line.is_adjusted { "  (justeret)" } else { "" },
            );
        }
        Ok(())
    })
}

pub(super) fn delivery(spot: Option<&DeliverySpot>, json: bool) -> Result<()> {
    emit(&spot, json, || {
        let Some(spot) = spot else {
            println!("nothing booked");
            return Ok(());
        };
        println!("order      {}", spot.order_number.as_deref().unwrap_or("-"));
        println!("state      {}", spot.state.as_deref().unwrap_or("-"));
        if let Some(window) = &spot.time_slot {
            println!(
                "window     {} to {}",
                window.start.format("%Y-%m-%d %H:%M"),
                window.end.format("%H:%M")
            );
        }
        if let Some(deadline) = spot.edit_deadline {
            println!("editable   until {}", deadline.format("%Y-%m-%d %H:%M"));
        }
        Ok(())
    })
}

pub(super) fn placements(placements: &[DeliveryPlacement], json: bool) -> Result<()> {
    emit(&placements, json, || {
        for placement in placements {
            println!("{:<12}  {}", placement.key, placement.value);
        }
        Ok(())
    })
}

pub(super) fn cards(cards: &[CreditCard], json: bool) -> Result<()> {
    emit(&cards, json, || {
        for card in cards {
            println!(
                "{:>8}  {:<12}  {:<20}  {}/{}{}",
                card.card_id,
                card.card_type.as_deref().unwrap_or("-"),
                card.card_mask.as_deref().unwrap_or("-"),
                card.card_expiration_month.as_deref().unwrap_or("--"),
                card.card_expiration_year.as_deref().unwrap_or("--"),
                if card.is_default { "  (default)" } else { "" },
            );
        }
        Ok(())
    })
}

/// Cut a name to fit its column, on a character boundary.
fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_owned();
    }
    text.chars()
        .take(width.saturating_sub(1))
        .collect::<String>()
        + "…"
}

/// Print a quantity without the trailing zeroes a whole number does not need.
fn format_quantity(quantity: f64) -> String {
    if (quantity.fract()).abs() < f64::EPSILON {
        format!("{quantity:.0}")
    } else {
        format!("{quantity:.3}")
    }
}

/// Take the tags out of the marketing copy, which nemlig.com sends as HTML.
///
/// Deliberately crude. The copy is prose with paragraph and emphasis tags in it, and a
/// parser would be a dependency for one field of one subcommand.
fn strip_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn a_long_name_is_cut_on_a_character_boundary() {
        let cut = truncate("Økologisk sødmælk fra Thise Mejeri i karton", 10);
        assert_eq!(cut.chars().count(), 10);
        assert!(cut.ends_with('…'));
        assert_eq!(truncate("mælk", 10), "mælk");
    }

    #[test]
    fn a_whole_quantity_loses_its_decimals() {
        assert_eq!(format_quantity(2.0), "2");
        assert_eq!(format_quantity(0.734), "0.734");
    }

    #[test]
    fn marketing_copy_loses_its_tags_and_keeps_its_words() {
        assert_eq!(
            strip_html("<p>Cocio Classic er\n  den <b>originale</b>.</p>"),
            "Cocio Classic er den originale."
        );
    }
}
