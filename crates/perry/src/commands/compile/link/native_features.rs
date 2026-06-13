//! Per-project cargo feature forwarding for `perry.nativeLibrary` crates.
//!
//! Native-library crates are built with `cargo build --release` and the
//! crate's own default feature set. Engines that serve more than one app
//! profile (e.g. a pure-2D game on a 2D+3D engine) gate optional
//! subsystems behind cargo features, but apps had no way to pick a
//! profile: feature flags were never forwarded to the nativeLibrary
//! build (see the workaround note in bloom-engine's
//! `native/macos/Cargo.toml`, which default-enables Jolt for exactly
//! this reason). Apps can now declare, in their `perry.toml`:
//!
//! ```toml
//! [native-library."@bloomengine/engine"]
//! default-features = false
//! features = ["renderer2d"]
//! ```
//!
//! The table key is the npm package name. A module spec like
//! `@bloomengine/engine/core` matches its package's entry (longest key
//! wins when nested). `features` / `default-features` map 1:1 onto
//! `cargo build --features …` / `--no-default-features`.
//!
//! Misconfiguration surfaces as a normal cargo error (unknown feature)
//! or an undefined-symbol link error when the app imports an FFI
//! function the chosen feature set compiled out — both name the crate,
//! so the failure is attributable.

use std::path::Path;
use std::process::Command;

/// Feature overrides for one native-library package, as declared in the
/// project's `perry.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct NativeLibraryBuildOverride {
    /// Cargo features to enable (`--features a,b,c`).
    pub features: Vec<String>,
    /// When false, pass `--no-default-features`.
    pub default_features: bool,
}

/// Look up the `[native-library."<pkg>"]` override matching `module`
/// (a module spec such as `@bloomengine/engine/core`) in the project's
/// `perry.toml`. Returns `None` when the file, table, or a matching key
/// is absent — the build then proceeds exactly as before this feature
/// existed.
pub(super) fn lookup_native_library_override(
    project_root: &Path,
    module: &str,
) -> Option<NativeLibraryBuildOverride> {
    let content = std::fs::read_to_string(project_root.join("perry.toml")).ok()?;
    let doc: toml::Table = content.parse().ok()?;
    lookup_in_table(&doc, module)
}

fn lookup_in_table(doc: &toml::Table, module: &str) -> Option<NativeLibraryBuildOverride> {
    let table = doc.get("native-library")?.as_table()?;
    // Longest matching key wins so `@scope/pkg/sub` beats `@scope/pkg`
    // if someone ever publishes a nested package name.
    let mut best: Option<(&str, &toml::Value)> = None;
    for (key, value) in table {
        let matches = module == key || module.starts_with(&format!("{key}/"));
        if matches && best.is_none_or(|(prev, _)| key.len() > prev.len()) {
            best = Some((key, value));
        }
    }
    let (_, value) = best?;
    let entry = value.as_table()?;
    let features = entry
        .get("features")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let default_features = entry
        .get("default-features")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    Some(NativeLibraryBuildOverride {
        features,
        default_features,
    })
}

/// Apply the project's `[native-library."<pkg>"]` override (if any) to a
/// native-library cargo invocation: `--no-default-features` /
/// `--features a,b`, logging the selection in text mode. No-op when the
/// project declares nothing for this package.
pub(super) fn apply_native_library_override(
    cargo_cmd: &mut Command,
    project_root: &Path,
    module: &str,
    text_output: bool,
) {
    let Some(ovr) = lookup_native_library_override(project_root, module) else {
        return;
    };
    if !ovr.default_features {
        cargo_cmd.arg("--no-default-features");
    }
    if !ovr.features.is_empty() {
        cargo_cmd.arg("--features").arg(ovr.features.join(","));
    }
    if text_output {
        println!(
            "  native-library features for {}: default-features={} features=[{}]",
            module,
            ovr.default_features,
            ovr.features.join(", ")
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> toml::Table {
        s.parse().unwrap()
    }

    #[test]
    fn module_spec_matches_package_key() {
        let doc = parse(
            r#"
[native-library."@bloomengine/engine"]
default-features = false
features = ["renderer2d", "mp3"]
"#,
        );
        let ovr = lookup_in_table(&doc, "@bloomengine/engine/core").unwrap();
        assert!(!ovr.default_features);
        assert_eq!(ovr.features, vec!["renderer2d", "mp3"]);
        // Exact package name (no submodule) matches too.
        assert!(lookup_in_table(&doc, "@bloomengine/engine").is_some());
        // Unrelated packages don't.
        assert!(lookup_in_table(&doc, "@other/pkg/core").is_none());
        // Prefix match must respect path boundaries.
        assert!(lookup_in_table(&doc, "@bloomengine/engine-extras/core").is_none());
    }

    #[test]
    fn absent_table_or_fields_default_cleanly() {
        let doc = parse("[project]\nname = \"x\"\n");
        assert!(lookup_in_table(&doc, "@bloomengine/engine/core").is_none());

        let doc = parse("[native-library.\"@bloomengine/engine\"]\n");
        let ovr = lookup_in_table(&doc, "@bloomengine/engine/core").unwrap();
        assert!(ovr.default_features);
        assert!(ovr.features.is_empty());
    }

    #[test]
    fn longest_key_wins() {
        let doc = parse(
            r#"
[native-library."@scope/pkg"]
features = ["outer"]
[native-library."@scope/pkg/sub"]
features = ["inner"]
"#,
        );
        let ovr = lookup_in_table(&doc, "@scope/pkg/sub/mod").unwrap();
        assert_eq!(ovr.features, vec!["inner"]);
    }
}
