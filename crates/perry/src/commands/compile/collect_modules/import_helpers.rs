//! Import-resolution + module-classification helpers for the module walk.
//!
//! Extracted from `collect_modules.rs` to keep it under the 2000-line cap.
//! These are the small, side-effect-light helpers the main discovery loop
//! (`collect_module_one` / `collect_module_finish`) consumes: env-define
//! mapping for HIR lowering, JS-module import scanning, lexical-vs-canonical
//! import resolution, and the known-node-submodule classifier.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use perry_hir::ModuleKind;

use super::{cached_resolve_import, CompilationContext};

/// #5009: build the bare-name → literal map perry-hir lowering consults to fold
/// `process.env.<NAME>` reads (`perry_hir::env_define_lookup`). Strips the
/// `process.env.` prefix the `perry.define` keys carry and converts each
/// [`super::super::DefineValue`] to the matching [`perry_hir::EnvDefine`]. Keys
/// that aren't `process.env.*` are skipped (only env defines are honored today).
pub(super) fn env_defines_for_lowering(
    define: &HashMap<String, super::super::DefineValue>,
) -> HashMap<String, perry_hir::EnvDefine> {
    define
        .iter()
        .filter_map(|(key, val)| {
            let name = key.strip_prefix("process.env.")?;
            let ev = match val {
                super::super::DefineValue::Str(s) => perry_hir::EnvDefine::Str(s.clone()),
                super::super::DefineValue::Bool(b) => perry_hir::EnvDefine::Bool(*b),
                super::super::DefineValue::Number(n) => perry_hir::EnvDefine::Num(*n),
                super::super::DefineValue::Null => perry_hir::EnvDefine::Null,
            };
            Some((name.to_string(), ev))
        })
        .collect()
}

