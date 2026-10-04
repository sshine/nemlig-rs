//! The tools, grouped the way the API is, and split by whether they change anything.
//!
//! Each group contributes a router, which [`NemligServer::router`] composes according to
//! the mode. Reads and writes are separate tools throughout: a client decides whether to
//! ask for confirmation from a tool's annotations, and a tool that both reads and writes
//! leaves it nothing to decide on.
//!
//! [`NemligServer::router`]: crate::server::NemligServer::router

pub mod account;
pub mod basket;
pub mod catalogue;
pub mod orders;
