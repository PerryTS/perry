//! Top-level CJS-to-ESM wrap orchestration: hoist requires/classes and
//! assemble the IIFE-shaped module.

use super::*;
use std::borrow::Cow;
use std::path::Path;

/// Is `name` a JS global-builtin VALUE (a constructor/namespace reachable as a
/// bare identifier at runtime)? Used only to decide whether a CJS named export
/// whose KEY equals such a name (`module.exports = { Error: Error }`) needs the
/// mangled-rebinding emission so a module-scope `export const <name>` doesn't
/// shadow the global for free references in the IIFE body. Deliberately limited
/// to runtime VALUES (not TS-only utility types): an over-broad set would only
/// route an unrelated export through the (correct) mangled re-export.
fn is_global_value_builtin_name(name: &str) -> bool {
    matches!(
        name,
        "Error"
            | "TypeError"
            | "RangeError"
            | "SyntaxError"
            | "ReferenceError"
            | "EvalError"
            | "URIError"
            | "AggregateError"
            | "SuppressedError"
            | "Object"
            | "Function"
            | "Array"
            | "Boolean"
            | "Number"
            | "String"
            | "Symbol"
            | "BigInt"
            | "Math"
            | "JSON"
            | "Date"
            | "RegExp"
            | "Promise"
            | "Proxy"
            | "Reflect"
            | "Map"
            | "Set"
            | "WeakMap"
            | "WeakSet"
            | "WeakRef"
            | "ArrayBuffer"
            | "SharedArrayBuffer"
            | "DataView"
            | "Buffer"
            | "Uint8Array"
            | "Uint8ClampedArray"
            | "Int8Array"
            | "Int16Array"
            | "Uint16Array"
            | "Int32Array"
            | "Uint32Array"
            | "Float32Array"
            | "Float64Array"
            | "BigInt64Array"
            | "BigUint64Array"
    )
}

/// Wrap CJS source as ESM. `source_path` is the absolute path of the file
/// being wrapped — used to resolve `require('./relative')` targets when
/// peeking at re-export wrappers' transitive named exports.
#[cfg(test)]
pub(in crate::commands::compile) fn wrap_commonjs(source: &str, source_path: &Path) -> String {
    wrap_commonjs_for_target(source, source_path, None)
}

pub(in crate::commands::compile) fn wrap_commonjs_for_target(
    source: &str,
    source_path: &Path,
    target: Option<&str>,
) -> String {
    wrap_commonjs_with_body_offset(source, source_path, target).0
}

