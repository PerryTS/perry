//! CLI-only helpers. Every capability is opt-in; an empty-feature build has
//! no dependencies. The runtime never links this crate, even when built in
//! the same Cargo invocation as the CLI.
#[cfg(feature = "apple-jwt")]
pub mod apple_jwt;
#[cfg(feature = "dotenv")]
pub mod dotenv;
#[cfg(feature = "logger")]
pub mod logger;
#[cfg(feature = "progress")]
pub mod terminal_progress;
#[cfg(feature = "utc")]
pub mod utc;
