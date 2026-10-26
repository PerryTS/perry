//! The CommonJS support module (`perry:cjs`): the program-wide part of the
//! CommonJS machinery as one ordinary compiled module.
//!
//! Every wrapped module imports it first, so it evaluates before any
//! CommonJS body (the module graph orders it; no runtime hook does). It holds
//! what Node keeps once per process:
//!
//! * `cache`, the program's `require.cache` (`Module._cache`): an object whose
//!   keys are exactly the graph's module paths, each absent until its module
//!   publishes its record (`__perry_declared_keys`), with a null prototype.
//!   A wrapper's `require.cache["<path>"]` is a load at a known slot.
//! * `main`, the entry's record (`require.main`), born here so dependencies
//!   that run before the entry's own preamble hold the same object; the
//!   entry fills it in place. `undefined` when the entry is not CommonJS.
//! * the deferred-require parent link (`setPending` / `takePending`), the
//!   shared `createRequire` for builtins, and the helpers every wrapper used
//!   to carry a copy of (`link`, `requireComputed`, `requireError`).
//!
//! The source depends only on the graph (its CommonJS module paths, in
//! collection order, and whether the entry is one of them), so a build
//! reproduces it byte for byte.

use std::fmt::Write as _;

/// The specifier wrapped modules import the support module by.
pub(crate) const SPECIFIER: &str = "perry:cjs";

/// The import every wrapped module carries first.
pub(super) const SUPPORT_IMPORT: &str = "import { cache as __perry_cjs_cache, main as __perry_cjs_main, extensions as __perry_cjs_extensions, setPending as __perry_cjs_set_pending, takePending as __perry_cjs_take_pending, builtin as __perry_cjs_builtin, link as __perry_cjs_link, requireComputed as __perry_cjs_require_computed, requireError as __perry_cjs_require_error } from 'perry:cjs';";

