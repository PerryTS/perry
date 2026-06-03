//! Top-level CJS-to-ESM wrap orchestration: hoist requires/classes and
//! assemble the IIFE-shaped module.

use super::*;
use std::path::Path;

/// Wrap CJS source as ESM. `source_path` is the absolute path of the file
/// being wrapped — used to resolve `require('./relative')` targets when
/// peeking at re-export wrappers' transitive named exports.
pub(in crate::commands::compile) fn wrap_commonjs(source: &str, source_path: &Path) -> String {
    // Issue #665 (fifth pass): rewrite `module.exports = class X { ... };`
    // expressions into declaration form + bare-identifier assignment so the
    // existing hoist + direct-default-export machinery surfaces the class.
    // Without this, the leaf `module.exports = class Abstract { ... };` shape
    // (rate-limiter-flexible/lib/RateLimiterAbstract.js) leaves `_cjs` as the
    // module's default — opaque to compile.rs's class-identity tracking, so
    // a downstream `class Memory extends RateLimiterAbstract { constructor(o){
    // super(o); ... } }` silently no-ops the parent constructor. The fix
    // mirrors the declaration-form path that v0.5.839 already wired up.
    let owned_source;
    let source: &str = match rewrite_module_exports_class_expression(source) {
        Some(rewritten) => {
            owned_source = rewritten;
            &owned_source
        }
        None => source,
    };

    let require_specs = extract_require_specifiers(source);

    // Issue #652: hoist top-level `class X { ... }` declarations OUT of the
    // IIFE so the consumer's `import { X } from "pkg"` resolves to the real
    // class instead of a runtime property access on `_cjs.X`.
    //
    // Pre-fix the cjs_wrap left every class inside the IIFE body. Perry's
    // HIR then sees `MiniPool` as `exported: false` (it's nested in a
    // closure body), and the consumer-side resolver couldn't find the
    // class. Calling `new MiniPool(...)` produced an empty instance with
    // no fields and no methods — typeof p.query was undefined, p.url was
    // undefined.
    //
    // The hoisted classes still get `exports.X = X` set inside the IIFE
    // body, so the default-export `_cjs` shape (`_cjs.X`) keeps working.
    // We replace the hoisted-class names in `named_exports` with direct
    // re-exports `export { X }` instead of `export const X = _cjs.X`,
    // so the import resolves to the class declaration directly.
    let (hoisted_class_block, hoisted_class_names, source_without_hoists) =
        extract_top_level_class_decls(source);

    // Issue #665 (third pass): for each spec that has a unique CJS-side alias
    // `var/const/let X = require('Y')`, use X as the import local name instead
    // of `_req_N`. This lets compile.rs propagate class identity for X — the
    // default-import handler registers `imported_class_ctors[X]`, and the
    // codegen super-call dispatch at expr.rs:5094 then resolves a child
    // class's `extends X` to the source module's standalone constructor.
    //
    // Without this, the wrap surfaced the alias only as a module-scope
    // `const X = _req_N;`, which HIR sees as a plain Let aliasing an import
    // — class identity for X is lost, so `class Child extends X { ctor(){
    // super(o) } }` silently no-ops the parent constructor (the
    // rate-limiter-flexible RateLimiterMemory ← RateLimiterAbstract shape).
    //
    // We only swap the import local name when the alias is "safe": a valid
    // identifier that won't collide with the wrap's own bindings (`_cjs`,
    // `module`, `exports`, `require`, `_req_*`) or with a hoisted class
    // name. The first alias for each spec wins; subsequent aliases of the
    // same spec keep their `const X = <chosen>;` form (handled below).
    let raw_aliases = extract_require_aliases_with_ranges(source);
    let alias_is_safe = |alias: &str| -> bool {
        if alias.starts_with("_req_") {
            return false;
        }
        if matches!(alias, "_cjs" | "module" | "exports" | "require") {
            return false;
        }
        if hoisted_class_names.iter().any(|c| c == alias) {
            return false;
        }
        true
    };
    let mut import_local_names: Vec<String> = require_specs
        .iter()
        .enumerate()
        .map(|(i, _)| format!("_req_{}", i))
        .collect();
    let mut chosen_alias_per_spec: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    for (alias, spec, _) in &raw_aliases {
        if !alias_is_safe(alias) {
            continue;
        }
        if import_local_names.iter().any(|n| n == alias) {
            continue;
        }
        let Some(idx) = require_specs.iter().position(|s| s == spec) else {
            continue;
        };
        if chosen_alias_per_spec.contains(spec) {
            continue;
        }
        import_local_names[idx] = alias.clone();
        chosen_alias_per_spec.insert(spec.clone());
    }

    // #1721: ranges of `const <alias> = require(<spec>)` lines whose alias we
    // ADOPTED as the import local name above (`import_local_names[idx] == alias`).
    // The synthetic `require` returns that name, and the hoisted `import <alias>`
    // already binds it at module scope — so the original body line would
    // *redeclare* `<alias>` inside the IIFE and shadow the import. Under
    // function scope the IIFE's `require` then returns the inner, not-yet-
    // initialized binding → the consumer's `const x = require('./m')` lands
    // `undefined`. We blank these body lines (below) so both the require-case
    // return and the body references resolve to the module-scope import via
    // closure. (Previously this blanking only happened when hoisting classes.)
    let adopted_alias_strip_ranges: Vec<(usize, usize)> = raw_aliases
        .iter()
        .filter(|(alias, spec, _)| {
            require_specs
                .iter()
                .position(|s| s == spec)
                .is_some_and(|idx| import_local_names[idx] == *alias)
        })
        .map(|(_, _, range)| *range)
        .collect();

    let imports = require_specs
        .iter()
        .zip(import_local_names.iter())
        .map(|(spec, local)| format!("import {} from '{}';", local, spec))
        .collect::<Vec<_>>()
        .join("\n");

    let require_cases = require_specs
        .iter()
        .zip(import_local_names.iter())
        .map(|(spec, local)| format!("        if (specifier === '{}') return {};", spec, local))
        .collect::<Vec<_>>()
        .join("\n");
    let require_resolve_cases = require_specs
        .iter()
        .map(|spec| format!("        if (specifier === '{}') return '{}';", spec, spec))
        .collect::<Vec<_>>()
        .join("\n");

    let mut named_exports = extract_exports_from_source(source);

    // For trivial re-export wrappers (`module.exports = require('./X')`),
    // recursively pull in the target's named exports. Without this,
    // react/index.js — which has zero `exports.X =` patterns of its own —
    // produces zero named exports and downstream `import { useState } from
    // "react"` link-fails.
    for spec in &require_specs {
        if !spec.starts_with("./") && !spec.starts_with("../") {
            continue;
        }
        let Some(target) = super::super::resolve::resolve_relative_import_path(spec, source_path)
        else {
            continue;
        };
        if let Ok(target_source) = std::fs::read_to_string(&target) {
            for name in extract_exports_from_source(&target_source) {
                if !named_exports.contains(&name) {
                    named_exports.push(name);
                }
            }
        }
    }

    // Issue #665: when the CJS body assigns `module.exports = <Ident>` and
    // `<Ident>` names a hoisted class, route the default export to the
    // hoisted class binding directly instead of through `_cjs`. The IIFE
    // still runs (side-effects and `exports.X = ...` keep their semantics),
    // but `import X from "pkg"` resolves to the hoisted class declaration
    // with all its methods on the prototype. Going through `_cjs` (whose
    // declaration is `const _cjs = (function(){...})()` and whose value
    // happens to be the class) loses class identity in HIR — instance
    // methods come back `undefined`. This is the `module.exports = Class`
    // + `extends` shape used by rate-limiter-flexible and most older
    // npm-published CJS classes.
    let default_export_identifier = extract_single_module_exports_assignment(source)
        .filter(|name| hoisted_class_names.contains(name));

    let direct_class_exports = if hoisted_class_names.is_empty() {
        String::new()
    } else {
        hoisted_class_names
            .iter()
            .map(|n| format!("export {{ {} }};", n))
            .collect::<Vec<_>>()
            .join("\n")
    };

    // Issue #665 follow-up: detect `(?:module\.)?exports\.X = require('Y')`
    // patterns and forward them as direct ESM re-exports of `Y`'s default
    // export. This preserves class identity through index-file aggregators
    // (the rate-limiter-flexible / older-npm shape: an index.js whose only
    // body is a series of `module.exports.RateLimiterMemory =
    // require('./lib/RateLimiterMemory')` lines).
    //
    // Pre-fix the consumer's `import { RateLimiterMemory } from "pkg"` resolved
    // to `export const RateLimiterMemory = _cjs.RateLimiterMemory;` — a
    // runtime property read on the IIFE result. HIR can't see through that
    // read to the class declaration in the required file, so `new
    // RateLimiterMemory(...)` produced an empty object with no methods.
    //
    // Emitting `export { _req_N as RateLimiterMemory };` makes the named
    // export an alias of the default import from `./lib/RateLimiterMemory`,
    // and the compile.rs class propagation (Export::Named arm at
    // compile.rs:2505) walks default-import specifiers and forwards the
    // source module's "default"-keyed class into this module's exported_classes
    // under the aliased name. Class identity survives the indirection.
    // Union of two named-reexport shapes:
    //   (a) `exports.X = require('Y')` direct-assignment (the v0.5.808 fix).
    //   (b) `const X = require('./Y'); module.exports = { X, ... }` object-literal
    //       aggregation — the published shape of `rate-limiter-flexible/index.js`
    //       and many older npm packages (#665 latest comment). The aggregator's
    //       entries are shorthand `{ X }` or longhand `{ X: Y }`; for shorthand
    //       the exported name and the alias name coincide, for longhand we look
    //       up the RHS as a require alias and emit the export under the
    //       property name.
    let mut named_reexport_requires = extract_named_exports_from_require(source);
    for (name, spec) in extract_object_literal_exports_from_require(source) {
        if !named_reexport_requires.iter().any(|(n, _)| *n == name) {
            named_reexport_requires.push((name, spec));
        }
    }
    let direct_named_reexports = if named_reexport_requires.is_empty() {
        String::new()
    } else {
        named_reexport_requires
            .iter()
            .filter_map(|(name, spec)| {
                require_specs
                    .iter()
                    .position(|s| s == spec)
                    .map(|n| format!("export {{ {} as {} }};", import_local_names[n], name))
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let named_reexport_names: Vec<String> = named_reexport_requires
        .iter()
        .map(|(n, _)| n.clone())
        .collect();

    let named_export_decls = if named_exports.is_empty() {
        String::new()
    } else {
        named_exports
            .iter()
            .filter(|n| !hoisted_class_names.contains(n))
            .filter(|n| !named_reexport_names.contains(n))
            .map(|n| format!("export const {} = _cjs.{};", n, n))
            .collect::<Vec<_>>()
            .join("\n")
    };

    // Refs #488 drizzle-sqlite: cross-file class inheritance bug.
    // The hoisted class block runs at module scope (so consumers can
    // `import { X } from "pkg"` and resolve to the real class), but the
    // class body's `extends import_foo.Bar` / `static [import_baz.key] = …`
    // references rely on the `var import_foo = require("./foo.cjs");`
    // bindings that the original CJS source declares INSIDE the IIFE.
    // Hoisting alone leaves `import_foo` undefined at the hoisted-class
    // location, so the runtime sees `extends undefined.Bar` and the
    // resulting class has no parent — every inherited method (drizzle's
    // `ColumnBuilder.setName`, etc.) reads `undefined` on instances.
    //
    // Fix: surface each `var import_X = require("Y")` as a module-scope
    // alias `const import_X = _req_N;` BEFORE the hoisted class block.
    // We ALSO blank the original `var import_X = require(...)` inside the
    // IIFE body so it doesn't shadow the module-scope alias when the IIFE
    // evaluates — perry's resolver hits the inner `var` first under
    // function scope and the hoisted class loses its parent again
    // otherwise. The IIFE body's existing `import_X.Y` references still
    // resolve via the outer `const import_X` through closure scope, so
    // non-hoisted code paths are unaffected.
    let (import_aliases, alias_strip_ranges) = if hoisted_class_block.is_empty() {
        // No hoisted classes: we don't need to surface module-scope `const
        // alias = _req_N;` lines (body references resolve to the imports via
        // closure), but we MUST still blank any adopted-alias `const alias =
        // require(spec)` lines so they don't shadow the hoisted import (#1721).
        (String::new(), adopted_alias_strip_ranges)
    } else {
        let aliases = extract_require_aliases_with_ranges(source);
        let lines = aliases
            .iter()
            .filter_map(|(alias, spec, _range)| {
                let idx = require_specs.iter().position(|s| s == spec)?;
                // When the alias is already the spec's import local name
                // (Issue #665 third pass: we renamed `_req_N` → alias upstream
                // so class-identity propagation works), the const would
                // redeclare the import — skip. Otherwise emit the const so
                // subsequent aliases of the same spec keep their binding.
                if import_local_names[idx] == *alias {
                    None
                } else {
                    Some(format!("const {} = {};", alias, import_local_names[idx]))
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let ranges = aliases
            .into_iter()
            .filter(|(_, spec, _)| require_specs.iter().any(|s| s == spec))
            .map(|(_, _, range)| range)
            .collect::<Vec<_>>();
        (lines, ranges)
    };

    // Start from the source (with hoisted classes already blanked when there
    // are any), then blank the `<kw> alias = require(...)` lines collected in
    // `alias_strip_ranges` so they don't shadow the module-scope import/alias
    // when the IIFE runs. Applies in both cases now: with classes it strips the
    // surfaced aliases (#665), without classes it strips adopted aliases (#1721).
    let body_for_iife = {
        let mut s = if hoisted_class_block.is_empty() {
            source.to_string()
        } else {
            source_without_hoists
        };
        for (start, end) in alias_strip_ranges.into_iter().rev() {
            let original = &source[start..end];
            let blanked: String = original
                .chars()
                .map(|c| if c == '\n' { '\n' } else { ' ' })
                .collect();
            s.replace_range(start..end, &blanked);
        }
        s
    };

    let default_export_decl = match &default_export_identifier {
        Some(name) => format!("export default {};", name),
        None => "export default _cjs;".to_string(),
    };

    let wrapped = format!(
        r#"{imports}
{import_aliases}
{hoisted_class_block}
const _cjs = (function() {{
    // #3527: `module`/`exports` are reassignable `var`s (mirroring Node, where
    // they are wrapper-function parameters), so CJS bodies that do
    // `var module = X` / `module = X` / `exports = X` — e.g. iconv-lite's
    // `for (...) {{ var module = modules[i]; mergeModules(exports, module); }}`
    // — rebind the local instead of colliding with a `const`. The stable
    // `__cjs_module` is what the module actually exports, read back at the end;
    // a body reassigning its local `module` can't clobber it (Node holds the
    // real module ref the same way), so named/default-export resolution stays
    // correct regardless of what the body does to its `module` local.
    const __cjs_module = {{ exports: {{}} }};
    var module = __cjs_module;
    var exports = __cjs_module.exports;
    function __perry_cjs_require_error(kind, code, message) {{
        const err = kind === 'type' ? new TypeError(message) : new Error(message);
        err.code = code;
        return err;
    }}
    function __perry_cjs_require_is_builtin(specifier) {{
        switch (specifier) {{
            case 'assert': case 'node:assert':
            case 'assert/strict': case 'node:assert/strict':
            case 'async_hooks': case 'node:async_hooks':
            case 'buffer': case 'node:buffer':
            case 'child_process': case 'node:child_process':
            case 'cluster': case 'node:cluster':
            case 'console': case 'node:console':
            case 'constants': case 'node:constants':
            case 'crypto': case 'node:crypto':
            case 'dns': case 'node:dns':
            case 'dns/promises': case 'node:dns/promises':
            case 'events': case 'node:events':
            case 'fs': case 'node:fs':
            case 'http': case 'node:http':
            case 'http2': case 'node:http2':
            case 'https': case 'node:https':
            case 'module': case 'node:module':
            case 'net': case 'node:net':
            case 'os': case 'node:os':
            case 'path': case 'node:path':
            case 'path/posix': case 'node:path/posix':
            case 'path/win32': case 'node:path/win32':
            case 'perf_hooks': case 'node:perf_hooks':
            case 'process': case 'node:process':
            case 'punycode': case 'node:punycode':
            case 'querystring': case 'node:querystring':
            case 'readline': case 'node:readline':
            case 'readline/promises': case 'node:readline/promises':
            case 'stream': case 'node:stream':
            case 'stream/promises': case 'node:stream/promises':
            case 'string_decoder': case 'node:string_decoder':
            case 'sys': case 'node:sys':
            case 'test': case 'node:test':
            case 'test/reporters': case 'node:test/reporters':
            case 'timers': case 'node:timers':
            case 'timers/promises': case 'node:timers/promises':
            case 'tty': case 'node:tty':
            case 'url': case 'node:url':
            case 'util': case 'node:util':
            case 'util/types': case 'node:util/types':
            case 'worker_threads': case 'node:worker_threads':
            case 'zlib': case 'node:zlib':
                return true;
            default:
                return false;
        }}
    }}
    function require(specifier) {{
        if (typeof specifier !== 'string') throw __perry_cjs_require_error('type', 'ERR_INVALID_ARG_TYPE', 'The "id" argument must be of type string.');
        if (specifier === '') throw __perry_cjs_require_error('type', 'ERR_INVALID_ARG_VALUE', 'The argument "id" must be a non-empty string.');
{require_cases}
        throw new Error('require() is not supported: ' + specifier);
    }}
    Object.defineProperty(require, 'name', {{
        value: 'require',
        writable: false,
        enumerable: false,
        configurable: true,
    }});
    require.resolve = function resolve(specifier, options) {{
        if (typeof specifier !== 'string') throw __perry_cjs_require_error('type', 'ERR_INVALID_ARG_TYPE', 'The "request" argument must be of type string.');
{require_resolve_cases}
        if (__perry_cjs_require_is_builtin(specifier)) return specifier;
        throw __perry_cjs_require_error('error', 'MODULE_NOT_FOUND', 'Cannot find module ' + specifier);
    }};
    require.resolve.paths = function paths(specifier) {{
        if (typeof specifier !== 'string') throw __perry_cjs_require_error('type', 'ERR_INVALID_ARG_TYPE', 'The "request" argument must be of type string.');
        return null;
    }};
    require.cache = {{}};
    require.extensions = {{
        '.js': function(module, filename) {{}},
        '.json': function(module, filename) {{}},
        '.node': function(module, filename) {{}},
    }};
    require.main = module;

    {body_for_iife}

    return __cjs_module.exports;
}})();

{default_export_decl}
{direct_class_exports}
{direct_named_reexports}
{named_export_decls}
"#
    );
    if std::env::var("PERRY_DEBUG_CJS_WRAP").is_ok() {
        eprintln!(
            "=== CJS WRAP for {} ===\n{}\n=== END ===",
            source_path.display(),
            wrapped
        );
    }
    wrapped
}
