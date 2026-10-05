//! Which perry-stdlib feature installs a program links.
//!
//! perry-stdlib's always-live hubs reach optional subsystems only through hook
//! slots that each feature fills from its own `js_stdlib_install_*` entry point
//! (perry-stdlib `common::feature_hooks`). Whenever the stdlib is linked, the
//! link step generates an object (perry-codegen
//! `stubs::generate_feature_installer_object`) whose static constructor
//! registers an installer calling the entry points chosen here;
//! `js_stdlib_init_dispatch` runs it.
//!
//! - an auto-optimized archive was built with exactly the program's features,
//!   so the installer calls `js_stdlib_install_compiled` and installs all of
//!   them — the behavior every link had before the hooks existed;
//! - a prebuilt full-feature archive (an installed perry with no workspace to
//!   rebuild from) installs only the features the program needs, so the linker
//!   can drop the rest of the archive. [`install_symbols`] maps that feature
//!   set to entry points.

use std::collections::BTreeSet;

/// Installs every feature the linked archive was compiled with.
pub const INSTALL_COMPILED_SYMBOL: &str = "js_stdlib_install_compiled";

/// What the generated installer should do.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum FeatureInstalls {
    /// Install every compiled feature (auto-optimized or opted-out links, and
    /// programs whose dynamic code the compiler cannot see through).
    #[default]
    Compiled,
    /// Install exactly these perry-stdlib Cargo features (prebuilt link).
    Selected(BTreeSet<String>),
}

/// Each installable feature's entry point and the perry-stdlib Cargo features
/// whose `[features]` closure contains it. A program selecting any trigger gets
/// the install; the `triggers_match_cargo_feature_closure` test keeps this in
/// step with `crates/perry-stdlib/Cargo.toml`, so a new implication there
/// fails the test instead of silently leaving a subsystem uninstalled.
const INSTALLS: &[(&str, &[&str])] = &[
    (
        "js_stdlib_install_bundled_streams",
        &[
            "bundled-streams",
            "default",
            "full",
            "http-client",
            "streams-brotli",
            "web-fetch",
        ],
    ),
    (
        "js_stdlib_install_events",
        &["bundled-events", "default", "full"],
    ),
    (
        "js_stdlib_install_bundled_nodemailer",
        &["bundled-nodemailer", "default", "email", "full"],
    ),
    (
        "js_stdlib_install_database_sqlite",
        &["database", "database-sqlite", "default", "full"],
    ),
    ("js_stdlib_install_crypto", &["crypto", "default", "full"]),
    (
        "js_stdlib_install_tls_runtime",
        &["default", "external-net-tls", "full", "tls", "tls-runtime"],
    ),
    (
        "js_stdlib_install_compression_gzip",
        &[
            "compression",
            "compression-brotli",
            "compression-gzip",
            "compression-zstd",
            "default",
            "full",
        ],
    ),
    (
        "js_stdlib_install_external_zlib_pump",
        &["external-zlib-pump"],
    ),
    (
        "js_stdlib_install_external_http_client_pump",
        &["external-http-client-pump"],
    ),
    (
        "js_stdlib_install_external_http_server_pump",
        &["external-http-server-pump"],
    ),
    (
        "js_stdlib_install_external_net_pump",
        &["external-net-pump"],
    ),
    (
        "js_stdlib_install_web_fetch",
        &["default", "full", "http-client", "web-fetch"],
    ),
    (
        "js_stdlib_install_turnloop_http_client",
        &[
            "default",
            "full",
            "http-client",
            "turnloop-http-client",
            "web-fetch",
        ],
    ),
    (
        "js_stdlib_install_turnloop_smtp_client",
        &[
            "bundled-nodemailer",
            "default",
            "email",
            "full",
            "turnloop-smtp-client",
        ],
    ),
];

/// The install entry points a program with stdlib features `features` needs,
/// in table order.
pub fn install_symbols(features: &BTreeSet<String>) -> Vec<String> {
    INSTALLS
        .iter()
        .filter(|(_, triggers)| triggers.iter().any(|t| features.contains(*t)))
        .map(|(symbol, _)| (*symbol).to_string())
        .collect()
}

