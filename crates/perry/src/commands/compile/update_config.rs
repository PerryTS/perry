//! `perry.update` — the update-check settings baked into a compiled binary.
//!
//! Perry's own CLI has checked for updates for a long time. An application
//! Perry *compiles* has not: shipping one meant the author wrote their own
//! version check, or shipped none and hoped users noticed.
//!
//! This is the declarative half. A `perry.update` block in the project's
//! package.json (or an `[update]` table in perry.toml) is validated at compile
//! time and baked into the executable, where the runtime reads it at startup.
//!
//! # Default off, and off means nothing is emitted
//!
//! With no block configured, nothing is embedded and the binary is unchanged —
//! not "embedded and disabled". A feature whose off-state still emits code is a
//! feature you cannot prove is off, and there is a codegen test asserting the
//! startup call is absent.
//!
//! # Why validation happens here rather than at runtime
//!
//! A typo in an update URL is discovered by the person who typed it, at build
//! time, with a message naming the key — or by their users, in production, as
//! silence. The first is strictly better, so the rules below are compile
//! errors: HTTPS only, a source-appropriate key set, an interval that means
//! something.

use anyhow::{bail, Result};
use serde::Serialize;

/// The shape of the embedded blob. The runtime refuses a version it does not
/// know rather than guessing at a layout, so this is a hard gate, not a hint.
const BLOB_SCHEMA: u32 = 1;

/// Where a compiled app asks what its latest version is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum AppUpdateSource {
    /// A GitHub releases API URL plus a tag pattern.
    GhReleases,
    /// An npm registry packument.
    Npm,
    /// GitHub Packages, which is npm-shaped and always authenticated.
    GhRegistry,
    /// An HTTPS URL returning `{"version": "..."}`.
    Custom,
}

impl AppUpdateSource {
    fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "gh-releases" => Some(Self::GhReleases),
            "npm" => Some(Self::Npm),
            "gh-registry" => Some(Self::GhRegistry),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::GhReleases => "gh-releases",
            Self::Npm => "npm",
            Self::GhRegistry => "gh-registry",
            Self::Custom => "custom",
        }
    }
}

/// The validated block, in the form the runtime reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AppUpdateConfig {
    pub(crate) schema: u32,
    /// Names the per-app state directory. Defaults to the binary's own name,
    /// so two apps never share a throttle.
    pub(crate) app_id: String,
    /// What to call the app in its own update notice.
    pub(crate) bin_name: String,
    /// The version the running binary believes it is.
    pub(crate) current_version: String,
    pub(crate) source: AppUpdateSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) package: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) registry: Option<String>,
    pub(crate) check_interval_hours: u64,
    pub(crate) notify_interval_hours: u64,
    /// A command the notice tells the user to run. Empty means the notice
    /// points at the release URL instead, which is the right default: an app
    /// that has not implemented an update command should not be advertising
    /// one.
    pub(crate) command: String,
    /// An environment variable that switches the check off for this app,
    /// alongside the always-honoured global ones.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) skip_env: Option<String>,
}

impl AppUpdateConfig {
    /// The bytes to embed.
    pub(crate) fn to_blob(&self) -> String {
        // JSON rather than a packed struct: the runtime parses it once at
        // startup, so the cost is irrelevant, and a self-describing blob means
        // a field added later cannot be misread as a different one.
        serde_json::to_string(self).expect("an AppUpdateConfig always serializes")
    }
}

