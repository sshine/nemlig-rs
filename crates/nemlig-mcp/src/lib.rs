//! An MCP server over the nemlig.com API.
//!
//! The binary is a thin wrapper around this: startup lives in [`cli`], the served surface in
//! [`server`] and [`tools`]. It is a library only so the integration tests can drive a real
//! client against a real server over a pipe, rather than asserting on the router in isolation.

pub mod cli;
pub mod error;
pub mod params;
pub mod render;
pub mod server;
pub mod tools;
