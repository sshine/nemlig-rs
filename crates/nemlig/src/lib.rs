//! An API client for [nemlig.com], the Danish online supermarket.
//!
//! Unofficial and unaffiliated. nemlig.com publishes no API; this crate drives the one its
//! own website uses, recovered from captured traffic, and it can stop working whenever the
//! website changes.
//!
//! ## What it covers
//!
//! Signing in, searching the catalogue, reading a product, reading and changing the basket,
//! reading past orders, and reading the delivery slot and address the account is set up
//! with. Everything here either reads, or changes the basket.
//!
//! Placing an order is not part of the default build. It exists behind the `place-order`
//! feature and a second, runtime opt-in, and the two are separate on purpose: nothing in a
//! default `cargo add nemlig` can spend money.
//!
//! ## Signing in
//!
//! ```no_run
//! use nemlig::{Client, Credentials};
//!
//! # async fn example() -> nemlig::Result<()> {
//! let client = Client::new(Credentials::new("someone@example.com", "hunter2"))?;
//!
//! let results = client.search().query("kakaomælk").take(5).send().await?;
//! for product in &results.products.products {
//!     println!("{:>9}  {:<40} {:>8.2} kr", product.id, product.name, product.price);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! There is no explicit login step. The first call signs in, and later calls renew the
//! five-minute bearer token from the session cookies rather than from the password.
//! [`Client::sign_in`] exists for when you want the handshake's own answer, such as to
//! check a password.
//!
//! ## Keeping the session
//!
//! Signing in costs five requests, and nemlig.com's authentication cookie lasts a year.
//! A process that runs once per invocation, such as a command-line tool, should keep it:
//!
//! ```no_run
//! # use nemlig::{Client, Credentials};
//! # fn example() -> nemlig::Result<()> {
//! let client = Client::builder()
//!     .credentials(Credentials::new("someone@example.com", "hunter2"))
//!     .session_file("/home/me/.cache/nemlig/session.json")
//!     .build()?;
//! # Ok(())
//! # }
//! ```
//!
//! The file is written so only its owner can read it, but it is still a credential.
//!
//! ## The basket
//!
//! nemlig.com has no endpoint that removes a basket line. Quantities are absolute, and
//! setting one to zero is what removes it, so the crate offers a single verb:
//!
//! ```no_run
//! # use nemlig::{Client, Credentials};
//! # async fn example(client: Client) -> nemlig::Result<()> {
//! let id = "5070417".parse()?;
//! client.basket().set_quantity(&id, 3).await?;  // three of them, not three more
//! client.basket().set_quantity(&id, 0).await?;  // gone
//! # Ok(())
//! # }
//! ```
//!
//! ## Pacing
//!
//! nemlig.com states no rate limit and sends no headers describing one, but it sits behind
//! Cloudflare, which answers a burst with a challenge rather than a number. The client
//! therefore spaces its own requests out, five a second by default, and treats a 429's
//! `Retry-After` as the only authority on how long to wait. Reads are retried; writes are
//! not, since the client cannot tell from the method whether one went through.
//!
//! ## Danish fields
//!
//! Responses are in Danish and stay that way: category and label names, campaign
//! descriptions, units such as `Stk.` and `kr./Kg.`, and nutrition keys such as `Energi`.
//! Numbers inside strings use the comma as the decimal separator, which
//! [`DanishNumber`](types::DanishNumber) reads.
//!
//! [nemlig.com]: https://www.nemlig.com

#![warn(missing_docs)]

mod auth;
mod client;
mod context;
mod error;
mod ratelimit;
mod session;

pub mod api;
pub mod types;

pub use auth::SignIn;
pub use client::{
    Client, ClientBuilder, Credentials, DEFAULT_BASE_URL, DEFAULT_CONTEXT_TTL,
    DEFAULT_SEARCH_BASE_URL, DEFAULT_TIMEOUT, DEFAULT_USER_AGENT, Secret, WEB_APP_VERSION,
};
pub use context::Context;
pub use error::{AuthError, Error, Result};
pub use ratelimit::{DEFAULT_MAX_RETRIES, DEFAULT_MAX_RETRY_DELAY, DEFAULT_MIN_INTERVAL};
pub use types::{Availability, Campaign, DanishNumber, InvalidValue, ProductId, TimeslotUtc};