/// Read `perry.update` from package.json, with `[update]` in perry.toml
/// overriding it key by key.
///
/// Returns `None` when neither configures the feature, which is the common
/// case and the one where nothing is emitted.
pub(crate) fn resolve(
    pkg: Option<&serde_json::Value>,
    perry_toml: Option<&toml::Table>,
    default_bin_name: &str,
    default_version: &str,
) -> Result<Option<AppUpdateConfig>> {
    let json = pkg
        .and_then(|p| p.get("perry"))
        .and_then(|p| p.get("update"))
        .and_then(|u| u.as_object());
    let toml_table = perry_toml
        .and_then(|t| t.get("update"))
        .and_then(|u| u.as_table());
    if json.is_none() && toml_table.is_none() {
        return None.pipe_ok();
    }

    // perry.toml wins: it is the app-metadata manifest, and a project carrying
    // both is expressing "the manifest is authoritative".
    let string = |camel: &str, snake: &str| -> Option<String> {
        toml_table
            .and_then(|t| t.get(snake))
            .and_then(|v| v.as_str())
            .or_else(|| json.and_then(|j| j.get(camel)).and_then(|v| v.as_str()))
            .map(str::to_string)
    };
    let number = |camel: &str, snake: &str| -> Option<u64> {
        toml_table
            .and_then(|t| t.get(snake))
            .and_then(|v| v.as_integer())
            .map(|i| i.max(0) as u64)
            .or_else(|| json.and_then(|j| j.get(camel)).and_then(|v| v.as_u64()))
    };
    let boolean = |camel: &str, snake: &str| -> Option<bool> {
        toml_table
            .and_then(|t| t.get(snake))
            .and_then(|v| v.as_bool())
            .or_else(|| json.and_then(|j| j.get(camel)).and_then(|v| v.as_bool()))
    };

    // An explicit `enabled = false` is how a project keeps its settings on disk
    // while switching the feature off, so it must not be a way to embed a
    // disabled block: nothing is emitted at all.
    if boolean("enabled", "enabled") == Some(false) {
        return None.pipe_ok();
    }

    let Some(source_raw) = string("source", "source") else {
        bail!("perry.update needs a `source`: one of gh-releases, npm, gh-registry, custom");
    };
    let Some(source) = AppUpdateSource::parse(&source_raw) else {
        bail!(
            "perry.update: unknown source `{source_raw}`. \
             Valid values: gh-releases, npm, gh-registry, custom"
        );
    };

    let url = string("url", "url");
    let tag = string("tag", "tag");
    let package = string("package", "package");
    let registry = string("registry", "registry");

    // Each source needs the keys it actually reads, and saying so at build time
    // is the difference between the author fixing a typo and their users
    // getting silence.
    match source {
        AppUpdateSource::GhReleases | AppUpdateSource::Custom => {
            if url.is_none() {
                bail!("perry.update: source `{}` needs a `url`", source.name());
            }
        }
        AppUpdateSource::Npm | AppUpdateSource::GhRegistry => {
            if package.is_none() {
                bail!(
                    "perry.update: source `{}` needs a `package` (the published name)",
                    source.name()
                );
            }
        }
    }

    for (label, value) in [("url", &url), ("registry", &registry)] {
        if let Some(value) = value {
            require_https(label, value)?;
        }
    }

    let check_interval_hours = number("checkInterval", "check_interval_hours").unwrap_or(24);
    let notify_interval_hours = number("notifyInterval", "notify_interval_hours").unwrap_or(24);
    if check_interval_hours == 0 {
        bail!(
            "perry.update: `checkInterval` of 0 would check on every run. \
             Use a positive number of hours, or remove the block to disable checks."
        );
    }

    let bin_name = string("binName", "bin_name").unwrap_or_else(|| default_bin_name.to_string());
    let app_id = string("appId", "app_id").unwrap_or_else(|| bin_name.clone());
    let current_version =
        string("currentVersion", "current_version").unwrap_or_else(|| default_version.to_string());
    if current_version.trim().is_empty() {
        bail!(
            "perry.update: the app has no version to compare against. \
             Set `version` in package.json, or `currentVersion` in the update block."
        );
    }

    Some(AppUpdateConfig {
        schema: BLOB_SCHEMA,
        app_id,
        bin_name,
        current_version,
        source,
        url,
        tag,
        package,
        registry,
        check_interval_hours,
        notify_interval_hours,
        command: string("command", "command").unwrap_or_default(),
        skip_env: string("skipEnv", "skip_env"),
    })
    .pipe_ok()
}