/// The install entry points the generated installer calls for `installs`.
pub fn installer_callees(installs: &FeatureInstalls) -> Vec<String> {
    match installs {
        FeatureInstalls::Compiled => vec![INSTALL_COMPILED_SYMBOL.to_string()],
        FeatureInstalls::Selected(features) => install_symbols(features),
    }
}

/// Installs every runtime feature the linked `libperry_runtime.a` was
/// compiled with.
pub const RUNTIME_INSTALL_COMPILED_SYMBOL: &str = "js_runtime_install_compiled";

/// perry-runtime's installable features, as [`INSTALLS`] is for perry-stdlib:
/// each entry point and the perry-runtime Cargo features whose `[features]`
/// closure contains it (checked against `crates/perry-runtime/Cargo.toml` by
/// `runtime_triggers_match_cargo_feature_closure`). The runtime's always-live
/// hubs reach these features only through slots the installs fill
/// (perry-runtime `feature_hooks`).
const RUNTIME_INSTALLS: &[(&str, &[&str])] = &[
    ("js_runtime_install_dyn_eval", &["default", "dyn-eval"]),
    (
        "js_runtime_install_bun_cli_utils",
        &["bun-cli-utils", "default"],
    ),
    (
        "js_runtime_install_intl_namespace",
        &["default", "intl-namespace"],
    ),
    ("js_runtime_install_temporal", &["default", "temporal"]),
    (
        "js_runtime_install_intl_datetime",
        &["default", "intl-datetime"],
    ),
    (
        "js_runtime_install_regex_engine",
        &["default", "regex-engine"],
    ),
    ("js_runtime_install_url_engine", &["default", "url-engine"]),
];