/// The support module's source for a graph whose CommonJS modules are
/// `paths` (canonical source paths, as the wrapper spells its cache key).
/// `entry_is_cjs`: the program entry is one of them (`require.main` is its
/// record; otherwise `undefined`, as for an ESM entry in Node).
/// `builtin_used`: some wrapped module has a literal builtin `require` arm;
/// otherwise `builtin` keeps no `createRequire` loader (and no native module
/// dispatcher is linked for it).
pub(crate) fn source(paths: &[String], entry_is_cjs: bool, builtin_used: bool) -> String {
    let mut keys = String::new();
    for (i, p) in paths.iter().enumerate() {
        if i > 0 {
            keys.push_str(", ");
        }
        let _ = write!(keys, "{}", serde_json::to_string(p).expect("path is JSON encodable"));
    }
    let main = if entry_is_cjs {
        "{ exports: undefined, __perry_cjs_record: true, id: undefined, path: undefined, filename: undefined, loaded: false, children: undefined, parent: undefined, paths: undefined, require: undefined }"
    } else {
        "undefined"
    };
    // The shared loader for builtins is bound to a path only for its
    // `require.resolve` base, which builtins never use.
    let base_path = serde_json::to_string(paths.first().map_or("/", String::as_str))
        .expect("path is JSON encodable");
    let builtin = if builtin_used {
        format!(
            "export function builtin(specifier) {{\n    if (base === undefined) base = __perry_cjs_support_create_require({base_path});\n    return base(specifier);\n}}"
        )
    } else {
        "export function builtin(specifier) {\n    throw requireError('error', 'MODULE_NOT_FOUND', \"Cannot find module '\" + specifier + \"'\");\n}".to_string()
    };
    format!(
        r#"import {{ createRequire as __perry_cjs_support_create_require }} from 'node:module';
export const cache = __perry_declared_keys({keys});
export const main = {main};
// The runtime's `Module._cache`, `process.mainModule` and
// `require.extensions` exist only for a program that can reach them (not
// closed): a closed program reads none of them by census.
export let extensions;
if (__perry_cjs_open()) {{
    extensions = __perry_cjs_install(cache, main);
}}
let pendingParent;
export function setPending(record) {{
    pendingParent = record;
}}
export function takePending() {{
    const record = pendingParent;
    pendingParent = undefined;
    return record;
}}
let base;
{builtin}
// Node's updateChildren: a required child is linked once to its first parent
// and appears once in each requiring module's children.
export function link(module, child) {{
    if (!child) return;
    if (child.parent === undefined) child.parent = module;
    if (module.children.indexOf(child) === -1) module.children.push(child);
}}
// A path module's live record, or its fresh evaluation after a deletion
// through the requiring module's own loader. (A wrapper's static cases do the
// same with their literal path, so each reads its own cache slot.)
function refresh(path, value, reloader) {{
    if (__perry_cjs_open()) {{
        const cached = cache[path];
        if (cached !== undefined) return cached.exports;
        if (!__perry_has_path_module(path)) return value;
        return reloader()(path);
    }} else {{
        return value;
    }}
}}
export function requireError(kind, code, message) {{
    const err = kind === 'type' ? new TypeError(message) : new Error(message);
    err.code = code;
    return err;
}}
export function requireComputed(specifier, dir, reloader) {{
    // A closed program has no computed require (design CJS_LINK §2.5).
    if (__perry_cjs_open()) {{
    // Builtins not hoisted as static imports resolve through createRequire,
    // which calls js_create_native_module_namespace, as Node's
    // require("process") does. The original spelling is kept: prefix-only
    // builtins are packages when requested without `node:` (#10410).
    if (__perry_is_builtin(specifier)) return builtin(specifier);
    // Native addons resolve through the same authenticated manifest and cache
    // as createRequire and process.dlopen, including computed paths.
    if (specifier.slice(-5) === '.node') return reloader()(specifier);
    // A module Perry compiled but that is reachable only by a computed path.
    // Next's webpack runtime uses absolute page paths and relative chunk
    // paths (`./chunks/` + id): a relative specifier joins the requiring
    // module's directory, as Node's per-module `require` does. The `./`
    // prefix is stripped textually: canonicalization only normalizes a path
    // that exists on disk, and deployed images may lack the sources.
    let path = specifier;
    if (specifier.charCodeAt(0) === 46) {{
        if (specifier.charCodeAt(1) === 47) {{
            path = dir + '/' + specifier.slice(2);
        }} else if (specifier.charCodeAt(1) === 46 && specifier.charCodeAt(2) === 47) {{
            path = dir + '/' + specifier;
        }} else if (specifier === '.' || specifier === '..') {{
            // Bare directory specifiers resolve through the directory's
            // `index.js` / package `main` (`js_require_path_module`'s
            // directory-candidate fallback).
            path = dir + '/' + specifier;
        }}
    }}
    const pathModule = __perry_require_path_module(path);
    if (pathModule !== undefined || __perry_has_path_module(path)) return refresh(path, pathModule, reloader);
    // An absolute `.json` path computed at run time (Next.js manifests) is
    // read and parsed; it is data, never code.
    if ((specifier.charCodeAt(0) === 47 || (specifier.length > 2 && specifier.charCodeAt(1) === 58)) && specifier.slice(-5) === '.json') {{
        return __perry_require_json_disk(specifier);
    }}
    // A computed bare platform package can name a manifest-listed addon
    // through package.json main, without a .node suffix in the request.
    return reloader()(specifier);
    }} else {{
        throw requireError('error', 'MODULE_NOT_FOUND', "Cannot find module '" + specifier + "'");
    }}
}}
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_source_is_a_function_of_the_graph_only() {
        let paths = vec!["/p/a.js".to_string(), "/p/b.js".to_string()];
        assert_eq!(source(&paths, true, true), source(&paths, true, true));
        let s = source(&paths, true, true);
        assert!(s.contains(r#"__perry_declared_keys("/p/a.js", "/p/b.js")"#), "{s}");
        assert!(s.contains("export const main = { exports: undefined"), "{s}");
        assert!(source(&paths, false, true).contains("export const main = undefined;"));
    }

    #[test]
    fn the_support_module_is_not_recognised_as_a_wrapped_module() {
        // `is_cjs_wrapped_module` keys on the wrapper's createRequire alias.
        let s = source(&["/p/a.js".to_string()], false, true);
        assert!(!s.contains("__perry_cjs_create_require"));
        assert!(!s.contains("__perry_cjs_strict_require"));
    }
}
