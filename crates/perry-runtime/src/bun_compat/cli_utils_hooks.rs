//! The Bun CLI utility surface the always-live runtime reaches.
//!
//! `native_module_property_by_name` answers `bun.YAML` / `.TOML` / `.semver` /
//! `.JSONL` and decorates every `bun.hash` value, and the data-URL/`import
//! with { type: "toml" }` loader parses TOML. Those callers are live in every
//! program, so they must not name `cli_utils` (YAML, TOML, semver, serde_json,
//! zstd) directly or the prebuilt full-feature runtime keeps it in all of them.
//! They call these forwarders instead, which reach `cli_utils` only through
//! slots its `bun-cli-utils` install fills (see `crate::feature_hooks`). An
//! empty slot answers exactly what `cli_utils_stub` did in a build without the
//! feature.

use crate::feature_hooks::Hook;

static YAML: Hook<fn() -> f64> = Hook::empty();
static TOML: Hook<fn() -> f64> = Hook::empty();
static SEMVER: Hook<fn() -> f64> = Hook::empty();
static JSONL: Hook<fn() -> f64> = Hook::empty();
static DECORATE_HASH: Hook<fn(f64) -> f64> = Hook::empty();
static TOML_PARSE: Hook<fn(&str) -> Result<f64, f64>> = Hook::empty();

fn undefined() -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

pub fn js_bun_yaml() -> f64 {
    YAML.get().map_or_else(undefined, |f| f())
}

pub fn js_bun_toml() -> f64 {
    TOML.get().map_or_else(undefined, |f| f())
}

pub fn js_bun_semver() -> f64 {
    SEMVER.get().map_or_else(undefined, |f| f())
}

pub fn js_bun_jsonl() -> f64 {
    JSONL.get().map_or_else(undefined, |f| f())
}

pub fn decorate_bun_hash(value: f64) -> f64 {
    DECORATE_HASH.get().map_or(value, |f| f(value))
}

/// `None` when TOML support is not installed: the loader then takes its
/// deferred-error path, as a build without `bun-cli-utils` always did.
pub(crate) fn toml_parse_result(source: &str) -> Option<Result<f64, f64>> {
    TOML_PARSE.get().map(|f| f(source))
}

/// The `bun-cli-utils` install: fill every slot above from the real backends.
#[cfg(feature = "bun-cli-utils")]
pub(crate) fn install() {
    use super::cli_utils;
    YAML.set(cli_utils::js_bun_yaml_impl);
    TOML.set(cli_utils::js_bun_toml_impl);
    SEMVER.set(cli_utils::js_bun_semver_impl);
    JSONL.set(cli_utils::js_bun_jsonl_impl);
    DECORATE_HASH.set(cli_utils::decorate_bun_hash_impl);
    TOML_PARSE.set(cli_utils::toml_parse_result_impl);
}