/// The runtime install entry points the generated installer calls for
/// `installs` (perry-runtime features, without the `perry-runtime/` prefix).
pub fn runtime_installer_callees(installs: &FeatureInstalls) -> Vec<String> {
    match installs {
        FeatureInstalls::Compiled => vec![RUNTIME_INSTALL_COMPILED_SYMBOL.to_string()],
        FeatureInstalls::Selected(features) => RUNTIME_INSTALLS
            .iter()
            .filter(|(_, triggers)| triggers.iter().any(|t| features.contains(*t)))
            .map(|(symbol, _)| (*symbol).to_string())
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    /// perry-stdlib's `[features]` table as `name -> [entries]`, parsed from
    /// the workspace copy. Only same-crate entries (no `dep:` / `x/y`) matter
    /// for the closure.
    fn stdlib_feature_table() -> Option<BTreeMap<String, Vec<String>>> {
        feature_table("perry-stdlib")
    }

    fn feature_table(krate: &str) -> Option<BTreeMap<String, Vec<String>>> {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../{krate}/Cargo.toml"));
        let text = std::fs::read_to_string(path).ok()?;
        let doc: toml::Table = text.parse().ok()?;
        let features = doc.get("features")?.as_table()?;
        Some(
            features
                .iter()
                .map(|(name, entries)| {
                    let entries = entries
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|e| e.as_str())
                                .filter(|e| !e.starts_with("dep:") && !e.contains('/'))
                                .map(str::to_string)
                                .collect()
                        })
                        .unwrap_or_default();
                    (name.clone(), entries)
                })
                .collect(),
        )
    }

    fn closure(table: &BTreeMap<String, Vec<String>>, root: &str) -> BTreeSet<String> {
        let mut seen = BTreeSet::new();
        let mut stack = vec![root.to_string()];
        while let Some(f) = stack.pop() {
            if seen.insert(f.clone()) {
                stack.extend(table.get(&f).cloned().unwrap_or_default());
            }
        }
        seen
    }

    /// The feature an install symbol is named after (`events` installs the
    /// `bundled-events` module helpers).
    fn installed_feature(symbol: &str) -> &str {
        match symbol {
            "js_stdlib_install_events" => "bundled-events",
            s => s.strip_prefix("js_stdlib_install_").unwrap(),
        }
    }

    #[test]
    fn triggers_match_cargo_feature_closure() {
        let Some(table) = stdlib_feature_table() else {
            // Packaged build without the workspace sibling: nothing to check.
            return;
        };
        for (symbol, triggers) in INSTALLS {
            let installed = installed_feature(symbol).replace('_', "-");
            let mut expected: BTreeSet<String> = table
                .keys()
                .filter(|f| closure(&table, f).contains(&installed))
                .cloned()
                .collect();
            let listed: BTreeSet<String> = triggers.iter().map(|t| t.to_string()).collect();
            assert_eq!(
                listed, expected,
                "{symbol}: trigger list must equal every perry-stdlib feature whose closure \
                 contains `{installed}` (update INSTALLS after changing Cargo.toml [features])"
            );
        }
    }

    #[test]
    fn every_install_symbol_is_defined_by_perry_stdlib() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../perry-stdlib/src/common/feature_hooks.rs");
        let Ok(source) = std::fs::read_to_string(path) else {
            return;
        };
        for (symbol, _) in INSTALLS {
            assert!(
                source.contains(&format!("pub extern \"C\" fn {symbol}()")),
                "{symbol} is listed here but perry-stdlib does not define it"
            );
        }
        for line in source.lines() {
            if let Some(rest) = line
                .trim()
                .strip_prefix("pub extern \"C\" fn js_stdlib_install_")
            {
                let symbol = format!("js_stdlib_install_{}", rest.split('(').next().unwrap());
                assert!(
                    symbol == INSTALL_COMPILED_SYMBOL || INSTALLS.iter().any(|(s, _)| *s == symbol),
                    "perry-stdlib defines {symbol} but INSTALLS does not list it"
                );
            }
        }
    }

    #[test]
    fn selected_features_map_to_their_installs() {
        let features: BTreeSet<String> = ["web-fetch", "external-net-pump", "async-runtime"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            install_symbols(&features),
            vec![
                "js_stdlib_install_bundled_streams",
                "js_stdlib_install_external_net_pump",
                "js_stdlib_install_web_fetch",
                "js_stdlib_install_turnloop_http_client",
            ]
        );
        assert_eq!(
            installer_callees(&FeatureInstalls::Compiled),
            vec![INSTALL_COMPILED_SYMBOL.to_string()]
        );
    }

    #[test]
    fn runtime_triggers_match_cargo_feature_closure() {
        let Some(table) = feature_table("perry-runtime") else {
            return;
        };
        for (symbol, triggers) in RUNTIME_INSTALLS {
            let installed = symbol
                .strip_prefix("js_runtime_install_")
                .unwrap()
                .replace('_', "-");
            let expected: BTreeSet<String> = table
                .keys()
                .filter(|f| closure(&table, f).contains(&installed))
                .cloned()
                .collect();
            let listed: BTreeSet<String> = triggers.iter().map(|t| t.to_string()).collect();
            assert_eq!(
                listed, expected,
                "{symbol}: trigger list must equal every perry-runtime feature whose closure \
                 contains `{installed}` (update RUNTIME_INSTALLS after changing Cargo.toml [features])"
            );
        }
    }

    #[test]
    fn every_runtime_install_symbol_is_defined_by_perry_runtime() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../perry-runtime/src/feature_hooks.rs");
        let Ok(source) = std::fs::read_to_string(path) else {
            return;
        };
        for (symbol, _) in RUNTIME_INSTALLS {
            assert!(
                source.contains(&format!("pub extern \"C\" fn {symbol}()")),
                "{symbol} is listed here but perry-runtime does not define it"
            );
        }
        for line in source.lines() {
            if let Some(rest) = line
                .trim()
                .strip_prefix("pub extern \"C\" fn js_runtime_install_")
            {
                let symbol = format!("js_runtime_install_{}", rest.split('(').next().unwrap());
                assert!(
                    symbol == RUNTIME_INSTALL_COMPILED_SYMBOL
                        || RUNTIME_INSTALLS.iter().any(|(s, _)| *s == symbol),
                    "perry-runtime defines {symbol} but RUNTIME_INSTALLS does not list it"
                );
            }
        }
    }
}
