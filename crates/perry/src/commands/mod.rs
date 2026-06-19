//! CLI command implementations

// Feature-gated command modules (#5422). Only modules that nothing in the
// always-compiled core depends on are gated; `audit`, `publish` and `setup`
// stay compiled because their audit/config helpers are used by core paths
// (telemetry, login, run, compile/bundle_ios, publish itself).
#[cfg(feature = "mobile-cli")]
pub mod appstore;
pub mod attest;
pub mod audit;
pub mod cache;
pub mod check;
pub mod compile;
pub mod deps;
#[cfg(feature = "watch-cli")]
pub mod dev;
pub mod doctor;
pub mod explain;
pub mod fix_applier;
pub mod fixer;
pub mod harmonyos_hap;
pub mod i18n;
pub mod init;
pub mod install;
pub mod lock;
pub mod login;
pub mod lower_diagnostic;
#[cfg(feature = "native-cli")]
pub mod native;
pub mod perry_lock;
pub(crate) mod progress;
pub mod publish;
pub mod run;
pub mod sandbox_profile;
pub mod sanitize;
pub mod setup;
pub mod stdlib_features;
pub mod typecheck;
pub mod types;
pub mod update;
#[cfg(feature = "updater-cli")]
pub mod updater;
#[cfg(feature = "audit-cli")]
pub mod verify;
#[cfg(feature = "mobile-cli")]
pub mod widget;
