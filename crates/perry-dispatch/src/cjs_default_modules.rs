//! The one table of Node builtins whose CommonJS `module.exports` is a
//! namespace object distinct from the ESM namespace — the modules for which
//! `require('<mod>')`, `import x from '<mod>'` and
//! `process.getBuiltinModule('<mod>')` hand out a `<mod>.default` namespace
//! whose method calls and property reads must reach the base module.
//!
//! #9500 (from #9485 / #9498): this knowledge used to live in FOUR
//! hand-maintained copies — the runtime's `cjs_default_base_module` and
//! `cjs_default_namespace_name` tables, the `cjs_default_export_value` match
//! arm, the method-call router's own `<mod>.default → base` list, and the
//! HIR's `is_cjs_style_native_default_import` (itself duplicated in two files
//! that had already drifted apart: one lacked `ffi`, `inspector`,
//! `inspector/promises` and `wasi`). The router's copy drifted far enough that
//! `require('child_process').spawn(...)` dispatched under a name with no
//! bucket and returned `undefined` WITHOUT SPAWNING, which is why claude-code's
//! MCP stdio client reported `Failed to connect` (#9485). Every consumer now
//! derives from this table: adding a module here is the whole edit.
//!
//! Base names are the runtime's canonical spellings (`path.posix`, not
//! `path/posix`; `util`, not `sys`) — the alias folding happens in
//! `normalize_native_module_name` before any lookup here.

/// Builds the `(base, "<base>.default")` pairs from one literal per module, so
/// the two spellings cannot disagree.
macro_rules! cjs_default_namespace_modules {
    ($($base:literal),+ $(,)?) => {
        /// `(base module, "<base>.default")` for every Node builtin with a
        /// distinct CommonJS default namespace. Sorted by base name.
        pub const CJS_DEFAULT_NAMESPACE_MODULES: &[(&str, &str)] =
            &[$(($base, concat!($base, ".default"))),+];
    };
}

cjs_default_namespace_modules!(
    "async_hooks",
    "child_process",
    "cluster",
    "constants",
    "dns",
    "dns/promises",
    "ffi",
    "inspector",
    "inspector/promises",
    "module",
    "node-pty",
    "os",
    "path",
    "path.posix",
    "path.win32",
    "perf_hooks",
    "process",
    "punycode",
    "querystring",
    "repl",
    "sea",
    "url",
    "util",
    "wasi",
);

/// Whether `base` (canonical spelling) has a distinct `<base>.default`
/// CommonJS namespace.
pub fn has_cjs_default_namespace(base: &str) -> bool {
    cjs_default_namespace_name(base).is_some()
}

/// `base` → `"<base>.default"`, the name the CJS default namespace object is
/// created under.
pub fn cjs_default_namespace_name(base: &str) -> Option<&'static str> {
    CJS_DEFAULT_NAMESPACE_MODULES
        .iter()
        .find(|(b, _)| *b == base)
        .map(|(_, name)| *name)
}

/// `"<base>.default"` → `base`: the module a CJS default namespace's method
/// calls and property reads dispatch against.
pub fn cjs_default_base_module(namespace_name: &str) -> Option<&'static str> {
    CJS_DEFAULT_NAMESPACE_MODULES
        .iter()
        .find(|(_, name)| *name == namespace_name)
        .map(|(base, _)| *base)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_row_round_trips() {
        for (base, name) in CJS_DEFAULT_NAMESPACE_MODULES {
            assert_eq!(*name, format!("{base}.default"));
            assert_eq!(cjs_default_namespace_name(base), Some(*name));
            assert_eq!(cjs_default_base_module(name), Some(*base));
            assert!(has_cjs_default_namespace(base));
        }
    }

    #[test]
    fn rows_are_unique_and_sorted() {
        let bases: Vec<&str> = CJS_DEFAULT_NAMESPACE_MODULES
            .iter()
            .map(|(b, _)| *b)
            .collect();
        let mut sorted = bases.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(bases, sorted, "keep the table sorted and duplicate-free");
    }

    #[test]
    fn base_names_are_canonical_spellings() {
        for (base, _) in CJS_DEFAULT_NAMESPACE_MODULES {
            assert!(!base.starts_with("node:"), "{base}: strip the node: scheme");
            assert!(
                !matches!(*base, "sys" | "path/posix" | "path/win32"),
                "{base}: an alias, not a canonical module name"
            );
        }
    }

    #[test]
    fn modules_without_a_cjs_default_namespace_are_absent() {
        for base in [
            "fs",
            "crypto",
            "events",
            "http",
            "stream",
            "test",
            "child_process.default",
        ] {
            assert!(!has_cjs_default_namespace(base), "{base}");
            assert_eq!(cjs_default_namespace_name(base), None, "{base}");
        }
        assert_eq!(cjs_default_base_module("child_process"), None);
        assert_eq!(cjs_default_base_module("fs.default"), None);
    }

    /// The #9485 regression, pinned at the source of truth.
    #[test]
    fn child_process_default_maps_to_child_process() {
        assert_eq!(
            cjs_default_base_module("child_process.default"),
            Some("child_process")
        );
        assert_eq!(
            cjs_default_namespace_name("child_process"),
            Some("child_process.default")
        );
    }
}
