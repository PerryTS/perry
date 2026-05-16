//! CommonJS-to-ESM source-level transformation for `compilePackages`.
//!
//! Closes the React-class blocker for issue #348 (ink-as-compilePackages).
//!
//! React 18 ships as CommonJS — `node_modules/react/index.js` does
//! `module.exports = require('./cjs/react.production.min.js')`, and the
//! actual implementation file uses `exports.useState = function() {...}`
//! patterns. Perry's native pipeline is ESM-only — `module`/`require` lower
//! to bare-identifier-zero, so the entire react module compiles to a no-op
//! and every downstream `import { useState } from "react"` link-fails with
//! `Undefined symbols: _perry_fn_node_modules_react_index_js__useState`.
//!
//! This module detects CJS at module-read time and rewrites the source to
//! ESM-shaped code before SWC parses it. The wrap pattern (modeled after
//! `perry-jsruntime/src/modules.rs:481` which already does this for the V8
//! fallback) is:
//!
//!   1. Hoist every `require('X')` call as `import _req_N from 'X';`.
//!   2. Wrap the CJS body in an IIFE that defines `module = { exports: {} }`,
//!      a synchronous `require(specifier)` that dispatches to the hoisted
//!      `_req_N` bindings, runs the original code, and returns
//!      `module.exports`. The IIFE result is bound to `_cjs`.
//!   3. Emit `export default _cjs;` plus `export const X = _cjs.X;` for each
//!      detected named export.
//!
//! Two named-export sources are unioned:
//!
//!   - `exports.X = ...` patterns *in this file* (regex; the existing
//!     jsruntime heuristic).
//!   - For "trivial re-export wrappers" (`module.exports = require('./X')`,
//!     optionally inside a `process.env.NODE_ENV` conditional), the
//!     `exports.X = ...` patterns of the recursively-required *target* file.
//!     Without this, react/index.js — whose only meaningful statements are
//!     two conditional `module.exports = require(...)` calls — produces zero
//!     named exports of its own and the link still fails. The recursion
//!     follows up to a small depth (2 levels) to handle one level of env
//!     switching; deeper indirection is rare and gets the no-op fallback.

use std::path::Path;

/// Heuristic CJS detection. Same shape as
/// `perry-jsruntime/src/modules.rs::is_commonjs`. False negatives are
/// acceptable (the file just falls through to the existing ESM-only
/// pipeline); false positives on a real ESM file would be more painful but
/// require a file that uses neither `module.exports` nor `exports.` nor
/// `require(` — i.e., an ESM file that *also* contains those tokens. Real
/// hybrid cases are rare and would need a `"type": "module"` package.json
/// override, which is the next refinement if this trips a real package.
///
/// Issue #851: Rollup-bundled output (the `vitest/dist/chunks/*.js` shape)
/// has top-level ESM `import`/`export` statements AND inlined CJS bodies
/// (`module.exports = factory()`) deep inside nested IIFE helpers. Such
/// files are unambiguously ESM — the inner CJS tokens are just identifiers
/// inside function bodies. If we wrap them as CJS, the wrap moves the
/// top-level `import`/`export` *inside* the IIFE body and SWC errors with
/// `ImportExportInScript`. The guard below short-circuits the wrap when a
/// top-level `import`/`export` statement is detected.
pub(super) fn is_commonjs(source: &str) -> bool {
    // ESM-at-the-top wins: a top-level `import`/`export` makes this an
    // ES module regardless of CJS patterns appearing deeper in the file.
    if has_top_level_esm(source) {
        return false;
    }
    source.contains("module.exports")
        || source.contains("exports.")
        || (source.contains("require(") && !source.contains("import "))
}

/// Returns true if `source` contains an unindented `import ` / `import{` /
/// `import"` / `import'` / `export ` / `export{` / `export*` / `export"` /
/// `export'` / `export=` (TS) statement on any line — a strong signal that
/// this file is an ES module regardless of any `module.exports`-style
/// content deeper in nested function bodies. Lines starting with leading
/// whitespace are treated as nested and ignored, because `import` /
/// `export` statements MUST be at module-top-level in ECMAScript. Comment
/// and string-literal contexts are not stripped — a `// import ` line is
/// already excluded by the leading-whitespace filter when indented; an
/// inline `/* import x */` followed by a real statement still triggers a
/// match on the real statement line. Worst case is a false positive on a
/// pathological file where the only top-level `import`/`export` lives
/// inside a multi-line string literal at column 0; we accept that risk
/// since the alternative is `ImportExportInScript` on real Rollup output.
fn has_top_level_esm(source: &str) -> bool {
    for raw_line in source.lines() {
        // Skip indented lines — `import`/`export` statements are only
        // valid at module top-level, so any indented occurrence is
        // either inside a function body, a comment, or a string.
        if raw_line.starts_with(' ') || raw_line.starts_with('\t') {
            continue;
        }
        let line = raw_line.trim_start();
        if starts_with_esm_keyword(line, "import") || starts_with_esm_keyword(line, "export") {
            return true;
        }
    }
    false
}

/// Returns true if `line` starts with `keyword` followed by a character
/// that can legally begin an `import`/`export` statement's continuation:
/// space, `{`, `*` (export only), `"`, `'`, or `(` (dynamic import). We
/// reject identifier-continuation characters (a-z, A-Z, 0-9, `_`, `$`) so
/// e.g. `exports.foo = …` does NOT match `export`, and `importMap = …`
/// does NOT match `import`.
fn starts_with_esm_keyword(line: &str, keyword: &str) -> bool {
    if let Some(rest) = line.strip_prefix(keyword) {
        match rest.chars().next() {
            None => false,
            Some(c) => {
                // Reject identifier-continuation: this is a different word
                // (`exports`, `importMap`, etc.), not the keyword.
                if c.is_alphanumeric() || c == '_' || c == '$' {
                    return false;
                }
                // Whitespace, `{`, `*`, `"`, `'`, `(` all legally follow
                // `import` or `export` — accept.
                matches!(c, ' ' | '\t' | '{' | '*' | '"' | '\'' | '(')
            }
        }
    } else {
        false
    }
}

/// JS reserved words that cannot be used as binding identifiers (e.g.
/// in `const X = ...`). Used by `extract_exports_from_source` to skip
/// CJS-style `module.exports.X = ...` patterns where `X` is a keyword —
/// emitting `export const <keyword> = _cjs.<keyword>;` would fail to
/// parse. `default` (pino's `module.exports.default = pino` interop
/// pattern) is the common real-world case; the rest are filtered
/// defensively. Contextual keywords (`async`, `arguments`, `eval`, `as`,
/// `from`, `of`) are legal identifiers and not included.
fn is_js_reserved_word(name: &str) -> bool {
    matches!(
        name,
        "break"
            | "case"
            | "catch"
            | "class"
            | "const"
            | "continue"
            | "debugger"
            | "default"
            | "delete"
            | "do"
            | "else"
            | "enum"
            | "export"
            | "extends"
            | "false"
            | "finally"
            | "for"
            | "function"
            | "if"
            | "import"
            | "in"
            | "instanceof"
            | "new"
            | "null"
            | "return"
            | "super"
            | "switch"
            | "this"
            | "throw"
            | "true"
            | "try"
            | "typeof"
            | "var"
            | "void"
            | "while"
            | "with"
            | "yield"
            | "let"
            | "static"
            | "implements"
            | "interface"
            | "package"
            | "private"
            | "protected"
            | "public"
            | "await"
    )
}

