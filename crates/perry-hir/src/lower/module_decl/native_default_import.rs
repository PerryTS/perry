//! Native/CJS-style default-import classification helpers — extracted from
//! `lower/module_decl.rs` (pure mechanical split, no logic changes).

pub(crate) fn canonicalize_native_import_source(raw_source: &str) -> String {
    let source = raw_source.strip_prefix("node:").unwrap_or(raw_source);
    if source.starts_with("@parcel/watcher-") {
        "@parcel/watcher".to_string()
    } else {
        source.to_string()
    }
}

/// Whether a native module's default import binds its CommonJS
/// `module.exports` — a `default` property read plus a builtin-module alias
/// for member calls — rather than the historical namespace object.
///
/// #9500: derived from the ONE shared table
/// (`perry_dispatch::CJS_DEFAULT_NAMESPACE_MODULES`) that the runtime's
/// property-read and method-call paths also consume, so the three cannot
/// drift apart again (the HIR alone used to carry two hand-written copies of
/// this list, and one had already lost `ffi`, `inspector`,
/// `inspector/promises` and `wasi`). The arms below are the deliberate
/// differences between "has a `<mod>.default` namespace at runtime" and
/// "lowers as a CJS-style default import", each spelled out so a new table
/// row is classified by this function automatically and only an exception
/// needs a line here.
pub(crate) fn is_cjs_style_native_default_import(module_name: &str) -> bool {
    match module_name {
        // `events`' CommonJS export is the `EventEmitter` class itself
        // (`cjs_default_export_value("events")`), not a `<mod>.default`
        // namespace, but the default import is still CJS-shaped.
        "events" => true,
        // Aliases the runtime folds before any table lookup
        // (`normalize_native_module_name`: `sys` → `util`, `path/posix` →
        // `path.posix`, `path/win32` → `path.win32`); the HIR sees the
        // import's own spelling.
        "sys" | "path/posix" | "path/win32" => true,
        // Table rows whose default import the HIR keeps on the namespace
        // object: `process` has its own lowering (the `source == "process"`
        // arms in `module_decl.rs`); `node-pty`, `repl` and `sea` never took
        // the CJS-style path — flipping them is a lowering change, not a
        // dedup, and is left for a follow-up.
        "node-pty" | "process" | "repl" | "sea" => false,
        other => perry_dispatch::has_cjs_default_namespace(other),
    }
}

pub(crate) fn node_submodule_default_export_key(module_name: &str) -> Option<&'static str> {
    match module_name {
        "test/reporters" => Some("test_reporters"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shared-table rows the HIR deliberately keeps on the namespace-object
    /// default. Adding a row to the table classifies it CJS-style unless it
    /// is listed here — so this list, not the table, is what a lowering
    /// decision edits.
    const NAMESPACE_OBJECT_DEFAULT_ROWS: &[&str] = &["node-pty", "process", "repl", "sea"];

    #[test]
    fn every_shared_table_row_is_classified() {
        for (base, _) in perry_dispatch::CJS_DEFAULT_NAMESPACE_MODULES {
            let expected = !NAMESPACE_OBJECT_DEFAULT_ROWS.contains(base);
            assert_eq!(
                is_cjs_style_native_default_import(base),
                expected,
                "`{base}`: shared-table row classified unexpectedly"
            );
        }
        for base in NAMESPACE_OBJECT_DEFAULT_ROWS {
            assert!(
                perry_dispatch::has_cjs_default_namespace(base),
                "`{base}` is listed as an exclusion but is not a shared-table row"
            );
        }
    }

    /// The spellings only the HIR sees (aliases + the callable-default
    /// `events`) stay CJS-style.
    #[test]
    fn hir_only_spellings_are_cjs_style() {
        for module in ["events", "sys", "path/posix", "path/win32"] {
            assert!(is_cjs_style_native_default_import(module), "{module}");
        }
    }

    /// #9485 / #9500: the rows one of the two former copies had lost, plus
    /// the module the regression was found on.
    #[test]
    fn formerly_drifted_rows_are_cjs_style() {
        for module in [
            "child_process",
            "ffi",
            "inspector",
            "inspector/promises",
            "wasi",
        ] {
            assert!(is_cjs_style_native_default_import(module), "{module}");
        }
    }

    #[test]
    fn plain_esm_shaped_builtins_are_not() {
        for module in [
            "fs",
            "fs/promises",
            "crypto",
            "http",
            "stream",
            "test",
            "buffer",
        ] {
            assert!(!is_cjs_style_native_default_import(module), "{module}");
        }
    }
}