/// Issue #818: scan a JS module's source for static ESM imports /
/// re-exports / string-literal dynamic imports, resolve each one
/// against the module's directory (with `resolve_with_extensions` so
/// extensionless and folder-index lookups work the same way they do at
/// import-time), and return the deduped list of file paths to add to
/// the bundle.
///
/// Bare specifiers (`react`, `@foo/bar`) and unresolvable relative
/// paths are skipped: bare specifiers are the V8 fallback's job to
/// resolve via the node_modules tree (we don't have a `require.resolve`
/// equivalent here without a full parse), and unresolvable relatives
/// just leak the same runtime error the V8 loader would have produced
/// anyway. This keeps the scan cheap and side-effect free.
pub(super) fn collect_js_module_imports(file_path: &std::path::Path, source: &str) -> Vec<PathBuf> {
    use std::sync::OnceLock;
    static IMPORT_RE: OnceLock<regex::Regex> = OnceLock::new();
    static EXPORT_FROM_RE: OnceLock<regex::Regex> = OnceLock::new();
    static DYNAMIC_IMPORT_RE: OnceLock<regex::Regex> = OnceLock::new();
    static BARE_IMPORT_RE: OnceLock<regex::Regex> = OnceLock::new();

    // `import ... from "spec"` — matches default/named/namespace forms.
    let import_re = IMPORT_RE.get_or_init(|| {
        regex::Regex::new(r#"(?m)^\s*import\s+(?:[^'"]+?\s+from\s+)?['"]([^'"]+)['"]"#)
            .expect("import regex")
    });
    // Bare side-effect import: `import "./foo.js";`
    let bare_re = BARE_IMPORT_RE.get_or_init(|| {
        regex::Regex::new(r#"(?m)^\s*import\s+['"]([^'"]+)['"]"#).expect("bare import regex")
    });
    // `export ... from "spec"` — covers `export *`, `export * as ns`,
    // `export { a, b }`. Captures the specifier.
    let export_re = EXPORT_FROM_RE.get_or_init(|| {
        regex::Regex::new(
            r#"(?m)^\s*export\s+(?:\*(?:\s+as\s+\w+)?|\{[^}]*\})\s+from\s+['"]([^'"]+)['"]"#,
        )
        .expect("export from regex")
    });
    // Dynamic `import("spec")` — string-literal only.
    let dyn_re = DYNAMIC_IMPORT_RE.get_or_init(|| {
        regex::Regex::new(r#"\bimport\s*\(\s*['"]([^'"]+)['"]\s*\)"#).expect("dynamic import regex")
    });

    let mut specs: Vec<String> = Vec::new();
    for cap in import_re.captures_iter(source) {
        specs.push(cap[1].to_string());
    }
    for cap in bare_re.captures_iter(source) {
        specs.push(cap[1].to_string());
    }
    for cap in export_re.captures_iter(source) {
        specs.push(cap[1].to_string());
    }
    for cap in dyn_re.captures_iter(source) {
        specs.push(cap[1].to_string());
    }

    let mut out: Vec<PathBuf> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    for spec in specs {
        // Only follow relative or absolute paths — bare specifiers like
        // `react` need the node_modules resolver which is more invasive
        // to call here. The original entry walker (TS path) already
        // pulled bare-specifier dependencies in via `cached_resolve_import`,
        // so the most common case (top-level package brings in submodules)
        // is covered. Inside a package's `node_modules` tree, all
        // sibling imports are relative-path anyway.
        if !(super::super::resolve::is_relative_specifier(&spec) || spec.starts_with('/')) {
            continue;
        }
        let resolved_path = if spec.starts_with('/') {
            super::super::resolve::resolve_absolute_import_paths(&spec)
        } else {
            super::super::resolve::resolve_relative_import_paths(&spec, file_path)
        };
        if let Some(resolved) = resolved_path {
            if seen.insert(resolved.canonical_path.clone()) {
                out.push(resolved.source_path);
            }
        }
    }
    out
}

pub(super) struct ResolvedImport {
    pub(super) canonical_path: PathBuf,
    pub(super) source_path: PathBuf,
    pub(super) kind: ModuleKind,
}

pub(super) fn cached_resolve_import_with_lexical_base(
    import_source: &str,
    lexical_importer_path: &Path,
    canonical_importer_path: &Path,
    ctx: &mut CompilationContext,
) -> Option<ResolvedImport> {
    // Module collection keys and reads use canonical paths, but source text
    // relative specifiers are written against the importer path the user
    // compiled. On platforms where /tmp is a symlink, resolving imports from
    // the canonical /private/tmp path can make a valid "../.." edge point at a
    // nonexistent sibling and leave imported classes unresolved.
    let resolved = cached_resolve_import_from_base(import_source, lexical_importer_path, ctx);
    if resolved.is_some() || lexical_importer_path == canonical_importer_path {
        return resolved;
    }
    cached_resolve_import_from_base(import_source, canonical_importer_path, ctx)
}

fn cached_resolve_import_from_base(
    import_source: &str,
    importer_path: &Path,
    ctx: &mut CompilationContext,
) -> Option<ResolvedImport> {
    let (canonical_path, kind) = cached_resolve_import(import_source, importer_path, ctx)?;
    let source_path = source_visible_resolved_path(import_source, importer_path, &canonical_path);
    Some(ResolvedImport {
        canonical_path,
        source_path,
        kind,
    })
}

fn source_visible_resolved_path(
    import_source: &str,
    importer_path: &Path,
    canonical_path: &Path,
) -> PathBuf {
    let resolved = if import_source.starts_with('/') {
        super::super::resolve::resolve_absolute_import_paths(import_source)
    } else if super::super::resolve::is_relative_specifier(import_source) {
        super::super::resolve::resolve_relative_import_paths(import_source, importer_path)
    } else {
        None
    };

    resolved
        .filter(|path| path.canonical_path == canonical_path)
        .map(|path| path.source_path)
        .unwrap_or_else(|| canonical_path.to_path_buf())
}

/// Issue #841: Node.js submodules that Perry knows about at the
/// resolver level (no perry-stdlib backing, no compiled-source backing)
/// but for which we still want to provide a minimal import surface so
/// `typeof import-name === "function"` and `import * as ns` work.
///
/// Each entry returns the bare submodule key that matches
/// `perry_runtime::node_submodules::SUBMODULES[i].key`. Codegen routes
/// every named/namespace import from these specifiers through the
/// runtime singleton getters in that module.
pub(in crate::commands::compile) fn known_node_submodule_key(source: &str) -> Option<&'static str> {
    let normalized = source.strip_prefix("node:").unwrap_or(source);
    match normalized {
        // node:timers — only the `import * as timers` namespace shape routes
        // through the submodule namespace; named imports keep the global
        // fast-path (gated in compile.rs). (#1213)
        "timers" => Some("timers"),
        "vm" => Some("vm"),
        "timers/promises" => Some("timers_promises"),
        "fs/promises" => Some("fs_promises"),
        "readline/promises" => Some("readline_promises"),
        "stream/promises" => Some("stream_promises"),
        "stream/consumers" => Some("stream_consumers"),
        // #1545: node:stream/web (WHATWG Web Streams). Named imports bind to
        // function singletons so `typeof ReadableStream === "function"`;
        // `new ReadableStream(...)` / `new CountQueuingStrategy(...)` are lowered
        // through the builtin-constructor dispatch in codegen regardless of the
        // import binding (see lower_call/builtin.rs), so these thunks only ever
        // run if the class is called *without* `new`.
        "stream/web" => Some("stream_web"),
        "sys" => Some("sys"),
        "test" => Some("test"),
        "test/reporters" => Some("test_reporters"),
        // Pino downstream (#906 follow-up): `require('node:diagnostics_channel')`
        // returns the module exports object. The CJS-wrap rewrites this as
        // `import diagChan from 'node:diagnostics_channel'`. Pre-fix the
        // codegen catch-all returned TAG_TRUE for that ExternFuncRef, so
        // `diagChan.tracingChannel(...)` threw
        // `TypeError: (boolean).tracingChannel is not a function`. Routing
        // through the namespace stub gives `diagChan` a real object whose
        // `tracingChannel` field is a callable thunk that hands back a
        // TracingChannel-shaped stub object — enough for pino to read
        // `asJsonChan.hasSubscribers === false` and take the fast path
        // without ever entering the tracing-instrumentation branch.
        "diagnostics_channel" => Some("diagnostics_channel"),
        "trace_events" => Some("trace_events"),
        // #1671: hono JSX runtime/streaming helpers. Perry renders JSX with the
        // built-in `js_jsx` runtime, so these submodules have no compiled-source
        // backing — they expose function singletons (jsx/jsxs/Fragment/JSXNode,
        // renderToReadableStream/Suspense) for code that imports the helpers
        // directly. Note these are NOT `node:`-prefixed; the strip above is a
        // no-op and they match verbatim.
        "hono/jsx/server" => Some("hono_jsx_server"),
        "hono/jsx/streaming" => Some("hono_jsx_streaming"),
        _ => None,
    }
}