/// Like [`wrap_commonjs_for_target`], but also returns the byte offset within
/// the returned wrapped source at which the ORIGINAL module body begins (i.e.
/// the length of the injected wrapper prefix: imports + aliases + hoisted
/// classes + the IIFE/preamble scaffolding). `--debug-symbols` uses this to map
/// a wrapped-coordinate `byte_offset` back to original-source coordinates.
/// `None` when the body could not be located in the wrapped output (a
/// special-case early rewrite changed it); callers then skip the mapping.
pub(in crate::commands::compile) fn wrap_commonjs_with_body_offset(
    source: &str,
    source_path: &Path,
    target: Option<&str>,
) -> (String, Option<usize>) {
    let mut source_cow = Cow::Borrowed(source);

    // Issue #5498: a genuine CommonJS file may carry a leading shebang
    // (`#!/usr/bin/env node`) — common for CLI entry points. The wrap splices
    // the source into the MIDDLE of the wrapper template, so a `#!` left intact
    // is no longer at byte 0 and SWC rejects it (a shebang is only a valid
    // token as the file's first bytes). Neutralize it into a line comment in
    // place: `#!` and `//` are both two bytes, so every downstream byte offset
    // — including the #5247 body-offset mapping — is preserved exactly.
    if source_cow.starts_with("#!") {
        let mut owned = source_cow.into_owned();
        owned.replace_range(0..2, "//");
        source_cow = Cow::Owned(owned);
    }

    if is_depd_index_path(source_path) {
        if let Some(rewritten) = rewrite_depd_dynamic_wrapper(source_cow.as_ref()) {
            source_cow = Cow::Owned(rewritten);
        }
    }
    if is_function_bind_implementation_path(source_path) {
        if let Some(rewritten) = rewrite_function_bind_dynamic_wrapper(source_cow.as_ref()) {
            source_cow = Cow::Owned(rewritten);
        }
    }
    if is_safer_buffer_path(source_path) {
        if let Some(rewritten) = rewrite_safer_buffer_private_binding(source_cow.as_ref()) {
            source_cow = Cow::Owned(rewritten);
        }
    }
    if is_safe_buffer_path(source_path) {
        if let Some(rewritten) = rewrite_safe_buffer_slow_buffer_fallback(source_cow.as_ref()) {
            source_cow = Cow::Owned(rewritten);
        }
    }

    // Issue #665 (fifth pass): rewrite `module.exports = class X { ... };`
    // expressions into declaration form + bare-identifier assignment so the
    // existing hoist + direct-default-export machinery surfaces the class.
    // Without this, the leaf `module.exports = class Abstract { ... };` shape
    // (rate-limiter-flexible/lib/RateLimiterAbstract.js) leaves `_cjs` as the
    // module's default — opaque to compile.rs's class-identity tracking, so
    // a downstream `class Memory extends RateLimiterAbstract { constructor(o){
    // super(o); ... } }` silently no-ops the parent constructor. The fix
    // mirrors the declaration-form path that v0.5.839 already wired up.
    if let Some(rewritten) = rewrite_module_exports_class_expression(source_cow.as_ref()) {
        source_cow = Cow::Owned(rewritten);
    }
    let source: &str = source_cow.as_ref();

    let mut require_specs = extract_require_specifiers(source);
    let dead_platform_requires = inactive_platform_guarded_requires(source, target);
    if !dead_platform_requires.is_empty() {
        require_specs.retain(|spec| !dead_platform_requires.contains(spec));
    }

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

    // Names this module will declare at MODULE scope as `export const X =
    // _cjs.X;` (the `named_export_decls` set computed below). Adopting an
    // alias whose name matches one of these is a self-collision: the wrap
    // would emit BOTH `import X from '<spec>';` (the adopted alias import) and
    // `export const X = _cjs.X;` — two module-scope bindings named `X`. HIR's
    // resolver then binds the IIFE-body reference `X` (e.g. `const { ... } =
    // X`) to the `export const`, whose value `_cjs.X` is `undefined` until the
    // IIFE returns. This is the pino `const symbols = require('./lib/symbols')`
    // + `module.exports.symbols = symbols` shape: the top-level
    // `const { ...30 syms... } = symbols` destructure read `undefined` and
    // threw `Cannot convert undefined or null to object` (pino.js:23).
    //
    // Refusing adoption keeps the spec on `_req_N`, so the original body line
    // `const X = require('<spec>')` is NOT blanked, runs inside the IIFE, and
    // binds an IIFE-LOCAL `X = _req_N` — distinct from the module-scope
    // `export const X`. The body's `X` references resolve to the local; the
    // export still surfaces the value. This mirrors how a destructured
    // `const { ... } = require('<spec>')` spec (levels/constants/tools) already
    // keeps `_req_N` and works. The collision set deliberately excludes
    // re-export-via-require names (those become `export { _req_N as X };`, no
    // module-scope `const X`) and hoisted-class names (already blocked below),
    // so class-identity adoption (#665) is unaffected.
    let export_const_collision_names: std::collections::HashSet<String> = {
        let plain_exports = extract_exports_from_source(source);
        let mut reexport_names: std::collections::HashSet<String> =
            extract_named_exports_from_require(source)
                .into_iter()
                .map(|(n, _)| n)
                .collect();
        for (name, _) in extract_object_literal_exports_from_require(source) {
            reexport_names.insert(name);
        }
        plain_exports
            .into_iter()
            .filter(|n| !reexport_names.contains(n))
            .filter(|n| !hoisted_class_names.contains(n))
            .collect()
    };

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
        // Self-collision with a module-scope `export const <alias> = _cjs.<alias>;`
        // (see `export_const_collision_names` above) — keep the spec on `_req_N`.
        if export_const_collision_names.contains(alias) {
            return false;
        }
        // #5006: a reassigned alias (`s = s.filter(...)`) must stay a real
        // mutable local — adopting it into an immutable `import s from '...'`
        // and blanking the declaration makes the reassignment unresolvable
        // (`ReferenceError: s is not defined`, the signal-exit → ink wall).
        if identifier_is_reassigned(source, alias) {
            return false;
        }
        true
    };
    // Next.js lazy-require: specifiers whose every `require('S')` call site is
    // inside a function body (lazy in Node). Computed up front because it also
    // suppresses alias ADOPTION below — a function-local `const dep =
    // require('S')` is a function-scoped const, not a module binding, and
    // adopting it would hoist `import dep from 'S'` to module scope (eager). We
    // instead keep the synthetic binding and rename it `_lazyreq_N` so the
    // target stays `Deferred` and inits only when the shim's
    // `return _lazyreq_N` runs (i.e. when the function actually calls require).
    let lazy_specs = function_local_specs(source);

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
        if lazy_specs.contains(spec) {
            // Don't adopt a function-local alias — keep it lazy (see above).
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

    // Rename the surviving synthetic bindings for function-local specs so
    // `collect_modules` can tag the import `is_deferred_require` by name and
    // codegen can fire `<S>__init()` at the shim read site.
    if !lazy_specs.is_empty() {
        for (i, spec) in require_specs.iter().enumerate() {
            if import_local_names[i] == format!("_req_{i}") && lazy_specs.contains(spec) {
                import_local_names[i] = format!("_lazyreq_{i}");
            }
        }
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
        .map(|(spec, local)| {
            // #4904: Node's underscore-prefixed internal http modules are
            // require-only re-exports of the public `http` surface
            // (`require('_http_agent').Agent` etc.). Bind the hoisted import
            // to the public module; the require shim still matches on the
            // original specifier string.
            let import_spec = match spec.as_str() {
                "_http_agent" | "_http_client" | "_http_incoming" | "_http_outgoing"
                | "_http_server" => "http",
                other => other,
            };
            format!("import {} from '{}';", local, import_spec)
        })
        .collect::<Vec<_>>()
        .join("\n");

    // An UNRESOLVABLE adopted specifier (`require('@opentelemetry/api')`
    // with only Next's vendored copy on disk) leaves its hoisted import
    // binding as the boolean TRUE sentinel at runtime. Returning that from
    // the shim defeats the ubiquitous try/require-fallback pattern — Node
    // throws MODULE_NOT_FOUND and the catch loads the vendored copy, but
    // the shim handed back `true` and the catch never ran. Guard such an
    // entry with a throw — but ONLY when a call site of that specifier
    // sits inside a `try` block: a BARE top-level require of a pruned
    // build-only module (`require('next/dist/compiled/browserslist')` in
    // get-supported-browsers.js) must keep the silent sentinel, because
    // Perry initializes every collected module eagerly while Node never
    // loads that file at all — a throw there kills startup. (A real module
    // default-exporting a boolean would mis-trip the guard; no such
    // package shape has been observed.)
    let require_cases = require_specs
        .iter()
        .zip(import_local_names.iter())
        .map(|(spec, local)| {
            if require_site_in_try(source, spec) {
                format!(
                    "        if (specifier === '{spec}') {{ if (typeof {local} === 'boolean') \
                     throw __perry_cjs_require_error('error', 'MODULE_NOT_FOUND', \
                     \"Cannot find module '{spec}'\"); return {local}; }}"
                )
            } else {
                format!("        if (specifier === '{}') return {};", spec, local)
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    // Heuristic: is any `require('<spec>')` call site lexically inside a
    // `try { … }` block? Reverse brace-depth scan from the call offset to
    // the nearest unmatched `{`, checking whether `try` precedes it.
    // String/comment contexts are not stripped — a false positive only
    // turns the silent sentinel into a (more Node-faithful) throw.
    fn require_site_in_try(source: &str, spec: &str) -> bool {
        let needle_sq = format!("require('{}')", spec);
        let needle_dq = format!("require(\"{}\")", spec);
        let bytes = source.as_bytes();
        let mut search = 0usize;
        loop {
            let hit = source[search..]
                .find(&needle_sq)
                .or_else(|| source[search..].find(&needle_dq));
            let Some(rel) = hit else { return false };
            let at = search + rel;
            // Walk backwards to the nearest unmatched `{`, repeatedly: each
            // enclosing block is checked for a preceding `try`.
            let mut depth = 0i32;
            let mut i = at;
            while i > 0 {
                i -= 1;
                match bytes[i] {
                    b'}' => depth += 1,
                    b'{' => {
                        if depth > 0 {
                            depth -= 1;
                        } else {
                            // Enclosing block opener — does `try` precede it?
                            let mut j = i;
                            while j > 0
                                && (bytes[j - 1] == b' '
                                    || bytes[j - 1] == b'\t'
                                    || bytes[j - 1] == b'\r'
                                    || bytes[j - 1] == b'\n')
                            {
                                j -= 1;
                            }
                            if j >= 3
                                && &bytes[j - 3..j] == b"try"
                                && (j == 3 || !bytes[j - 4].is_ascii_alphanumeric())
                            {
                                return true;
                            }
                            // Keep walking outward (this block wasn't a try).
                        }
                    }
                    _ => {}
                }
            }
            search = at + 1;
        }
    }

    let require_resolve_cases = require_specs
        .iter()
        .map(|spec| format!("        if (specifier === '{}') return '{}';", spec, spec))
        .collect::<Vec<_>>()
        .join("\n");

    let mut named_exports = extract_exports_from_source(source);

    // Issue #4872: `__exportStar(require('X'), exports)` is tsc's CJS
    // lowering of `export * from 'X'` — emit exactly that as a real ESM
    // re-export at module scope. The static `export *` lets compile.rs's
    // transitive re-export propagation resolve names through multi-level
    // barrels to their defining module (nestjs's `@nestjs/common/index.js`
    // → `decorators/index.js` → `core/index.js` → `controller.decorator.js`),
    // so a consumer's `import { Controller } from '@nestjs/common'` binds
    // the origin's symbol instead of link-failing on
    // `perry_fn_<common_index_js>__Controller`. The runtime copy inside the
    // IIFE still runs, so `_cjs.X` property reads keep working too.
    let export_star_specs = extract_export_star_specs(source);

    // For trivial re-export wrappers (`module.exports = require('./X')`),
    // recursively pull in the target's named exports. Without this,
    // react/index.js — which has zero `exports.X =` patterns of its own —
    // produces zero named exports and downstream `import { useState } from
    // "react"` link-fails.
    //
    // CRUCIAL: only specs THIS module actually re-exports
    // (`module.exports = require('SPEC')`) qualify. A module that merely
    // `require()`s a sibling for its own internal use — e.g. semver's
    // `classes/comparator.js` doing `const { safeRe: re, t } =
    // require('../internal/re')` and then defining a class that reads
    // `re[t.COMPARATOR]` — is NOT a re-export wrapper of `../internal/re`.
    // Forwarding the target's names here emitted spurious module-scope
    // `export const t = _cjs.t;` (and `re`, `src`, `safeRe`) declarations
    // that (a) shadowed the module's own same-named bindings and (b)
    // resolved to `undefined` because those names are not on THIS module's
    // `exports` — the `Cannot read properties of undefined (reading
    // 'COMPARATOR')` root for semver/pino/bluebird.
    let reexport_specs = module_reexport_specs(source);
    for spec in &require_specs {
        if !spec.starts_with("./") && !spec.starts_with("../") {
            continue;
        }
        // Only forward exports of specs this module genuinely re-exports.
        if !reexport_specs.iter().any(|s| s == spec) {
            continue;
        }
        // #4872: specs re-exported via `__exportStar` surface through the
        // static `export * from` emitted below — resolving to the ORIGIN
        // module's symbols. Pulling the target's textual exports here would
        // emit explicit `export const X = _cjs.X;` bindings that shadow the
        // star re-export (ESM precedence) and degrade those names back to
        // runtime property reads.
        if export_star_specs.contains(spec) {
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

    // A named export whose name is a JS global-builtin VALUE (`Error`,
    // `TypeError`, `Object`, `Promise`, …) and is NOT declared as a module
    // binding in the body is the `module.exports = { Error: Error }` shape: the
    // KEY collides with a global the IIFE body references freely. Emitting
    // `export const Error = _cjs.Error;` puts a module-scope `Error` binding
    // ahead of the global, so the body's `Error` (e.g. bluebird errors.js
    // `inherits(SubError, Error)`) resolves to the `export const`, whose value
    // `_cjs.Error` is `undefined` until the IIFE returns — `Parent.prototype`
    // then threw `Cannot read properties of undefined (reading 'prototype')`.
    // For these, surface the export through a MANGLED module-scope binding and
    // re-export it under the original name (`const __cjsexp_Error = _cjs.Error;
    // export { __cjsexp_Error as Error };`). The value still surfaces for named
    // imports, but no `Error` binding shadows the global in the body.
    let builtin_value_global_collision = |n: &str| -> bool {
        is_global_value_builtin_name(n) && !identifier_is_declared_binding(source, n)
    };
    let named_export_decls = if named_exports.is_empty() {
        String::new()
    } else {
        named_exports
            .iter()
            .filter(|n| !hoisted_class_names.contains(n))
            .filter(|n| !named_reexport_names.contains(n))
            .map(|n| {
                if builtin_value_global_collision(n) {
                    format!(
                        "const __cjsexp_{n} = _cjs.{n};\nexport {{ __cjsexp_{n} as {n} }};",
                        n = n
                    )
                } else {
                    format!("export const {} = _cjs.{};", n, n)
                }
            })
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
            // #5006: a reassigned alias must keep its mutable `var alias =
            // require(...)` local in the IIFE body — never surface it as an
            // immutable module-scope `const alias = _req_N;` (the const write
            // would throw) nor strip its declaration below.
            .filter(|(alias, _, _)| !identifier_is_reassigned(source, alias))
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
            .filter(|(alias, _, _)| !identifier_is_reassigned(source, alias))
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

    // Issue #4933 — flat-emit a `module.exports = <Class>` module that we
    // could NOT hoist. The hoist refuses any class whose body references a
    // top-level `const`/`let`/`var` (#2310 — moving the class out of the
    // IIFE would sever its closure over that binding). For a default-export
    // class this is fatal: with the class trapped inside the IIFE, the
    // module's default becomes the opaque `_cjs` result, so compile.rs never
    // registers class identity. The consumer's `import StackUtils` then gets
    // a value whose static methods, `.prototype`, AND closure are all gone
    // (`StackUtils.nodeInternals` / `.prototype.clean` read `undefined`).
    //
    // The IIFE exists only to give the body a function scope (so a CJS
    // top-level `return` is legal). When the body has no top-level `return`
    // we can drop the IIFE entirely and run the body at ESM module scope:
    // the class becomes a real top-level declaration (`export default
    // StackUtils` resolves to it with full identity), every sibling binding
    // it closes over stays in scope, and statement order is preserved
    // verbatim. We only take this path for the case that is *currently
    // broken* (a top-level class that is the single `module.exports = X`
    // target but did not hoist), so working packages are unaffected.
    let flat_default_class = extract_single_module_exports_assignment(source).filter(|name| {
        !hoisted_class_names.contains(name)
            && top_level_class_names(source).iter().any(|c| c == name)
            && !source_has_top_level_return(source)
    });

    // #4872: ESM `export * from` declarations for every `__exportStar`
    // call detected above.
    let export_star_decls = export_star_specs
        .iter()
        .map(|spec| format!("export * from '{}';", spec))
        .collect::<Vec<_>>()
        .join("\n");

    // #3527 / #4933: the CommonJS runtime preamble (`module` / `exports` /
    // `require` shims). Built once and shared by the IIFE wrap and the flat
    // (#4933) emission so the two paths can never drift. The 4-space indent is
    // written for the in-IIFE position; at module scope (flat) it is purely
    // cosmetic. Embedding `{cjs_preamble}` reproduces the historical IIFE text
    // byte-for-byte.
    // Debug-quoted absolute dir of this module, the starting point for the
    // require.resolve node_modules subpath fallback.
    let module_dir_literal = format!(
        "{:?}",
        source_path
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default()
    );
    let cjs_preamble = format!(
        r#"    // #3527: `module`/`exports` are reassignable `var`s (mirroring Node, where
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
        // Runtime `require(absolutePath.js)` of a module Perry AOT-compiled but
        // that is only reachable via a runtime-computed path (Next.js / turbopack
        // load page + chunk modules by a manifest path at request time, not a
        // static specifier). Resolve it from the path->module registry that each
        // compiled module self-registers into at init; `undefined` = not
        // registered, fall through to the `.json` read / MODULE_NOT_FOUND throw.
        {{
            const __perry_path_mod = __perry_require_path_module(specifier);
            if (__perry_path_mod !== undefined) return __perry_path_mod;
        }}
        // Runtime `require(absolutePath)` of a `.json` file (Next.js loads
        // manifests this way: `require(this.middlewareManifestPath)`). Node's
        // require reads + JSON.parses `.json` files; the statically-resolved
        // cases above only cover specifiers known at compile time, so a path
        // computed at runtime falls here. `.json` is pure data (no eval), so we
        // read it from disk and parse it. `.js`/`.node` runtime require stays
        // unsupported — that would require evaluating arbitrary code.
        if ((specifier.charCodeAt(0) === 47 || (specifier.length > 2 && specifier.charCodeAt(1) === 58)) && specifier.slice(-5) === '.json') {{
            return __perry_require_json_disk(specifier);
        }}
        throw __perry_cjs_require_error('error', 'MODULE_NOT_FOUND', "Cannot find module '" + specifier + "'");
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
        var __perry_nm_resolved = __perry_require_resolve_node_modules({module_dir_literal}, specifier);
        if (__perry_nm_resolved !== undefined) return __perry_nm_resolved;
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
    require.main = module;"#
    );

    // Wall 54: self-register this compiled module's exports under its absolute
    // source path so a runtime `require(absolutePath.js)` (turbopack/Next.js
    // page+chunk loading) resolves to it. `{:?}` debug-quotes to a valid JS
    // string literal.
    let path_register = format!(
        "__perry_register_path_module({:?}, __cjs_module.exports);",
        source_path.to_string_lossy()
    );
    let wrapped = if let Some(flat_class) = &flat_default_class {
        // Issue #4933 — flat emission. Drop the IIFE and run the CommonJS body
        // at ESM module scope: `module.exports = {flat_class}` then resolves to
        // a real top-level `class {flat_class}` declaration, so the consumer's
        // default import keeps full class identity (statics, `.prototype`, and
        // the closure over sibling top-level bindings). `{hoisted_class_block}`
        // still carries any sibling classes we DID hoist; `{flat_class}` itself
        // was refused a hoist (it closes over an IIFE-local), so it stays in
        // `{body_for_iife}` and lands at module scope here unchanged.
        format!(
            r#"{imports}
{import_aliases}
{hoisted_class_block}
{cjs_preamble}

{body_for_iife}

const _cjs = __cjs_module.exports;
{path_register}
export default {flat_class};
export {{ {flat_class} }};
{direct_class_exports}
{direct_named_reexports}
{named_export_decls}
{export_star_decls}
"#
        )
    } else {
        format!(
            r#"{imports}
{import_aliases}
{hoisted_class_block}
const _cjs = (function() {{
{cjs_preamble}

    {body_for_iife}

    {path_register}
    return __cjs_module.exports;
}})();

{default_export_decl}
{direct_class_exports}
{direct_named_reexports}
{named_export_decls}
{export_star_decls}
"#
        )
    };
    if std::env::var("PERRY_DEBUG_CJS_WRAP").is_ok() {
        eprintln!(
            "=== CJS WRAP for {} ===\n{}\n=== END ===",
            source_path.display(),
            wrapped
        );
    }
    // #5247: locate the original body within the wrapped output so callers can
    // translate a wrapped-coordinate byte offset back to original coordinates.
    // `body_for_iife` is interpolated verbatim into `wrapped`, so the first
    // occurrence is its start. Empty body → no mapping.
    let body_offset = if body_for_iife.is_empty() {
        None
    } else {
        wrapped.find(body_for_iife.as_str())
    };
    (wrapped, body_offset)
}

fn target_node_platform(target: Option<&str>) -> Option<&'static str> {
    match target {
        Some("windows") | Some("windows-winui") => Some("win32"),
        Some("linux") | Some("linux-x86_64") | Some("linux-arm64") | Some("linux-aarch64")
        // musl shares node's `process.platform === "linux"` (#4826).
        | Some("linux-musl") | Some("linux-x86_64-musl") | Some("linux-aarch64-musl") => {
            Some("linux")
        }
        Some("macos")
        | Some("ios")
        | Some("ios-simulator")
        | Some("ios-widget")
        | Some("ios-widget-simulator")
        | Some("visionos")
        | Some("visionos-simulator")
        | Some("watchos")
        | Some("watchos-simulator")
        | Some("watchos-widget")
        | Some("watchos-widget-simulator")
        | Some("tvos")
        | Some("tvos-simulator") => Some("darwin"),
        Some(_) => None,
        None => {
            #[cfg(target_os = "windows")]
            {
                Some("win32")
            }
            #[cfg(target_os = "linux")]
            {
                Some("linux")
            }
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            {
                Some("darwin")
            }
            #[cfg(not(any(
                target_os = "windows",
                target_os = "linux",
                target_os = "macos",
                target_os = "ios"
            )))]
            {
                None
            }
        }
    }
}

fn inactive_platform_guarded_requires(
    source: &str,
    target: Option<&str>,
) -> std::collections::HashSet<String> {
    let Some(platform) = target_node_platform(target) else {
        return std::collections::HashSet::new();
    };
    let re = regex::Regex::new(
        r#"(?s)if\s*\(\s*process\.platform\s*(===|!==)\s*['"]([^'"]+)['"]\s*\)\s*\{(?P<then>.*?)\}\s*else\s*\{(?P<else>.*?)\}"#,
    )
    .unwrap();
    let mut inactive = std::collections::HashSet::new();
    for cap in re.captures_iter(source) {
        let Some(op) = cap.get(1).map(|m| m.as_str()) else {
            continue;
        };
        let Some(expected) = cap.get(2).map(|m| m.as_str()) else {
            continue;
        };
        let condition_true = match op {
            "===" => platform == expected,
            "!==" => platform != expected,
            _ => continue,
        };
        let dead_body = if condition_true {
            cap.name("else")
        } else {
            cap.name("then")
        };
        if let Some(body) = dead_body {
            inactive.extend(extract_require_specifiers(body.as_str()));
        }
    }
    inactive
}

fn is_depd_index_path(source_path: &Path) -> bool {
    source_path
        .file_name()
        .map(|name| name == "index.js")
        .unwrap_or(false)
        && source_path
            .components()
            .any(|component| component.as_os_str().to_string_lossy() == "depd")
}

fn is_function_bind_implementation_path(source_path: &Path) -> bool {
    source_path
        .file_name()
        .map(|name| name == "implementation.js")
        .unwrap_or(false)
        && source_path
            .components()
            .any(|component| component.as_os_str().to_string_lossy() == "function-bind")
}

fn is_safer_buffer_path(source_path: &Path) -> bool {
    source_path
        .file_name()
        .map(|name| name == "safer.js")
        .unwrap_or(false)
        && source_path
            .components()
            .any(|component| component.as_os_str().to_string_lossy() == "safer-buffer")
}

fn is_safe_buffer_path(source_path: &Path) -> bool {
    source_path
        .file_name()
        .map(|name| name == "index.js")
        .unwrap_or(false)
        && source_path
            .components()
            .any(|component| component.as_os_str().to_string_lossy() == "safe-buffer")
}

fn rewrite_depd_dynamic_wrapper(source: &str) -> Option<String> {
    let needle = r#"  // eslint-disable-next-line no-new-func
  var deprecatedfn = new Function('fn', 'log', 'deprecate', 'message', 'site',
    '"use strict"\n' +
    'return function (' + args + ') {' +
    'log.call(deprecate, message, site)\n' +
    'return fn.apply(this, arguments)\n' +
    '}')(fn, log, this, message, site)"#;

    let replacement = r#"  var deprecatedfn = (function (fn, log, deprecate, message, site) {
    "use strict"
    return function () {
      log.call(deprecate, message, site)
      return fn.apply(this, arguments)
    }
  })(fn, log, this, message, site)"#;

    if source.contains(needle) {
        Some(source.replace(needle, replacement))
    } else {
        None
    }
}

