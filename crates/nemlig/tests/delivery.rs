//! Delivery and account reads.

#![allow(clippy::expect_used)]

mod common;

use serde_json::json;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, ResponseTemplate};

use common::{mock, ok, site};

#[tokio::test]
async fn a_booked_slot_is_read_with_its_window() {
    let (client, server) = mock().await;
    site("GET", "/webapi/Order/DeliverySpot")
        .respond_with(ok(json!({
            "Id": "12345678",
            "CustomerName": "Anders And",
            "OrderNumber": "1050001234",
            "State": "Reorder",
            "TimeSlot": {"Start": "2025-11-25T16:00:00Z", "End": "2025-11-25T19:00:00Z"},
            "EditDeadline": "2025-11-24T23:00:00Z",
            "Progress": 1.0,
            "DeliveryTime": null,
        })))
        .mount(&server)
        .await;

    let spot = client
        .delivery()
        .spot()
        .await
        .expect("reads the slot")
        .expect("there is one booked");
    assert_eq!(spot.order_number.as_deref(), Some("1050001234"));
    assert!(spot.time_slot.is_some());
}

#[tokio::test]
async fn no_booked_delivery_is_none_rather_than_an_error() {
    let (client, server) = mock().await;
    Mock::given(method("GET"))
        .and(path("/webapi/Order/DeliverySpot"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    assert!(client.delivery().spot().await.expect("reads").is_none());
}

#[tokio::test]
async fn an_empty_body_also_means_nothing_is_booked() {
    let (client, server) = mock().await;
    Mock::given(method("GET"))
        .and(path("/webapi/Order/DeliverySpot"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;

    assert!(client.delivery().spot().await.expect("reads").is_none());
}

#[tokio::test]
async fn placements_keep_their_danish_wording() {
    let (client, server) = mock().await;
    site("GET", "/webapi/Checkout/GetDeliveryPlacements")
        .and(query_param("memberType", "0"))
        .respond_with(ok(json!([
            {"Key": "Frontdoor", "Value": "Foran min hoveddør"},
            {"Key": "Carport", "Value": "I carporten"},
        ])))
        .mount(&server)
        .await;

    let placements = client.delivery().placements().await.expect("reads them");
    assert_eq!(placements.len(), 2);
    assert_eq!(placements[0].value, "Foran min hoveddør");
}

#[tokio::test]
async fn the_page_settings_say_where_and_when_the_account_shops() {
    let (client, _server) = mock().await;
    let context = client
        .account()
        .context()
        .await
        .expect("reads the settings");
    assert_eq!(context.zip_code.as_deref(), Some("1620"));
    assert_eq!(context.delivery_zone_id, 1);
    assert_eq!(context.user_id(), Some(common::USER_ID));
}

#[tokio::test]
async fn the_cards_are_read_without_their_numbers() {
    let (client, server) = mock().await;
    site("GET", "/webapi/Checkout/GetCreditCards")
        .respond_with(ok(json!([{
            "CardId": 100001,
            "CardMask": "52340000****1234",
            "CardType": "MasterCard",
            "CardExpirationMonth": "06",
            "CardExpirationYear": "27",
            "FeeInPercent": 0.0,
            "IsDefault": true,
        }])))
        .mount(&server)
        .await;
    site("GET", "/webapi/Checkout/GetCardsFees")
        .and(query_param("total", "604.48"))
        .respond_with(ok(
            json!([{"CardId": 100001, "Amount": 0.0, "IsFailed": false}]),
        ))
        .mount(&server)
        .await;

    let cards = client
        .checkout()
        .credit_cards()
        .await
        .expect("reads the cards");
    assert_eq!(cards.len(), 1);
    assert!(cards[0].is_default);

    let fees = client
        .checkout()
        .card_fees(604.48, &[100001])
        .await
        .expect("reads the fees");
    assert_eq!(fees.len(), 1);
    assert!(!fees[0].is_failed);
}
