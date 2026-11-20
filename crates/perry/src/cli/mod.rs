//! CLI-only helpers for the `perry` binary: dotenv loading, logging, terminal
//! progress, UTC timestamps, and Apple JWT signing.
//!
//! These used to be the `perry-cli-support` crate. They live here because this
//! binary was their only consumer; the dependency that isolation was buying
//! (`p256`, for JWT signing) is still opt-in, behind `apple-api`.
//!
//! Nothing here may be depended on by a runtime, stdlib or binding crate: a
//! runtime that links `p256` would pay for the CLI's signing path.
#[cfg(feature = "apple-api")]
pub mod apple_jwt;
pub mod dotenv;
pub mod logger;
pub mod terminal_progress;
pub mod utc;