/// Wrap CJS source as ESM. `source_path` is the absolute path of the file
/// being wrapped — used to resolve `require('./relative')` targets when
/// peeking at re-export wrappers' transitive named exports.
pub(super) fn wrap_commonjs(source: &str, source_path: &Path) -> String {
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

    let mut named_exports = extract_exports_from_source(source);

    // For trivial re-export wrappers (`module.exports = require('./X')`),
    // recursively pull in the target's named exports. Without this,
    // react/index.js — which has zero `exports.X =` patterns of its own —
    // produces zero named exports and downstream `import { useState } from
    // "react"` link-fails.
    let parent = source_path.parent();
    if let Some(parent) = parent {
        for spec in &require_specs {
            if !spec.starts_with("./") && !spec.starts_with("../") {
                continue;
            }
            let target = parent.join(spec);
            if let Ok(target_source) = std::fs::read_to_string(&target) {
                for name in extract_exports_from_source(&target_source) {
                    if !named_exports.contains(&name) {
                        named_exports.push(name);
                    }
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
        (String::new(), Vec::new())
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

    let body_for_iife = if hoisted_class_block.is_empty() {
        source.to_string()
    } else {
        // Start from the source with hoisted classes already blanked, then
        // also blank the surfaced `var import_X = require(...)` lines so
        // they don't shadow the module-scope aliases when the IIFE runs.
        let mut s = source_without_hoists;
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
    const module = {{ exports: {{}} }};
    const exports = module.exports;
    function require(specifier) {{
{require_cases}
        throw new Error('require() is not supported: ' + specifier);
    }}

    {body_for_iife}

    return module.exports;
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

/// Issue #665: detect `module.exports = <BareIdentifier>;` patterns. Returns
/// `Some(name)` when at least one such assignment exists and every
/// `module.exports = ...` assignment in the source targets the same bare
/// identifier. Returns `None` if there are no such assignments, if multiple
/// assignments disagree, or if any assignment is to a non-identifier (object
/// literal, call, member expression, etc.) — those cases need the IIFE's
/// `module.exports` machinery to resolve correctly.
fn extract_single_module_exports_assignment(source: &str) -> Option<String> {
    let re = regex::Regex::new(r#"(?m)^\s*module\.exports\s*=\s*([^;\n]+?)\s*;?\s*$"#).ok()?;
    let ident_re = regex::Regex::new(r#"^[A-Za-z_$][A-Za-z0-9_$]*$"#).ok()?;
    let mut found: Option<String> = None;
    for cap in re.captures_iter(source) {
        let rhs = cap.get(1)?.as_str().trim();
        if !ident_re.is_match(rhs) {
            return None;
        }
        match &found {
            Some(prev) if prev != rhs => return None,
            Some(_) => {}
            None => found = Some(rhs.to_string()),
        }
    }
    found
}

/// Issue #665 follow-up: detect `(?:module\.)?exports\.NAME = require('SPEC')`
/// patterns and return `(name, spec)` pairs. Order is preserved and duplicates
/// (same NAME) are dropped on the first occurrence. If the same NAME also
/// appears with a non-`require(...)` RHS anywhere else in the source, the
/// pair is dropped — we don't want to forward a name that the file later
/// reassigns to a non-default-import value.
///
/// Matches both `exports.X = require('Y')` and `module.exports.X = require('Y')`.
/// Skips `__esModule` (the Babel/tsc interop marker; never user-meaningful).
fn extract_named_exports_from_require(source: &str) -> Vec<(String, String)> {
    let require_re = regex::Regex::new(
        r#"(?m)^\s*(?:module\.)?exports\.([A-Za-z_$][A-Za-z0-9_$]*)\s*=\s*require\s*\(\s*['"]([^'"]+)['"]\s*\)\s*;?\s*$"#,
    )
    .unwrap();
    // Any non-require assignment to the same `exports.X` should disqualify
    // the direct-reexport: the file is doing something more interesting and
    // we'd be skipping that runtime value if we routed through the import.
    let other_re = regex::Regex::new(
        r#"(?m)^\s*(?:module\.)?exports\.([A-Za-z_$][A-Za-z0-9_$]*)\s*=\s*(.+?)\s*;?\s*$"#,
    )
    .unwrap();

    let mut found: Vec<(String, String)> = Vec::new();
    let mut seen_names: Vec<String> = Vec::new();
    for cap in require_re.captures_iter(source) {
        if let (Some(name), Some(spec)) = (cap.get(1), cap.get(2)) {
            let name = name.as_str().to_string();
            if name == "__esModule" {
                continue;
            }
            if seen_names.contains(&name) {
                continue;
            }
            seen_names.push(name.clone());
            found.push((name, spec.as_str().to_string()));
        }
    }
    if found.is_empty() {
        return found;
    }
    // Filter out any name that ALSO appears with a non-require RHS. Walk the
    // looser regex; if a name we matched has an RHS that doesn't start with
    // `require(`, drop the pair.
    let mut disqualified: Vec<String> = Vec::new();
    for cap in other_re.captures_iter(source) {
        if let (Some(name), Some(rhs)) = (cap.get(1), cap.get(2)) {
            let name = name.as_str();
            if seen_names.iter().any(|n| n == name) {
                let rhs = rhs.as_str().trim();
                if !rhs.starts_with("require") {
                    disqualified.push(name.to_string());
                }
            }
        }
    }
    found.retain(|(n, _)| !disqualified.contains(n));
    found
}

/// Issue #665 follow-up (object-literal aggregator): detect the published
/// `rate-limiter-flexible/index.js` shape —
///
/// ```js
/// const RateLimiterMemory = require('./lib/RateLimiterMemory');
/// const RateLimiterRedis  = require('./lib/RateLimiterRedis');
/// module.exports = {
///   RateLimiterMemory,
///   RateLimiterRedis,
///   // ...
/// };
/// ```
///
/// Returns `(exported_name, require_spec)` pairs. Shorthand `{ X }` and longhand
/// `{ X: Y }` are both supported (for longhand, the RHS identifier is what
/// gets looked up against the require-alias table). The consumer's `import
/// { X } from "pkg"` then resolves through the emitted `export { _req_N as X }`
/// directly to the leaf module's default export — which compile.rs's
/// Export::Named arm propagates class identity through, so prototype methods
/// survive the indirection.
///
/// Edge cases skipped (left for the `_cjs.X` fallback):
///   - Computed keys (`[foo]: bar`).
///   - Spreads (`...obj`).
///   - Method definitions (`X() { ... }`).
///   - RHS expressions other than a bare identifier.
///   - Any case where the alias name doesn't match a `const|let|var X = require(...)`
///     binding elsewhere in the file.
///   - Multiple `module.exports = { ... }` assignments — we only inspect the
///     last one, since later assignments overwrite earlier ones at runtime.
fn extract_object_literal_exports_from_require(source: &str) -> Vec<(String, String)> {
    // Locate the LAST `module.exports = {` or `exports = {` (case where the file
    // reassigns the whole exports object). Anchored at start-of-line. We use
    // `rfind`-style behavior because later assignments win at runtime.
    let header_re = regex::Regex::new(r#"(?m)^\s*(?:module\.exports|exports)\s*=\s*\{"#).unwrap();
    let last_match = header_re.find_iter(source).last();
    let m = match last_match {
        Some(m) => m,
        None => return Vec::new(),
    };
    let bytes = source.as_bytes();
    // The `{` is the last char of the match.
    let mut p = m.end() - 1;
    if p >= bytes.len() || bytes[p] != b'{' {
        return Vec::new();
    }
    // Brace-balanced scan to find the matching `}`.
    let body_start = p + 1;
    let mut depth: i32 = 1;
    p = body_start;
    while p < bytes.len() && depth > 0 {
        match bytes[p] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            b'"' | b'\'' => {
                let quote = bytes[p];
                p += 1;
                while p < bytes.len() && bytes[p] != quote {
                    if bytes[p] == b'\\' && p + 1 < bytes.len() {
                        p += 2;
                        continue;
                    }
                    p += 1;
                }
            }
            b'`' => {
                p += 1;
                while p < bytes.len() && bytes[p] != b'`' {
                    if bytes[p] == b'\\' && p + 1 < bytes.len() {
                        p += 2;
                        continue;
                    }
                    p += 1;
                }
            }
            b'/' if p + 1 < bytes.len() && bytes[p + 1] == b'/' => {
                p += 2;
                while p < bytes.len() && bytes[p] != b'\n' {
                    p += 1;
                }
            }
            b'/' if p + 1 < bytes.len() && bytes[p + 1] == b'*' => {
                p += 2;
                while p + 1 < bytes.len() && !(bytes[p] == b'*' && bytes[p + 1] == b'/') {
                    p += 1;
                }
                if p + 1 < bytes.len() {
                    p += 2;
                }
            }
            _ => {}
        }
        if depth == 0 {
            break;
        }
        p += 1;
    }
    if depth != 0 || p <= body_start {
        return Vec::new();
    }
    let body = match std::str::from_utf8(&bytes[body_start..p]) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    // Build alias -> spec map from `const|let|var X = require('Y')` bindings.
    let alias_re = regex::Regex::new(
        r#"(?m)^\s*(?:var|const|let)\s+([A-Za-z_$][A-Za-z0-9_$]*)\s*=\s*require\s*\(\s*['"]([^'"]+)['"]\s*\)\s*;?"#,
    )
    .unwrap();
    let mut alias_to_spec: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for cap in alias_re.captures_iter(source) {
        if let (Some(name), Some(spec)) = (cap.get(1), cap.get(2)) {
            // First binding wins (matches JS hoisting / shadowing semantics).
            alias_to_spec
                .entry(name.as_str().to_string())
                .or_insert_with(|| spec.as_str().to_string());
        }
    }
    if alias_to_spec.is_empty() {
        return Vec::new();
    }

    // Split body into top-level entries (comma-separated, brace-balanced).
    let mut entries: Vec<String> = Vec::new();
    let body_bytes = body.as_bytes();
    let mut entry_start = 0usize;
    let mut bdepth: i32 = 0;
    let mut q = 0usize;
    while q < body_bytes.len() {
        match body_bytes[q] {
            b'{' | b'[' | b'(' => bdepth += 1,
            b'}' | b']' | b')' => bdepth -= 1,
            b'"' | b'\'' => {
                let quote = body_bytes[q];
                q += 1;
                while q < body_bytes.len() && body_bytes[q] != quote {
                    if body_bytes[q] == b'\\' && q + 1 < body_bytes.len() {
                        q += 2;
                        continue;
                    }
                    q += 1;
                }
            }
            b'`' => {
                q += 1;
                while q < body_bytes.len() && body_bytes[q] != b'`' {
                    if body_bytes[q] == b'\\' && q + 1 < body_bytes.len() {
                        q += 2;
                        continue;
                    }
                    q += 1;
                }
            }
            b'/' if q + 1 < body_bytes.len() && body_bytes[q + 1] == b'/' => {
                while q < body_bytes.len() && body_bytes[q] != b'\n' {
                    q += 1;
                }
                continue;
            }
            b'/' if q + 1 < body_bytes.len() && body_bytes[q + 1] == b'*' => {
                q += 2;
                while q + 1 < body_bytes.len()
                    && !(body_bytes[q] == b'*' && body_bytes[q + 1] == b'/')
                {
                    q += 1;
                }
                if q + 1 < body_bytes.len() {
                    q += 2;
                }
                continue;
            }
            b',' if bdepth == 0 => {
                let entry = body[entry_start..q].trim().to_string();
                if !entry.is_empty() {
                    entries.push(entry);
                }
                entry_start = q + 1;
            }
            _ => {}
        }
        q += 1;
    }
    let tail = body[entry_start..].trim().to_string();
    if !tail.is_empty() {
        entries.push(tail);
    }

    // Parse each entry as shorthand `X` or longhand `X: Y` (Y must be a bare ident).
    let shorthand_re = regex::Regex::new(r#"^[A-Za-z_$][A-Za-z0-9_$]*$"#).unwrap();
    let longhand_re =
        regex::Regex::new(r#"^([A-Za-z_$][A-Za-z0-9_$]*)\s*:\s*([A-Za-z_$][A-Za-z0-9_$]*)$"#)
            .unwrap();
    let mut out: Vec<(String, String)> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for entry in entries {
        // Strip trailing line/block comments and the trailing comma we might
        // have included.
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        if shorthand_re.is_match(entry) {
            if entry == "__esModule" {
                continue;
            }
            if let Some(spec) = alias_to_spec.get(entry) {
                if seen.insert(entry.to_string()) {
                    out.push((entry.to_string(), spec.clone()));
                }
            }
        } else if let Some(cap) = longhand_re.captures(entry) {
            let key = cap.get(1).unwrap().as_str();
            let val = cap.get(2).unwrap().as_str();
            if key == "__esModule" {
                continue;
            }
            if let Some(spec) = alias_to_spec.get(val) {
                if seen.insert(key.to_string()) {
                    out.push((key.to_string(), spec.clone()));
                }
            }
        }
        // Anything else (computed keys, spreads, methods, expressions) is
        // intentionally skipped — those need the `_cjs.X` runtime path.
    }
    out
}

/// Refs #488 drizzle-sqlite: extract `var <alias> = require("<spec>");`
/// declarations from the source as `(alias_name, spec, (start_byte,
/// end_byte))`. The byte range covers the whole matched statement so
/// `wrap_commonjs` can blank it from the IIFE body — leaving the binding
/// only at module scope where the wrap emits `const <alias> = _req_N;`,
/// so hoisted class declarations' `extends <alias>.Y` resolve correctly
/// without the inner `var` re-binding shadowing the outer alias when the
/// IIFE evaluates.
///
/// Matches `var` / `const` / `let`. Order is preserved and duplicates
/// are dropped on the alias name (the first binding wins — matches JS
/// hoisting semantics for the original source).
///
/// Issue #845: the trailing `\s*(?:;|$)` (require a semicolon or
/// end-of-line in multiline mode) is intentional. Without it,
/// `const EventEmitter = require('events').EventEmitter;` matches as
/// `const EventEmitter = require('events')` and the blanking pass at
/// line 336 above leaves `.EventEmitter;` dangling at column 0 of the
/// wrapped output, producing a TS1109 ("Expression expected") parse
/// failure 1000+ bytes past EOF. Only whole-statement aliases (those
/// whose require call is followed by `;` or end-of-line) are safe to
/// blank — anything with `.X` trailing member access binds to the
/// property, not the module object, so the alias-rename pass would
/// be wrong anyway. Same-line follow-on statements like
/// `var dep = require('./dep'); module.exports = dep.value;` still
/// match because the `;` form ends the alias matched region before
/// the follow-on.
fn extract_require_aliases_with_ranges(source: &str) -> Vec<(String, String, (usize, usize))> {
    let re = regex::Regex::new(
        r#"(?m)^\s*(?:var|const|let)\s+([A-Za-z_$][A-Za-z0-9_$]*)\s*=\s*require\s*\(\s*['"]([^'"]+)['"]\s*\)\s*(?:;|$)"#,
    )
    .unwrap();
    let mut seen = Vec::new();
    let mut out = Vec::new();
    for cap in re.captures_iter(source) {
        if let (Some(alias), Some(spec), Some(whole)) = (cap.get(1), cap.get(2), cap.get(0)) {
            let alias = alias.as_str().to_string();
            if seen.contains(&alias) {
                continue;
            }
            seen.push(alias.clone());
            out.push((
                alias,
                spec.as_str().to_string(),
                (whole.start(), whole.end()),
            ));
        }
    }
    out
}

/// Issue #665 (fifth pass): rewrite the leaf-file shape
/// `module.exports = class Name { ... };` into declaration form
/// `class Name { ... }\nmodule.exports = Name;` so the existing
/// `extract_top_level_class_decls` + `extract_single_module_exports_assignment`
/// pipeline can surface the class as a module-scope binding. Returns the
/// rewritten source on success; `None` when the input does not match the
/// pattern (rest of the pipeline runs unchanged in that case).
///
/// This is the class-expression counterpart to the v0.5.839 fix, which
/// only handled the declaration form. Real-world packages like
/// rate-limiter-flexible (`lib/RateLimiterAbstract.js`) ship the
/// expression form, which made `super(opts)` calls from child classes
/// silently no-op the parent constructor — the consumer's `import X` saw
/// only the opaque `_cjs` IIFE result, never registered class identity
/// in compile.rs, and codegen's super-call dispatch fell through to the
/// no-parent-in-ctx branch.
///
/// Defensive constraints (returns `None` if any fails):
///   - exactly one top-level `module.exports = ...` assignment exists
///   - that assignment is anchored at column 0 (no leading whitespace)
///   - the RHS starts with `class\b`
///   - the class body is brace-balanced (with string/template/comment skip)
///   - the chosen class name does not collide with any existing top-level
///     `class <Name>` declaration in the source
fn rewrite_module_exports_class_expression(source: &str) -> Option<String> {
    // Find every `module.exports = ...` assignment at column 0. Multiple
    // (possibly conflicting) targets disqualify the rewrite — the IIFE's
    // last-assignment-wins semantics must keep running through `_cjs`.
    let any_assign_re = regex::Regex::new(r#"(?m)^module\.exports[\t ]*="#).ok()?;
    let assigns: Vec<_> = any_assign_re.find_iter(source).collect();
    if assigns.len() != 1 {
        return None;
    }
    let assign = &assigns[0];
    let assign_start = assign.start();
    let assign_end_byte = assign.end();

    let bytes = source.as_bytes();

    // Locate the `class` keyword after `module.exports =` (with optional
    // intervening spaces / tabs — we don't cross newlines into the RHS).
    let mut p = assign_end_byte;
    while p < bytes.len() && (bytes[p] == b' ' || bytes[p] == b'\t') {
        p += 1;
    }
    let class_kw_start = p;
    if class_kw_start + "class".len() > bytes.len() {
        return None;
    }
    if &bytes[class_kw_start..class_kw_start + "class".len()] != b"class" {
        return None;
    }
    // `class` must be followed by a non-identifier character (whitespace,
    // `{`, etc.) so we don't match `classify` or similar.
    let after_kw = class_kw_start + "class".len();
    if after_kw >= bytes.len() {
        return None;
    }
    let next = bytes[after_kw];
    let is_ident_cont = next.is_ascii_alphanumeric() || next == b'_' || next == b'$';
    if is_ident_cont {
        return None;
    }
    p = after_kw;

    // Skip whitespace (including newlines — the class body can span lines,
    // and the optional name may sit on the next line in rare formatting).
    while p < bytes.len() && bytes[p].is_ascii_whitespace() {
        p += 1;
    }

    // Optional class name.
    let name_start = p;
    while p < bytes.len()
        && (bytes[p].is_ascii_alphanumeric() || bytes[p] == b'_' || bytes[p] == b'$')
    {
        p += 1;
    }
    let name_end = p;
    let parsed_name = if name_end > name_start {
        Some(
            std::str::from_utf8(&bytes[name_start..name_end])
                .ok()?
                .to_string(),
        )
    } else {
        None
    };

    // Scan forward to the opening `{` of the class body. `extends X`
    // clauses live here and may include member access / call expressions,
    // but not newlines that exit the declaration head — class bodies
    // always open with `{` before any executable statement.
    while p < bytes.len() && bytes[p] != b'{' {
        p += 1;
    }
    if p >= bytes.len() {
        return None;
    }
    let body_start = p;

    // Brace-balanced scan, skipping string / template / line-comment /
    // block-comment contents. Mirrors the logic in
    // `extract_top_level_class_decls`.
    let mut depth: i32 = 0;
    let mut r = body_start;
    while r < bytes.len() {
        match bytes[r] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    r += 1;
                    break;
                }
            }
            b'"' | b'\'' => {
                let quote = bytes[r];
                r += 1;
                while r < bytes.len() && bytes[r] != quote {
                    if bytes[r] == b'\\' && r + 1 < bytes.len() {
                        r += 2;
                        continue;
                    }
                    r += 1;
                }
            }
            b'`' => {
                r += 1;
                while r < bytes.len() && bytes[r] != b'`' {
                    if bytes[r] == b'\\' && r + 1 < bytes.len() {
                        r += 2;
                        continue;
                    }
                    r += 1;
                }
            }
            b'/' if r + 1 < bytes.len() && bytes[r + 1] == b'/' => {
                r += 2;
                while r < bytes.len() && bytes[r] != b'\n' {
                    r += 1;
                }
            }
            b'/' if r + 1 < bytes.len() && bytes[r + 1] == b'*' => {
                r += 2;
                while r + 1 < bytes.len() && !(bytes[r] == b'*' && bytes[r + 1] == b'/') {
                    r += 1;
                }
                if r + 1 < bytes.len() {
                    r += 2;
                }
            }
            _ => {}
        }
        r += 1;
    }
    if depth != 0 {
        return None;
    }
    let body_end = r;

    // Optional trailing whitespace + optional `;` to consume.
    let mut q = body_end;
    while q < bytes.len() && (bytes[q] == b' ' || bytes[q] == b'\t') {
        q += 1;
    }
    if q < bytes.len() && bytes[q] == b';' {
        q += 1;
    }

    // Pick the name to use in the rewritten declaration. Anonymous gets
    // a synthetic name. Reject if a top-level `class <ChosenName>`
    // declaration already exists — we don't want to emit duplicates.
    let chosen_name = parsed_name
        .clone()
        .unwrap_or_else(|| "__perry_cjs_default__".to_string());
    let collision_pattern = format!(r#"(?m)^class[\t ]+{}\b"#, regex::escape(&chosen_name));
    let collision_re = regex::Regex::new(&collision_pattern).ok()?;
    if collision_re.is_match(source) {
        return None;
    }

    // Build the replacement. Use the original class head when named
    // (`class Foo extends Bar `) so any extends clause survives byte-for-byte.
    // For anonymous, inject the synthetic name between `class` and the rest.
    let class_head = if parsed_name.is_some() {
        std::str::from_utf8(&bytes[class_kw_start..body_start])
            .ok()?
            .to_string()
    } else {
        let after_class_kw = std::str::from_utf8(&bytes[after_kw..body_start]).ok()?;
        format!("class {}{}", chosen_name, after_class_kw)
    };
    let class_body = std::str::from_utf8(&bytes[body_start..body_end]).ok()?;
    let replacement = format!(
        "{}{}\nmodule.exports = {};",
        class_head, class_body, chosen_name
    );

    let mut s = source.to_string();
    s.replace_range(assign_start..q, &replacement);
    Some(s)
}

/// Issue #652: extract top-level `class X { ... }` declarations from the CJS
/// source so they can be hoisted OUT of the wrapping IIFE. Returns:
///   - the extracted class block (joined with newlines, empty if none)
///   - the list of class names extracted
///   - the source with the class blocks replaced by blank lines (preserves
///     line numbers for diagnostics)
///
/// Detection is brace-balanced, anchored to lines where `class ` appears at
/// column 0 (strict top-level only — nested classes inside functions /
/// blocks / object literals are left alone). Skips classes whose name is
/// already a duplicate of a previously-seen class (defensive).
fn extract_top_level_class_decls(source: &str) -> (String, Vec<String>, String) {
    let bytes = source.as_bytes();
    let mut hoisted_blocks: Vec<&str> = Vec::new();
    let mut hoisted_names: Vec<String> = Vec::new();
    let mut elided: Vec<(usize, usize)> = Vec::new();

    let mut i = 0usize;
    while i < bytes.len() {
        // Anchor on a `class` keyword at the start of a line (after only
        // whitespace would also be acceptable in principle, but real CJS
        // packages put their class declarations at column 0).
        let line_start = if i == 0 || bytes[i - 1] == b'\n' {
            i
        } else {
            // Find the next newline; advance.
            i += 1;
            continue;
        };

        // Match optional leading whitespace.
        let mut p = line_start;
        while p < bytes.len() && (bytes[p] == b' ' || bytes[p] == b'\t') {
            p += 1;
        }

        if p + 6 <= bytes.len() && &bytes[p..p + 6] == b"class " {
            // Skip past "class ".
            let name_start = p + 6;
            // Scan identifier.
            let mut name_end = name_start;
            while name_end < bytes.len() {
                let c = bytes[name_end];
                let valid = (c.is_ascii_alphanumeric()) || c == b'_' || c == b'$';
                if !valid {
                    break;
                }
                name_end += 1;
            }
            if name_end > name_start {
                let class_name = std::str::from_utf8(&bytes[name_start..name_end])
                    .unwrap_or("")
                    .to_string();
                // Skip whitespace + optional `extends ...` clause + opening `{`.
                let mut q = name_end;
                while q < bytes.len() && (bytes[q] == b' ' || bytes[q] == b'\t') {
                    q += 1;
                }
                // Optional `extends X` (or `extends X.Y` / `extends X(arg)` etc.) — scan
                // until we hit the opening `{` for the class body, refusing
                // to cross newlines so we stay inside the declaration head.
                while q < bytes.len() && bytes[q] != b'{' && bytes[q] != b'\n' {
                    q += 1;
                }
                if q < bytes.len() && bytes[q] == b'{' {
                    // Brace-balanced scan to find the matching closing `}`.
                    let body_start = q;
                    let mut depth: i32 = 0;
                    let mut r = q;
                    while r < bytes.len() {
                        match bytes[r] {
                            b'{' => depth += 1,
                            b'}' => {
                                depth -= 1;
                                if depth == 0 {
                                    r += 1;
                                    break;
                                }
                            }
                            // String / template / line-comment / block-comment
                            // skip — minimal handling, sufficient for typical
                            // class bodies. Class bodies don't usually contain
                            // string literals with stray braces, but handle
                            // the common cases defensively.
                            b'"' | b'\'' => {
                                let quote = bytes[r];
                                r += 1;
                                while r < bytes.len() && bytes[r] != quote {
                                    if bytes[r] == b'\\' && r + 1 < bytes.len() {
                                        r += 2;
                                        continue;
                                    }
                                    r += 1;
                                }
                            }
                            b'`' => {
                                r += 1;
                                while r < bytes.len() && bytes[r] != b'`' {
                                    if bytes[r] == b'\\' && r + 1 < bytes.len() {
                                        r += 2;
                                        continue;
                                    }
                                    r += 1;
                                }
                            }
                            b'/' if r + 1 < bytes.len() && bytes[r + 1] == b'/' => {
                                r += 2;
                                while r < bytes.len() && bytes[r] != b'\n' {
                                    r += 1;
                                }
                            }
                            b'/' if r + 1 < bytes.len() && bytes[r + 1] == b'*' => {
                                r += 2;
                                while r + 1 < bytes.len()
                                    && !(bytes[r] == b'*' && bytes[r + 1] == b'/')
                                {
                                    r += 1;
                                }
                                if r + 1 < bytes.len() {
                                    r += 2;
                                }
                            }
                            _ => {}
                        }
                        r += 1;
                    }
                    if depth == 0 && r > body_start {
                        // Successful brace-balanced match. Record the block.
                        let block_text = std::str::from_utf8(&bytes[line_start..r]).unwrap_or("");
                        if !hoisted_names.contains(&class_name) {
                            hoisted_blocks.push(block_text);
                            hoisted_names.push(class_name);
                            elided.push((line_start, r));
                        }
                        i = r;
                        continue;
                    }
                }
            }
        }
        // Advance to the next line.
        while i < bytes.len() && bytes[i] != b'\n' {
            i += 1;
        }
        i += 1;
    }

    let mut out_source = source.to_string();
    // Replace the elided ranges with whitespace (back-to-front to preserve
    // earlier indices). Empty out the original class body but keep newlines
    // for line-number stability.
    for (start, end) in elided.iter().rev() {
        let original = &source[*start..*end];
        let blanked: String = original
            .chars()
            .map(|c| if c == '\n' { '\n' } else { ' ' })
            .collect();
        out_source.replace_range(*start..*end, &blanked);
    }

    let hoisted_block = hoisted_blocks.join("\n");
    (hoisted_block, hoisted_names, out_source)
}

/// Extract `require('X')` / `require("X")` specifiers, preserving order and
/// deduping. Only matches static string literal arguments — dynamic
/// `require(someVar)` is unrepresentable as ESM and the bound `require`
/// inside the IIFE will throw at runtime if hit.
fn extract_require_specifiers(source: &str) -> Vec<String> {
    let re = regex::Regex::new(r#"require\s*\(\s*['"]([^'"]+)['"]\s*\)"#).unwrap();
    let mut specs = Vec::new();
    for cap in re.captures_iter(source) {
        if let Some(m) = cap.get(1) {
            let s = m.as_str().to_string();
            if !specs.contains(&s) {
                specs.push(s);
            }
        }
    }
    specs
}

/// Extract named-export patterns from CJS source. Three shapes are matched:
///
///   1. `exports.X = ...` and `module.exports.X = ...` — the canonical CJS
///      named-export form. Skips `__esModule` (the interop marker injected
///      by Babel/TypeScript that consumers use to detect "this is a CJS
///      module pretending to be ESM" — we don't want to re-export a boolean
///      as if it were a named binding).
///   2. `module.exports = { X, Y, fn: someFn }` — object-literal assignment
///      to `module.exports`. Issue #624: the synthetic-package shape that
///      hand-written CJS code typically uses (and that React's transpiled
///      output occasionally falls back to) was unsupported, so the consumer
///      `import { X } from "pkg"` link-failed because no named export was
///      ever extracted.
fn extract_exports_from_source(source: &str) -> Vec<String> {
    let mut names = Vec::new();
    let push_unique = |names: &mut Vec<String>, name: &str| {
        if name == "__esModule" {
            return;
        }
        // Issue #845: skip JS reserved words. `export const default = _cjs.default;`
        // (and other reserved-word forms) is invalid syntax — the named-export
        // emission emits `export const <NAME> = _cjs.<NAME>;`, which fails to
        // parse if `<NAME>` isn't a valid binding identifier. `default` is the
        // common real-world case (pino: `module.exports.default = pino` —
        // ESM-interop convention). The default export is already covered by
        // the separate `export default _cjs;` statement, so skipping `default`
        // here doesn't lose any export. Reserved words other than `default`
        // are extremely rare as CJS export names but would parse-fail the
        // same way, so filter them all.
        if is_js_reserved_word(name) {
            return;
        }
        let owned = name.to_string();
        if !names.contains(&owned) {
            names.push(owned);
        }
    };

    // Shape 1: `exports.X = ...` / `module.exports.X = ...`
    let dot_re = regex::Regex::new(
        r"(?:^|[^A-Za-z0-9_$])(?:module\.)?exports\.([A-Za-z_$][A-Za-z0-9_$]*)\s*=",
    )
    .unwrap();
    for cap in dot_re.captures_iter(source) {
        if let Some(m) = cap.get(1) {
            push_unique(&mut names, m.as_str());
        }
    }

    // Shape 2: `module.exports = { ... }` — extract every key from the
    // object literal body. Brace-balanced scan because the body may contain
    // nested braces (`module.exports = { fn: function() {} }`). Two key
    // forms are recognized:
    //   - `name` (shorthand: `{ createContext }` ≡ `{ createContext: createContext }`)
    //   - `name: <expr>` (explicit: `{ createContext: createContext }` or `{ name: function() {} }`)
    // String-keyed entries (`"name": …`) and computed-key entries
    // (`[expr]: …`) are intentionally skipped — those don't surface as ESM
    // named exports anyway.
    let bytes = source.as_bytes();
    let mut search_from = 0usize;
    while let Some(idx) = source[search_from..].find("module.exports") {
        let abs = search_from + idx;
        // Skip past `module.exports`
        let mut p = abs + "module.exports".len();
        // Skip whitespace
        while p < bytes.len() && (bytes[p] == b' ' || bytes[p] == b'\t' || bytes[p] == b'\n') {
            p += 1;
        }
        // Must be `=` (not `.`, `==`, etc.)
        if p >= bytes.len() || bytes[p] != b'=' {
            search_from = abs + 1;
            continue;
        }
        // Reject `==` / `===`
        if p + 1 < bytes.len() && bytes[p + 1] == b'=' {
            search_from = abs + 1;
            continue;
        }
        p += 1;
        // Skip whitespace
        while p < bytes.len() && (bytes[p] == b' ' || bytes[p] == b'\t' || bytes[p] == b'\n') {
            p += 1;
        }
        // Must be `{`
        if p >= bytes.len() || bytes[p] != b'{' {
            search_from = abs + 1;
            continue;
        }
        // Brace-balanced scan to find the matching close.
        let body_start = p + 1;
        let mut depth: i32 = 1;
        let mut q = body_start;
        while q < bytes.len() && depth > 0 {
            match bytes[q] {
                b'{' => depth += 1,
                b'}' => depth -= 1,
                _ => {}
            }
            q += 1;
        }
        if depth != 0 {
            // Unbalanced — bail out, advance and continue scanning.
            search_from = abs + 1;
            continue;
        }
        let body_end = q - 1; // points at the closing `}`
        let body = &source[body_start..body_end];
        extract_object_literal_keys(body, &mut |name| push_unique(&mut names, name));
        search_from = q;
    }

    names
}

/// Extract top-level keys from an object-literal body (the text between
/// `{` and `}`, exclusive). Skips nested braces / brackets / parens so
/// `fn: function() { return 1; }` doesn't pull `return` as a key. Calls
/// `out` with each shorthand or `name:` key encountered at depth 0.
fn extract_object_literal_keys(body: &str, out: &mut dyn FnMut(&str)) {
    let bytes = body.as_bytes();
    let mut i = 0;
    let mut at_entry_start = true;
    let mut depth: i32 = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'{' | b'[' | b'(' => {
                depth += 1;
                at_entry_start = false;
                i += 1;
            }
            b'}' | b']' | b')' => {
                depth -= 1;
                i += 1;
            }
            b',' if depth == 0 => {
                at_entry_start = true;
                i += 1;
            }
            b' ' | b'\t' | b'\n' | b'\r' => {
                i += 1;
            }
            _ if depth == 0 && at_entry_start => {
                // Try to read an identifier at the start of an entry.
                if (b as char).is_ascii_alphabetic() || b == b'_' || b == b'$' {
                    let start = i;
                    while i < bytes.len() {
                        let c = bytes[i];
                        if (c as char).is_ascii_alphanumeric() || c == b'_' || c == b'$' {
                            i += 1;
                        } else {
                            break;
                        }
                    }
                    let name = &body[start..i];
                    // Skip whitespace after the name.
                    let mut j = i;
                    while j < bytes.len()
                        && (bytes[j] == b' ' || bytes[j] == b'\t' || bytes[j] == b'\n')
                    {
                        j += 1;
                    }
                    // Accept shorthand (`,` / end-of-body) or explicit key (`:`).
                    if j == bytes.len() || bytes[j] == b',' || bytes[j] == b':' {
                        out(name);
                    }
                    at_entry_start = false;
                } else {
                    // Non-identifier at entry start (e.g. `"key":` string,
                    // `[expr]:` computed, `...spread`) — skip; not an ESM
                    // exportable name.
                    at_entry_start = false;
                    i += 1;
                }
            }
            _ => {
                at_entry_start = false;
                i += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn detects_module_exports_assignment() {
        assert!(is_commonjs("module.exports = function() {};"));
    }

    #[test]
    fn detects_exports_dot_pattern() {
        assert!(is_commonjs("exports.foo = 1;"));
    }

    #[test]
    fn detects_require_without_import() {
        assert!(is_commonjs("var x = require('foo');"));
    }

    #[test]
    fn does_not_detect_pure_esm() {
        assert!(!is_commonjs("import x from 'foo'; export const y = 1;"));
    }

    #[test]
    fn issue_851_rollup_hybrid_esm_with_inner_cjs_is_esm() {
        // Rollup-bundled output (vitest's `dist/chunks/*.js` shape):
        // top-level ESM `import` + inlined CJS body in a nested IIFE.
        // Such files MUST be treated as ESM — wrapping them moves the
        // `import` inside the IIFE and SWC errors `ImportExportInScript`.
        let src = r#"import { foo } from 'bar';
function helper() {
  (function (module, exports$1) {
    module.exports = factory();
  })(this, function() { return {}; });
}
export const baz = helper();
"#;
        assert!(
            !is_commonjs(src),
            "rollup hybrid ESM/CJS file must be classified as ESM"
        );
    }

    #[test]
    fn issue_851_top_level_export_wins_over_cjs_tokens() {
        // Even with `module.exports` and `exports.` patterns inside
        // function bodies, a top-level `export` makes this ESM.
        let src = r#"export { x } from './x';
function inner() {
  module.exports = 1;
  exports.foo = 2;
}
"#;
        assert!(!is_commonjs(src));
    }

    #[test]
    fn issue_851_export_star_is_esm() {
        // `export *` is a valid top-level ESM form.
        let src = "export * from './re';\nfunction inner() { module.exports = 1; }\n";
        assert!(!is_commonjs(src));
    }

    #[test]
    fn issue_851_does_not_match_exports_dot_as_export_keyword() {
        // Make sure `exports.foo = …` at the top level is NOT mistakenly
        // matched as `export` (the keyword check must reject identifier
        // continuation `s`).
        let src = "exports.foo = 1;\n";
        assert!(is_commonjs(src));
    }

    #[test]
    fn issue_851_does_not_match_importmap_identifier() {
        // `importMap = …` is a plain identifier write, not an import
        // statement; it must not flip ESM detection.
        let src = "var importMap = {};\nmodule.exports = importMap;\n";
        assert!(is_commonjs(src));
    }

    #[test]
    fn issue_851_indented_import_is_ignored() {
        // An `import` keyword inside a function body (indented) must
        // not classify the file as ESM.
        let src = r#"function inner() {
    import('./x'); // dynamic import inside a function — not top-level
}
module.exports = inner;
"#;
        assert!(is_commonjs(src));
    }

    #[test]
    fn issue_851_top_level_dynamic_import_counts_as_esm() {
        // A bare `import('./x')` at column 0 is a top-level
        // (dynamic-import) expression — only valid in module scope.
        // Treating it as ESM is the safe call.
        let src = "import('./x');\nmodule.exports = 1;\n";
        assert!(!is_commonjs(src));
    }

    #[test]
    fn extracts_named_exports() {
        let src = "exports.foo = 1; exports.bar = function() {}; exports.__esModule = true;";
        let names = extract_exports_from_source(src);
        assert_eq!(names, vec!["foo".to_string(), "bar".to_string()]);
    }

    #[test]
    fn extracts_module_exports_object_literal_shorthand() {
        // Issue #624: `module.exports = { createContext }`
        let src = "function createContext(v){return v;}\nmodule.exports = { createContext };";
        let names = extract_exports_from_source(src);
        assert_eq!(names, vec!["createContext".to_string()]);
    }

    #[test]
    fn extracts_module_exports_object_literal_explicit() {
        // `module.exports = { foo: foo, bar: function(){} }`
        let src = "module.exports = { foo: foo, bar: function(){} };";
        let names = extract_exports_from_source(src);
        assert_eq!(names, vec!["foo".to_string(), "bar".to_string()]);
    }

    #[test]
    fn extracts_module_exports_dot_form() {
        // `module.exports.foo = ...`
        let src = "module.exports.foo = 1; module.exports.bar = 2;";
        let names = extract_exports_from_source(src);
        assert_eq!(names, vec!["foo".to_string(), "bar".to_string()]);
    }

    #[test]
    fn extracts_unions_dot_and_object_literal_forms() {
        let src = "exports.a = 1; module.exports = { b, c };";
        let names = extract_exports_from_source(src);
        assert_eq!(
            names,
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
    }

    #[test]
    fn extracts_require_specifiers_dedup() {
        let src = r#"var a = require('./a'); var b = require("./b"); var c = require('./a');"#;
        let specs = extract_require_specifiers(src);
        assert_eq!(specs, vec!["./a".to_string(), "./b".to_string()]);
    }

    #[test]
    fn wraps_simple_cjs_as_esm() {
        let src = "exports.foo = 42;";
        let wrapped = wrap_commonjs(src, &PathBuf::from("/tmp/test.js"));
        assert!(wrapped.contains("export default _cjs;"));
        assert!(wrapped.contains("export const foo = _cjs.foo;"));
        assert!(wrapped.contains("const _cjs = (function()"));
    }

    #[test]
    fn wrap_hoists_require_as_import() {
        // Issue #665 (third pass): when the CJS source has a unique alias
        // `var dep = require('./dep')`, the wrap uses the alias name as the
        // import local so compile.rs propagates class identity for `dep`.
        // The `_req_0` placeholder only appears when no safe alias is found.
        let src = "var dep = require('./dep'); module.exports = dep.value;";
        let wrapped = wrap_commonjs(src, &PathBuf::from("/tmp/test.js"));
        assert!(
            wrapped.contains("import dep from './dep';"),
            "expected import using alias name, got:\n{}",
            wrapped
        );
        assert!(
            wrapped.contains("if (specifier === './dep') return dep;"),
            "expected require dispatch through aliased import, got:\n{}",
            wrapped
        );
    }

    #[test]
    fn wrap_falls_back_to_req_n_when_alias_unsafe() {
        // Reserved internal names (`_cjs`, `module`, `exports`, `require`)
        // and `_req_<N>` aliases must not become import locals — fall back
        // to the auto-generated `_req_N` instead.
        let src = "var _cjs = require('./a'); module.exports = 1;";
        let wrapped = wrap_commonjs(src, &PathBuf::from("/tmp/test.js"));
        assert!(
            wrapped.contains("import _req_0 from './a';"),
            "expected _req_0 fallback when alias collides with wrap internals, got:\n{}",
            wrapped
        );
    }

    #[test]
    fn wrap_aliases_import_for_hoisted_class_extends_and_strips_iife_var() {
        // Refs #488 drizzle-sqlite: hoisted `class B extends import_X.Y { }`
        // needs `import_X` bound at module scope (not just inside the IIFE),
        // AND the inner `var import_X = require("...")` must be stripped so
        // it doesn't re-bind in IIFE scope and shadow the outer alias when
        // the IIFE runs.
        //
        // Issue #665 (third pass): the alias `import_dep` is now used as
        // the import local name directly (`import import_dep from "./dep.cjs"`),
        // so the separate `const import_dep = _req_N;` line is no longer
        // needed. The hoisted class's `extends import_dep.A` still resolves
        // because `import_dep` is a module-scope binding.
        let src = "var import_dep = require(\"./dep.cjs\");\nclass B extends import_dep.A {\n  foo = 1;\n}\nexports.B = B;";
        let wrapped = wrap_commonjs(src, &PathBuf::from("/tmp/test.js"));
        let import_pos = wrapped
            .find("import import_dep from './dep.cjs';")
            .expect("module-scope import using alias name missing");
        let class_pos = wrapped
            .find("class B extends import_dep.A")
            .expect("hoisted class missing");
        assert!(
            import_pos < class_pos,
            "alias-as-import must precede hoisted class so `extends import_dep.A` resolves"
        );
        // Inner `var import_dep = require(...)` must NOT survive — otherwise
        // it shadows the outer import inside the IIFE and re-breaks the
        // hoisted class's parent link.
        let var_count = wrapped
            .matches("var import_dep = require(\"./dep.cjs\")")
            .count();
        assert_eq!(var_count, 0, "inner var declaration must be stripped");
    }

    #[test]
    fn detects_single_module_exports_class_assignment() {
        // Issue #665: rate-limiter-flexible shape.
        let src = "class Child {}\nmodule.exports = Child;";
        assert_eq!(
            extract_single_module_exports_assignment(src),
            Some("Child".to_string())
        );
    }

    #[test]
    fn rejects_object_literal_module_exports() {
        let src = "module.exports = { foo: 1 };";
        assert_eq!(extract_single_module_exports_assignment(src), None);
    }

    #[test]
    fn rejects_member_expr_module_exports() {
        let src = "module.exports = dep.value;";
        assert_eq!(extract_single_module_exports_assignment(src), None);
    }

    #[test]
    fn rejects_conflicting_module_exports_targets() {
        let src = "module.exports = Foo;\nmodule.exports = Bar;";
        assert_eq!(extract_single_module_exports_assignment(src), None);
    }

    #[test]
    fn wrap_emits_direct_default_export_for_class_module_exports() {
        // Issue #665: `module.exports = Child` + hoisted `class Child {...}`.
        let src = "class Child { greet(){} }\nmodule.exports = Child;";
        let wrapped = wrap_commonjs(src, &PathBuf::from("/tmp/test.js"));
        assert!(
            wrapped.contains("export default Child;"),
            "expected direct default export of Child, got:\n{}",
            wrapped
        );
        assert!(
            !wrapped.contains("export default _cjs;"),
            "should bypass _cjs for single-class module.exports, got:\n{}",
            wrapped
        );
        assert!(wrapped.contains("export { Child };"));
    }

    #[test]
    fn wrap_keeps_cjs_default_when_module_exports_is_object_literal() {
        let src = "module.exports = { foo: 1, bar: 2 };";
        let wrapped = wrap_commonjs(src, &PathBuf::from("/tmp/test.js"));
        assert!(wrapped.contains("export default _cjs;"));
    }

    #[test]
    fn wrap_keeps_cjs_default_when_module_exports_is_function_call() {
        let src = "module.exports = makeThing();";
        let wrapped = wrap_commonjs(src, &PathBuf::from("/tmp/test.js"));
        assert!(wrapped.contains("export default _cjs;"));
    }

    #[test]
    fn extracts_named_exports_from_require_basic() {
        // Issue #665 follow-up: rate-limiter-flexible-shaped index.js
        let src = "module.exports.RateLimiterMemory = require('./lib/RateLimiterMemory');\nmodule.exports.Foo = require('./lib/Foo');";
        let got = extract_named_exports_from_require(src);
        assert_eq!(
            got,
            vec![
                (
                    "RateLimiterMemory".to_string(),
                    "./lib/RateLimiterMemory".to_string()
                ),
                ("Foo".to_string(), "./lib/Foo".to_string()),
            ]
        );
    }

    #[test]
    fn extracts_named_exports_from_require_bare_exports_dot() {
        let src = "exports.Bar = require('./bar');";
        let got = extract_named_exports_from_require(src);
        assert_eq!(got, vec![("Bar".to_string(), "./bar".to_string())]);
    }

    #[test]
    fn skips_named_export_when_name_has_non_require_assignment() {
        // If the file ALSO does something else with the same name, route
        // through the IIFE (via `_cjs.X`) so the file's runtime semantics win.
        let src = "exports.X = require('./x');\nexports.X = wrap(exports.X);";
        let got = extract_named_exports_from_require(src);
        assert!(got.is_empty(), "expected empty, got {:?}", got);
    }

    #[test]
    fn wrap_emits_direct_reexport_for_module_exports_dot_require() {
        // Issue #665 follow-up: rate-limiter-flexible-shaped index.js
        let src = "module.exports.RateLimiterMemory = require('./lib/RateLimiterMemory');";
        let wrapped = wrap_commonjs(src, &PathBuf::from("/tmp/test.js"));
        assert!(
            wrapped.contains("export { _req_0 as RateLimiterMemory };"),
            "expected direct re-export, got:\n{}",
            wrapped
        );
        // And does NOT emit the property-read form for the same name.
        assert!(
            !wrapped.contains("export const RateLimiterMemory = _cjs.RateLimiterMemory;"),
            "should NOT emit _cjs property read for direct-reexport name, got:\n{}",
            wrapped
        );
    }

    #[test]
    fn extracts_object_literal_aggregator_shorthand() {
        // Issue #665 latest comment: real rate-limiter-flexible/index.js shape.
        let src = "const RateLimiterMemory = require('./lib/RateLimiterMemory');\n\
                   const RateLimiterRedis = require('./lib/RateLimiterRedis');\n\
                   module.exports = { RateLimiterMemory, RateLimiterRedis };";
        let got = extract_object_literal_exports_from_require(src);
        assert_eq!(
            got,
            vec![
                (
                    "RateLimiterMemory".to_string(),
                    "./lib/RateLimiterMemory".to_string()
                ),
                (
                    "RateLimiterRedis".to_string(),
                    "./lib/RateLimiterRedis".to_string()
                ),
            ]
        );
    }

    #[test]
    fn extracts_object_literal_aggregator_longhand() {
        let src = "const X = require('./x');\n\
                   module.exports = { Foo: X };";
        let got = extract_object_literal_exports_from_require(src);
        assert_eq!(got, vec![("Foo".to_string(), "./x".to_string())]);
    }

    #[test]
    fn extracts_object_literal_aggregator_mixed_with_skipped_entries() {
        // Computed keys, spreads, methods, and non-alias values are skipped.
        let src = "const A = require('./a');\n\
                   const B = require('./b');\n\
                   const C = makeThing();\n\
                   module.exports = { A, ...other, [key]: B, fn() {}, B, C, D: A };";
        let got = extract_object_literal_exports_from_require(src);
        assert_eq!(
            got,
            vec![
                ("A".to_string(), "./a".to_string()),
                ("B".to_string(), "./b".to_string()),
                ("D".to_string(), "./a".to_string()),
            ]
        );
    }

    #[test]
    fn skips_object_literal_aggregator_when_no_require_aliases() {
        let src = "module.exports = { foo: 1, bar: 'baz' };";
        let got = extract_object_literal_exports_from_require(src);
        assert!(got.is_empty(), "expected empty, got {:?}", got);
    }

    #[test]
    fn picks_last_module_exports_object_literal_assignment() {
        // When the file assigns `module.exports = {...}` twice, the later
        // assignment wins at runtime — and so does our static analysis.
        let src = "const A = require('./a');\n\
                   const B = require('./b');\n\
                   module.exports = { A };\n\
                   module.exports = { B };";
        let got = extract_object_literal_exports_from_require(src);
        assert_eq!(got, vec![("B".to_string(), "./b".to_string())]);
    }

    #[test]
    fn wrap_emits_direct_reexport_for_object_literal_aggregator() {
        // Issue #665: each alias is now also the import local (third pass
        // rename — needed so `class … extends RateLimiterMemory` in the
        // consumer picks up class identity via compile.rs's default-import
        // handler). The re-export targets the same name, so `<alias> as
        // <name>` is `RateLimiterMemory as RateLimiterMemory`.
        let src = "const RateLimiterMemory = require('./lib/RateLimiterMemory');\n\
                   const RateLimiterRedis = require('./lib/RateLimiterRedis');\n\
                   module.exports = { RateLimiterMemory, RateLimiterRedis };";
        let wrapped = wrap_commonjs(src, &PathBuf::from("/tmp/test.js"));
        assert!(
            wrapped.contains("export { RateLimiterMemory as RateLimiterMemory };"),
            "expected direct re-export of RateLimiterMemory, got:\n{}",
            wrapped
        );
        assert!(
            wrapped.contains("export { RateLimiterRedis as RateLimiterRedis };"),
            "expected direct re-export of RateLimiterRedis, got:\n{}",
            wrapped
        );
    }

    #[test]
    fn wrap_rewrites_module_exports_class_expression_named() {
        // Issue #665 (fifth pass): `module.exports = class Abstract { ... };`
        // (rate-limiter-flexible/lib/RateLimiterAbstract.js shape). The
        // expression is rewritten to declaration form so the existing
        // hoist + direct-default-export pipeline surfaces the class as a
        // module-scope binding, restoring class identity for the
        // consumer's `import RateLimiterAbstract from "..."`.
        let src = "module.exports = class Abstract {\n  hello() { return 1; }\n};";
        let wrapped = wrap_commonjs(src, &PathBuf::from("/tmp/abstract.js"));
        assert!(
            wrapped.contains("export default Abstract;"),
            "expected direct default export of Abstract, got:\n{}",
            wrapped
        );
        assert!(
            wrapped.contains("export { Abstract };"),
            "expected named re-export of Abstract for class identity, got:\n{}",
            wrapped
        );
        assert!(
            !wrapped.contains("export default _cjs;"),
            "should bypass _cjs for class-expression default export, got:\n{}",
            wrapped
        );
    }

    #[test]
    fn wrap_rewrites_module_exports_class_expression_with_extends() {
        // Class expressions with extends — the extends clause must survive
        // the rewrite so the consumer's class-identity propagation works
        // through the IIFE-emitted parent binding.
        let src = "var Base = require('./base');\n\
                   module.exports = class Child extends Base {\n  m() { return 2; }\n};";
        let wrapped = wrap_commonjs(src, &PathBuf::from("/tmp/child.js"));
        assert!(
            wrapped.contains("class Child extends Base {"),
            "expected hoisted declaration to keep extends clause, got:\n{}",
            wrapped
        );
        assert!(
            wrapped.contains("export default Child;"),
            "expected direct default export of Child, got:\n{}",
            wrapped
        );
        assert!(
            wrapped.contains("export { Child };"),
            "expected named re-export of Child, got:\n{}",
            wrapped
        );
    }

    #[test]
    fn wrap_rewrites_module_exports_anonymous_class_expression() {
        // Anonymous class expression — invent a synthetic name. The
        // important post-condition is that the default export is NOT
        // `_cjs` (which would hide class identity from compile.rs).
        let src = "module.exports = class { hello() { return 1; } };";
        let wrapped = wrap_commonjs(src, &PathBuf::from("/tmp/anon.js"));
        assert!(
            wrapped.contains("export default __perry_cjs_default__;"),
            "expected synthetic-named default export, got:\n{}",
            wrapped
        );
        assert!(
            !wrapped.contains("export default _cjs;"),
            "should bypass _cjs for anonymous class-expression default, got:\n{}",
            wrapped
        );
    }

    #[test]
    fn wrap_leaves_non_class_module_exports_alone() {
        // Don't fire on non-class RHS — preserves the existing IIFE
        // routing for `module.exports = <value>` shapes that aren't
        // classes (object literals, calls, identifiers, primitives, …).
        let src = "module.exports = 1 + 2;";
        let wrapped = wrap_commonjs(src, &PathBuf::from("/tmp/scalar.js"));
        assert!(
            wrapped.contains("export default _cjs;"),
            "should keep _cjs default for non-class RHS, got:\n{}",
            wrapped
        );
    }

    #[test]
    fn wrap_skips_class_expression_rewrite_with_conflicting_module_exports() {
        // Multiple top-level `module.exports = ...` lines defeat the
        // single-target invariant; fall back to `_cjs` so last-assignment-
        // wins runtime semantics are preserved.
        let src = "module.exports = class Foo { m() {} };\n\
                   module.exports = somethingElse;";
        let wrapped = wrap_commonjs(src, &PathBuf::from("/tmp/conflict.js"));
        assert!(
            wrapped.contains("export default _cjs;"),
            "expected _cjs default when conflicting module.exports lines exist, got:\n{}",
            wrapped
        );
        assert!(
            !wrapped.contains("export default Foo;"),
            "should not direct-export the first-assignment class, got:\n{}",
            wrapped
        );
    }

    #[test]
    fn wrap_skips_class_expression_rewrite_on_name_collision() {
        // If a `class <SameName>` declaration already exists at top level,
        // refuse the rewrite — emitting the declaration form again would
        // duplicate the binding. Falls back to `_cjs` for default export.
        let src = "class Foo { existing() {} }\n\
                   module.exports = class Foo { conflict() {} };";
        let wrapped = wrap_commonjs(src, &PathBuf::from("/tmp/collide.js"));
        assert!(
            wrapped.contains("export default _cjs;"),
            "expected _cjs default on name collision, got:\n{}",
            wrapped
        );
    }

    #[test]
    fn require_alias_extract_skips_trailing_member_access() {
        // Issue #845 — mysql2 sub-bug 2.
        //
        // `const EventEmitter = require('events').EventEmitter;` binds the
        // class, not the module object. The old regex matched it as
        // `const EventEmitter = require('events')` (optional-`;?` stopping
        // at `)`) and the blanking pass at wrap_commonjs left `.EventEmitter;`
        // dangling at column 0 of the wrapped output — TS1109 parse error
        // 1000+ bytes past the original-file EOF.
        let src = "class B extends EventEmitter { }\n\
                   const EventEmitter = require('events').EventEmitter;\n\
                   const Readable = require('stream').Readable;\n\
                   const Net = require('net');\n";
        let aliases = extract_require_aliases_with_ranges(src);
        // Only `Net` is a whole-statement alias; the other two have
        // trailing `.X` and must be skipped.
        assert_eq!(
            aliases.len(),
            1,
            "expected 1 whole-statement alias, got: {:?}",
            aliases
        );
        assert_eq!(aliases[0].0, "Net");
        assert_eq!(aliases[0].1, "net");
    }

    #[test]
    fn wrap_does_not_dangle_member_access_after_blanking() {
        // Regression test for issue #845: the wrap output must remain
        // parseable when a require() has `.X` member access after it,
        // even in the presence of top-level class declarations (which is
        // what triggers the blanking pass).
        let src = "const EventEmitter = require('events').EventEmitter;\n\
                   class BaseConnection extends EventEmitter {\n\
                     constructor() { super(); }\n\
                   }\n\
                   module.exports = BaseConnection;\n";
        let wrapped = wrap_commonjs(src, &PathBuf::from("/tmp/test.js"));
        // The post-wrap source must NOT contain a stray `.EventEmitter`
        // sitting at column 0 (or anywhere outside a valid expression).
        // The simplest invariant: every `.EventEmitter` occurrence must
        // be preceded by either `_req` (the inner require dispatch) or
        // a non-whitespace, non-newline byte (a valid receiver).
        for (i, _) in wrapped.match_indices(".EventEmitter") {
            let prev_char = wrapped[..i].chars().rev().next().unwrap_or(' ');
            assert!(
                prev_char.is_alphanumeric()
                    || prev_char == '_'
                    || prev_char == '$'
                    || prev_char == ')',
                ".EventEmitter at byte {} has invalid receiver {:?} — would parse-fail:\n{}",
                i,
                prev_char,
                wrapped
            );
        }
        // And it should parse cleanly through SWC.
        let parsed = perry_parser::parse_typescript(&wrapped, "test.js");
        assert!(
            parsed.is_ok(),
            "wrap output failed to parse: {:?}\nwrapped:\n{}",
            parsed.err(),
            wrapped
        );
    }

    #[test]
    fn extract_exports_skips_default_reserved_word() {
        // Issue #845 — pino: `module.exports.default = pino` flows into the
        // named-export loop and pre-fix emitted `export const default =
        // _cjs.default;` (invalid syntax — `default` is a reserved word).
        // The named-export path must skip reserved words; the separate
        // `export default _cjs;` machinery covers the default export.
        let src = "module.exports = function pino(){};\n\
                   module.exports.default = function pino(){};\n\
                   module.exports.transport = require('./transport');\n\
                   module.exports.version = '1.0';\n";
        let names = extract_exports_from_source(src);
        assert!(
            !names.contains(&"default".to_string()),
            "must skip `default`, got: {:?}",
            names
        );
        assert!(names.contains(&"transport".to_string()));
        assert!(names.contains(&"version".to_string()));
    }

    #[test]
    fn wrap_pino_shape_parses_cleanly() {
        // Issue #845 — pino sub-bug: end-to-end check that a pino-shaped
        // CJS module produces parseable wrap output.
        let src = "function pino() { return {}; }\n\
                   module.exports = pino;\n\
                   module.exports.default = pino;\n\
                   module.exports.pino = pino;\n\
                   module.exports.version = '1.0';\n";
        let wrapped = wrap_commonjs(src, &PathBuf::from("/tmp/pino.js"));
        assert!(
            !wrapped.contains("export const default"),
            "must not emit `export const default` (reserved word), got:\n{}",
            wrapped
        );
        let parsed = perry_parser::parse_typescript(&wrapped, "pino.js");
        assert!(
            parsed.is_ok(),
            "pino wrap failed to parse: {:?}\nwrapped:\n{}",
            parsed.err(),
            wrapped
        );
    }
}
