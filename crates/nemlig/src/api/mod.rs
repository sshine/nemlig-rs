//! The API, one module per group of endpoints.
//!
//! Each group is reached through an accessor on [`Client`], which hands back a handle
//! borrowing the client: `client.basket().get()`, `client.orders().history(0, 10)`. The
//! handles are free to make and hold nothing of their own.
//!
//! Two hosts sit behind this. [`search`] talks to nemlig.com's search gateway; everything
//! else talks to the website. The difference shows up only in which headers go out, which
//! the client handles.

pub mod basket;
pub mod checkout;
pub mod delivery;
pub mod orders;
pub mod products;
pub mod search;

use crate::client::{Backend, Client};
use crate::context::Context;
use crate::error::Result;

impl Client {
    /// The account itself: who is signed in, and where and when it would be delivered to.
    pub fn account(&self) -> AccountApi<'_> {
        AccountApi::new(self)
    }
}

/// Reads about the signed-in account.
#[derive(Debug, Clone, Copy)]
pub struct AccountApi<'a> {
    client: &'a Client,
}

impl<'a> AccountApi<'a> {
    pub(crate) fn new(client: &'a Client) -> Self {
        Self { client }
    }

    /// The page settings the catalogue is read through: catalogue version, delivery slot,
    /// delivery zone, postcode and account id.
    ///
    /// Cached for [`DEFAULT_CONTEXT_TTL`](crate::DEFAULT_CONTEXT_TTL) unless the client was
    /// built with a different one, so calling this repeatedly costs one request.
    pub async fn context(&self) -> Result<Context> {
        self.client.context().await
    }

    /// Whatever nemlig.com says about the signed-in account.
    ///
    /// Returned as sent rather than as a struct: this endpoint was never captured, so a
    /// type for it would be a guess, and a guess that drops fields is worse than none.
    pub async fn current_user(&self) -> Result<serde_json::Value> {
        let req = self
            .client
            .get(Backend::Site, &["webapi", "user", "GetCurrentUser"])?;
        self.client.send_json(req, "the current user").await
    }
}