fn rewrite_function_bind_dynamic_wrapper(source: &str) -> Option<String> {
    let needle = r#"    bound = Function('binder', 'return function (' + joiny(boundArgs, ',') + '){ return binder.apply(this,arguments); }')(binder);"#;
    let replacement = r#"    bound = function () {
        return binder.apply(this, arguments);
    };"#;

    if source.contains(needle) {
        Some(source.replace(needle, replacement))
    } else {
        None
    }
}

fn rewrite_safer_buffer_private_binding(source: &str) -> Option<String> {
    let needle = r#"if (!safer.kStringMaxLength) {
  try {
    safer.kStringMaxLength = process.binding('buffer').kStringMaxLength
  } catch (e) {
    // we can't determine kStringMaxLength in environments where process.binding
    // is unsupported, so let's not set it
  }
}"#;

    let replacement = r#"if (!safer.kStringMaxLength) {
  safer.kStringMaxLength = 536870888
}"#;

    if source.contains(needle) {
        Some(source.replace(needle, replacement))
    } else {
        None
    }
}

fn rewrite_safe_buffer_slow_buffer_fallback(source: &str) -> Option<String> {
    let needle = "return buffer.SlowBuffer(size)";
    let replacement = "return Buffer.allocUnsafeSlow(size)";

    if source.contains(needle) {
        Some(source.replace(needle, replacement))
    } else {
        None
    }
}