/// HTTPS, or loopback for someone testing against a local server.
///
/// Plain HTTP is refused rather than warned about: an on-path attacker can
/// suppress a legitimate update by answering "you are current", and a warning
/// in build output is not where that gets noticed.
fn require_https(label: &str, value: &str) -> Result<()> {
    let lower = value.to_ascii_lowercase();
    if lower.starts_with("https://") {
        return Ok(());
    }
    // The exemption has to stop at a host boundary. A bare prefix test accepts
    // `http://localhost.example.test/v` and `http://127.0.0.1.example.test/v`,
    // both of which are ordinary remote hosts — and would ship a plain-HTTP
    // update URL in the binary, which is exactly what this rule exists to stop.
    let loopback = ["http://127.0.0.1", "http://localhost", "http://[::1]"]
        .iter()
        .any(|prefix| {
            lower.strip_prefix(prefix).is_some_and(|rest| {
                rest.is_empty() || rest.starts_with(':') || rest.starts_with('/')
            })
        });
    if loopback {
        return Ok(());
    }
    bail!(
        "perry.update: `{label}` must be an https:// URL (got `{value}`). \
         Loopback http:// is allowed for local testing."
    )
}

/// A tiny helper so the happy paths above read as expressions.
trait PipeOk<T> {
    fn pipe_ok(self) -> Result<T>;
}
impl<T> PipeOk<T> for T {
    fn pipe_ok(self) -> Result<T> {
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg(update: serde_json::Value) -> serde_json::Value {
        serde_json::json!({ "perry": { "update": update } })
    }

    fn resolve_pkg(update: serde_json::Value) -> Result<Option<AppUpdateConfig>> {
        resolve(Some(&pkg(update)), None, "myapp", "1.2.3")
    }

    /// The common case: nothing configured, nothing embedded. A feature whose
    /// off-state still emits something is one you cannot prove is off.
    #[test]
    fn no_block_means_no_config_at_all() {
        assert_eq!(
            resolve(
                Some(&serde_json::json!({ "name": "x" })),
                None,
                "myapp",
                "1.2.3"
            )
            .unwrap(),
            None
        );
        assert_eq!(resolve(None, None, "myapp", "1.2.3").unwrap(), None);
    }

    /// `enabled = false` keeps the settings on disk and emits nothing — rather
    /// than embedding a disabled block, which would ship the URL and the
    /// startup call for a feature nobody asked to run.
    #[test]
    fn enabled_false_emits_nothing_rather_than_a_disabled_block() {
        let config = resolve_pkg(serde_json::json!({
            "enabled": false,
            "source": "npm",
            "package": "myapp"
        }))
        .unwrap();
        assert_eq!(config, None);
    }

    #[test]
    fn a_minimal_npm_block_resolves_with_defaults() {
        let config = resolve_pkg(serde_json::json!({ "source": "npm", "package": "myapp" }))
            .unwrap()
            .expect("configured");
        assert_eq!(config.source, AppUpdateSource::Npm);
        assert_eq!(config.package.as_deref(), Some("myapp"));
        assert_eq!(
            config.bin_name, "myapp",
            "defaults to the binary's own name"
        );
        assert_eq!(config.app_id, "myapp", "and the state directory follows it");
        assert_eq!(config.current_version, "1.2.3", "taken from package.json");
        assert_eq!(config.check_interval_hours, 24);
        assert_eq!(config.notify_interval_hours, 24);
        assert_eq!(
            config.command, "",
            "an app that has not implemented an update command must not advertise one"
        );
    }

    /// Each source needs the keys it reads. Saying so at build time is the
    /// difference between the author fixing a typo and their users getting
    /// silence.
    #[test]
    fn each_source_requires_the_keys_it_reads() {
        let missing_url = resolve_pkg(serde_json::json!({ "source": "gh-releases" }));
        assert!(format!("{:#}", missing_url.unwrap_err()).contains("needs a `url`"));

        let missing_package = resolve_pkg(serde_json::json!({ "source": "npm" }));
        assert!(format!("{:#}", missing_package.unwrap_err()).contains("needs a `package`"));

        let unknown = resolve_pkg(serde_json::json!({ "source": "carrier-pigeon" }));
        assert!(format!("{:#}", unknown.unwrap_err()).contains("unknown source"));

        let no_source = resolve_pkg(serde_json::json!({ "url": "https://example.test/v" }));
        assert!(format!("{:#}", no_source.unwrap_err()).contains("needs a `source`"));
    }

    /// Plain HTTP is refused, not warned about: an on-path attacker can
    /// suppress an update by answering "you are current", and a warning in
    /// build output is not where that gets noticed.
    #[test]
    fn plain_http_is_refused_but_loopback_is_allowed() {
        let insecure = resolve_pkg(serde_json::json!({
            "source": "custom",
            "url": "http://updates.example.test/v"
        }));
        assert!(format!("{:#}", insecure.unwrap_err()).contains("must be an https:// URL"));

        for local in [
            "http://127.0.0.1:8080/v",
            "http://localhost:8080/v",
            "http://[::1]:8080/v",
            "http://localhost/v",
            "http://localhost",
        ] {
            assert!(
                resolve_pkg(serde_json::json!({ "source": "custom", "url": local })).is_ok(),
                "{local} should be allowed for local testing"
            );
        }

        // ★ The exemption stops at a host boundary. These are ordinary remote
        // hosts that merely START with a loopback literal, and a prefix test
        // would ship plain HTTP in the binary.
        for impostor in [
            "http://localhost.example.test/v",
            "http://127.0.0.1.example.test/v",
            "http://localhost-evil.test/v",
        ] {
            let error = resolve_pkg(serde_json::json!({ "source": "custom", "url": impostor }));
            assert!(
                error.is_err(),
                "{impostor} is a remote host and must be refused"
            );
        }
    }

    /// A zero check interval would ask on every run. That is a mistake rather
    /// than a preference, and "remove the block" is the way to disable checks.
    #[test]
    fn a_zero_check_interval_is_an_error_not_an_every_run_mode() {
        let error = resolve_pkg(serde_json::json!({
            "source": "npm",
            "package": "myapp",
            "checkInterval": 0
        }));
        assert!(format!("{:#}", error.unwrap_err()).contains("would check on every run"));
    }

    /// An app with no version has nothing to compare against, so the check
    /// could only ever report "newer" or crash. Caught at build time.
    #[test]
    fn an_app_without_a_version_is_rejected() {
        let error = resolve(
            Some(&pkg(
                serde_json::json!({ "source": "npm", "package": "myapp" }),
            )),
            None,
            "myapp",
            "  ",
        );
        assert!(format!("{:#}", error.unwrap_err()).contains("no version to compare"));
    }

    /// perry.toml is the app-metadata manifest, so it wins key by key — a
    /// project carrying both is saying the manifest is authoritative.
    #[test]
    fn perry_toml_overrides_package_json_key_by_key() {
        let mut update = toml::Table::new();
        update.insert("package".into(), toml::Value::String("from-toml".into()));
        update.insert("check_interval_hours".into(), toml::Value::Integer(6));
        let mut root = toml::Table::new();
        root.insert("update".into(), toml::Value::Table(update));

        let config = resolve(
            Some(&pkg(serde_json::json!({
                "source": "npm",
                "package": "from-json",
                "checkInterval": 24,
                "binName": "kept-from-json"
            }))),
            Some(&root),
            "myapp",
            "1.2.3",
        )
        .unwrap()
        .expect("configured");

        assert_eq!(config.package.as_deref(), Some("from-toml"));
        assert_eq!(config.check_interval_hours, 6);
        assert_eq!(
            config.bin_name, "kept-from-json",
            "a key the manifest does not set is left alone"
        );
    }

    /// The blob is what the runtime reads, so its shape is a contract: the
    /// schema is always present, and absent optionals are absent rather than
    /// null.
    #[test]
    fn the_blob_stamps_its_schema_and_omits_unset_keys() {
        let config = resolve_pkg(serde_json::json!({ "source": "npm", "package": "myapp" }))
            .unwrap()
            .expect("configured");
        let blob = config.to_blob();
        assert!(blob.contains("\"schema\":1"), "{blob}");
        assert!(
            blob.contains("\"source\":\"npm\""),
            "kebab-case on the wire: {blob}"
        );
        assert!(
            !blob.contains("\"url\""),
            "an unset optional is omitted: {blob}"
        );
        assert!(!blob.contains("null"), "and never written as null: {blob}");
    }

    /// The source name on the wire must be the same spelling the config uses,
    /// so a reader never has to translate between two vocabularies.
    #[test]
    fn every_source_round_trips_through_its_wire_name() {
        for source in [
            AppUpdateSource::GhReleases,
            AppUpdateSource::Npm,
            AppUpdateSource::GhRegistry,
            AppUpdateSource::Custom,
        ] {
            assert_eq!(
                AppUpdateSource::parse(source.name()),
                Some(source),
                "{} must parse back from its own name",
                source.name()
            );
        }
    }
}
