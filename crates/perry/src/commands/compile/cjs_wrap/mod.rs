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

mod detect;
mod extract_exports;
mod extract_requires;
mod hoist_classes;
mod wrap;

// Cross-sibling helpers — siblings reach for these via `use super::*;`.
pub(self) use detect::is_js_reserved_word;
pub(self) use extract_exports::{
    extract_exports_from_source, extract_named_exports_from_require,
    extract_object_literal_exports_from_require, extract_single_module_exports_assignment,
    module_reexport_specs,
};
pub(self) use extract_requires::{
    extract_export_star_specs, extract_require_aliases_with_ranges, extract_require_specifiers,
    function_local_specs, identifier_is_reassigned,
};
pub(self) use hoist_classes::{
    extract_top_level_class_decls, rewrite_module_exports_class_expression,
    source_has_top_level_return, top_level_class_names,
};

// Public API consumed by `compile.rs` / `collect_modules.rs`.
pub(super) use detect::is_commonjs;
pub(super) use wrap::{wrap_commonjs_for_target, wrap_commonjs_with_body_offset};

#[cfg(test)]
mod tests {
    use super::detect::is_commonjs;
    use super::extract_exports::{
        extract_exports_from_source, extract_named_exports_from_require,
        extract_object_literal_exports_from_require, extract_single_module_exports_assignment,
        module_reexport_specs,
    };
    use super::extract_requires::{
        extract_require_aliases_with_ranges, extract_require_specifiers,
    };
    use super::hoist_classes::{
        extract_top_level_class_decls, source_has_top_level_return, top_level_class_names,
    };
    use super::wrap::{wrap_commonjs, wrap_commonjs_for_target, wrap_commonjs_with_body_offset};
    use std::fs;
    use std::path::PathBuf;

    // #5247: the wrapped output must report where the ORIGINAL body begins, and
    // because blanking/hoisting preserve newlines, the prefix line count lets a
    // wrapped body line map back to its original-source line. This is the unit
    // that backs the `--debug-symbols` CJS-wrap coordinate correction.
    #[test]
    fn cjs_wrap_body_offset_maps_back_to_original_line() {
        // Original body: `function f(){...}` on line 1, `module.exports = f`
        // on line 3. A throw inside f (wrapped line L) must map to original
        // line `L - prefix_line_count`.
        let original = "function f() {\n  return new Nope();\n}\nmodule.exports = f;\n";
        let path = PathBuf::from("/tmp/x/index.js");
        let (wrapped, body_off) = wrap_commonjs_with_body_offset(original, &path, None);
        let body_off = body_off.expect("body should be locatable in wrapped output");
        // Prefix line count = newlines before the body in the wrapped output.
        let prefix_lines = wrapped.as_bytes()[..body_off]
            .iter()
            .filter(|&&b| b == b'\n')
            .count();
        // The `return new Nope();` line is original line 2. Find its wrapped
        // line and confirm subtracting the prefix recovers line 2.
        let needle_off = wrapped.find("return new Nope();").unwrap();
        let wrapped_line = 1 + wrapped.as_bytes()[..needle_off]
            .iter()
            .filter(|&&b| b == b'\n')
            .count();
        assert_eq!(wrapped_line - prefix_lines, 2);
    }

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
    fn require_only_file_with_import_word_in_comment_is_cjs() {
        // Next.js `setup-node-env.external.js`: pure side-effect requires,
        // but the header comment contains the word "import". The comment
        // must not flip classification to ESM.
        let src = r#"// This is a minimal import that initializes the node environment
"use strict";
if (process.env.NEXT_RUNTIME !== 'edge') {
    require('next/dist/server/node-environment');
}
"#;
        assert!(
            is_commonjs(src),
            "comment text must not defeat require( arm"
        );
    }

    #[test]
    fn template_literal_esm_codegen_is_still_cjs() {
        // next/dist/build/utils.js writes an ESM server.js via a template
        // literal whose column-0 `import path from 'node:path'` line must
        // not flip this CJS file to the ESM pipeline.
        let src = "\"use strict\";\nObject.defineProperty(exports, \"__esModule\", { value: true });\nexports.write = function() {\n  return `performance.mark('next-start');\nimport path from 'node:path'\nimport module from 'node:module'\n`;\n};\n";
        assert!(
            is_commonjs(src),
            "template-literal import must not defeat CJS detection"
        );
    }

    #[test]
    fn nested_template_interpolation_stays_masked() {
        // next/dist/build/utils.js shape: an outer template whose `${…}`
        // interpolation contains NESTED templates with column-0 `import`
        // lines. The whole construct must stay masked as string content.
        let src = "\"use strict\";\nexports.write = (m) => {\n  return `${m ? `x\nimport path from 'node:path'\n` : `const path = require('path')`}\nrest`;\n};\n";
        assert!(
            is_commonjs(src),
            "nested template import lines must not defeat CJS detection"
        );
    }

    #[test]
    fn regex_with_quote_does_not_mask_trailing_module_exports() {
        // comment-json's bundle shape: regex literals containing quotes
        // followed by the real `module.exports=` tail. The stripper must
        // track regex literals or the tail is masked as string content.
        let src = "const e = s.split(/['\"]/);\nvar i = make();\nmodule.exports = i;\n";
        assert!(
            is_commonjs(src),
            "regex with quote must not hide module.exports"
        );
    }

    #[test]
    fn require_in_string_only_is_not_cjs() {
        // `require(` appearing only inside a string literal is not evidence
        // of CommonJS.
        let src = "const msg = \"call require('x') yourself\";\nconsole.log(msg);\n";
        assert!(!is_commonjs(src));
    }

    #[test]
    fn empty_file_is_cjs() {
        // Marker packages (react's `client-only`) ship a 0-byte index.js;
        // its default import must resolve to the wrap's empty exports
        // object, so empty/whitespace-only sources count as CommonJS.
        assert!(is_commonjs(""));
        assert!(is_commonjs("  \n\t\n"));
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
    fn issue_5275_detects_bracket_module_exports() {
        // @colors/colors/lib/custom/trap.js shape: bracket default export.
        assert!(is_commonjs("module['exports'] = function runTheTrap() {};"));
        assert!(is_commonjs(
            "module[\"exports\"] = function runTheTrap() {};"
        ));
    }

    #[test]
    fn issue_5275_detects_bracket_named_exports() {
        assert!(is_commonjs("exports['foo'] = 1;"));
        assert!(is_commonjs("exports[\"foo\"] = 1;"));
    }

    #[test]
    fn issue_5275_dynamic_bracket_key_is_not_cjs_on_its_own() {
        // A genuinely dynamic `module[k] = …` (non-literal key) is not a CJS
        // export signal — without other CJS tokens this stays ESM.
        assert!(!is_commonjs("const k = 'x';\nmodule[k] = 1;\n"));
    }

    #[test]
    fn issue_5275_extracts_bracket_named_exports() {
        let src = "exports['foo'] = 1;\nexports[\"bar\"] = function(){};";
        let names = extract_exports_from_source(src);
        assert_eq!(names, vec!["foo".to_string(), "bar".to_string()]);
    }

    #[test]
    fn issue_5275_extracts_bracket_module_exports_dot_named() {
        let src = "module.exports['foo'] = 1;";
        let names = extract_exports_from_source(src);
        assert_eq!(names, vec!["foo".to_string()]);
    }

    #[test]
    fn issue_5275_does_not_extract_dynamic_bracket_key() {
        // `exports[k] = …` with a non-string-literal key must not surface a
        // named export.
        let src = "const k = 'x';\nexports[k] = 1;";
        let names = extract_exports_from_source(src);
        assert!(names.is_empty(), "expected no names, got {:?}", names);
    }

    #[test]
    fn issue_5275_single_module_exports_accepts_bracket_form() {
        let src = "class Child {}\nmodule['exports'] = Child;";
        assert_eq!(
            extract_single_module_exports_assignment(src),
            Some("Child".to_string())
        );
        let src2 = "class Child {}\nmodule[\"exports\"] = Child;";
        assert_eq!(
            extract_single_module_exports_assignment(src2),
            Some("Child".to_string())
        );
    }

    #[test]
    fn issue_5275_wrap_default_export_for_bracket_module_exports() {
        // The mb repro: `module['exports'] = function greet(){}`. The IIFE
        // runs the bracket assignment, so `export default _cjs;` resolves to
        // the function — but the file MUST be wrapped first (detection).
        let src = "module['exports'] = function greet(n) { return n; };";
        assert!(is_commonjs(src));
        let wrapped = wrap_commonjs(src, &PathBuf::from("/tmp/mb/index.js"));
        assert!(
            wrapped.contains("export default _cjs;"),
            "expected default export through _cjs, got:\n{}",
            wrapped
        );
        assert!(
            perry_parser::parse_typescript(&wrapped, "mb/index.js").is_ok(),
            "wrapped bracket-export module must parse, got:\n{}",
            wrapped
        );
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
    fn wrap_module_and_exports_are_reassignable_vars() {
        // #3527: a CJS body may rebind `module`/`exports` (iconv-lite's
        // `for (...) { var module = modules[i]; mergeModules(exports, module); }`).
        // The wrapper must expose them as reassignable `var`s — not a `const`
        // the body would silently fail to rebind — while reading the real
        // exports back from a stable, body-untouchable `__cjs_module`.
        let src = "exports.foo = 42;";
        let wrapped = wrap_commonjs(src, &PathBuf::from("/tmp/test.js"));
        assert!(
            wrapped.contains("const __cjs_module = { exports: {} };"),
            "expected stable __cjs_module, got:\n{}",
            wrapped
        );
        assert!(
            wrapped.contains("var module = __cjs_module;"),
            "expected reassignable `var module`, got:\n{}",
            wrapped
        );
        assert!(
            wrapped.contains("var exports = __cjs_module.exports;"),
            "expected reassignable `var exports`, got:\n{}",
            wrapped
        );
        assert!(
            wrapped.contains("return __cjs_module.exports;"),
            "export must be read from the stable ref, got:\n{}",
            wrapped
        );
        // The body must NOT re-collide with a `const module`/`const exports`.
        assert!(!wrapped.contains("const module = "));
        assert!(!wrapped.contains("const exports = "));
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
    fn wrap_keeps_reassigned_require_alias_as_mutable_local() {
        // Issue #5006: a `require()`-initialized alias that is later
        // *reassigned* (the signal-exit `signals = signals.filter(...)` shape)
        // must NOT be hoisted into an immutable `import s from '...'` with its
        // declaration blanked — that makes the reassignment unresolvable
        // (`ReferenceError: s is not defined`). It must stay a real mutable
        // local fed by the `_req_N` import.
        let src = "var s = require('./data.js');\ns = s.filter(function () { return true; });\nmodule.exports = s;";
        let wrapped = wrap_commonjs(src, &PathBuf::from("/tmp/test.js"));
        // Falls back to the placeholder import name (alias not adopted)...
        assert!(
            wrapped.contains("import _req_0 from './data.js';"),
            "expected non-adopted _req_0 import, got:\n{}",
            wrapped
        );
        // ...the require dispatches through it...
        assert!(
            wrapped.contains("if (specifier === './data.js') return _req_0;"),
            "expected require dispatch through _req_0, got:\n{}",
            wrapped
        );
        // ...and the original `var s = require('./data.js')` declaration stays
        // in the IIFE body (not blanked) so `s` is a mutable local.
        assert!(
            wrapped.contains("var s = require('./data.js');"),
            "expected the alias declaration to survive as a mutable local, got:\n{}",
            wrapped
        );
    }

    #[test]
    fn wrap_does_not_adopt_alias_that_collides_with_named_export() {
        // Regression: pino.js does `const symbols = require('./lib/symbols')`
        // AND `module.exports.symbols = symbols`. Adopting the `symbols` alias
        // as the import local (`import symbols from './lib/symbols';`) collided
        // with the module-scope `export const symbols = _cjs.symbols;` the wrap
        // emits for the named export. HIR bound the IIFE-body reference
        // `const { ... } = symbols` to the `export const` (value `_cjs.symbols`,
        // `undefined` until the IIFE returns), so the top-level destructure
        // threw `Cannot convert undefined or null to object` (pino.js:23).
        //
        // The fix refuses to adopt an alias whose name is also a plain named
        // export: the spec stays on `_req_N`, the body's `const symbols =
        // require(...)` survives as an IIFE-local, and the module-scope
        // `export const symbols` no longer collides.
        let src = "const symbols = require('./lib/symbols');\n\
                   const { aSym, bSym } = symbols;\n\
                   function build() { return [aSym, bSym]; }\n\
                   module.exports = build;\n\
                   module.exports.symbols = symbols;";
        let wrapped = wrap_commonjs(src, &PathBuf::from("/tmp/pkg/index.js"));
        // Alias NOT adopted — import keeps the `_req_N` placeholder name...
        assert!(
            wrapped.contains("import _req_0 from './lib/symbols';"),
            "expected non-adopted _req_0 import (no `import symbols`), got:\n{}",
            wrapped
        );
        assert!(
            !wrapped.contains("import symbols from './lib/symbols';"),
            "must NOT adopt the colliding `symbols` alias as the import local, got:\n{}",
            wrapped
        );
        // ...the require dispatches through it...
        assert!(
            wrapped.contains("if (specifier === './lib/symbols') return _req_0;"),
            "expected require dispatch through _req_0, got:\n{}",
            wrapped
        );
        // ...the original `const symbols = require(...)` survives in the IIFE
        // body (NOT blanked) so the destructure reads the real value...
        assert!(
            wrapped.contains("const symbols = require('./lib/symbols');"),
            "expected the alias declaration to survive as an IIFE-local, got:\n{}",
            wrapped
        );
        // ...and the named export still surfaces.
        assert!(
            wrapped.contains("export const symbols = _cjs.symbols;"),
            "expected the named export to be preserved, got:\n{}",
            wrapped
        );
    }

    #[test]
    fn identifier_is_reassigned_distinguishes_declaration_from_write() {
        use super::extract_requires::identifier_is_reassigned;
        // Pure read-only alias: declaration + member reads only.
        assert!(!identifier_is_reassigned(
            "var dep = require('./dep'); module.exports = dep.value;",
            "dep"
        ));
        // Reassignment.
        assert!(identifier_is_reassigned(
            "var s = require('./d'); s = s.filter(() => true);",
            "s"
        ));
        // Compound assignment.
        assert!(identifier_is_reassigned(
            "var n = require('./n'); n += 1;",
            "n"
        ));
        // Comparisons / arrows / member writes must not count as reassignment.
        assert!(!identifier_is_reassigned(
            "var s = require('./d'); if (s === other) {} obj.s = 1; cb(() => s);",
            "s"
        ));
    }

    #[test]
    fn wrap_prunes_dead_process_platform_require_for_windows_target() {
        let src = r#"
var terminalCtor;
if (process.platform === 'win32') {
    terminalCtor = require('./windowsTerminal').WindowsTerminal;
}
else {
    terminalCtor = require('./unixTerminal').UnixTerminal;
}
exports.spawn = function spawn() { return terminalCtor; };
"#;
        let wrapped = wrap_commonjs_for_target(
            src,
            &PathBuf::from("/tmp/node_modules/node-pty/lib/index.js"),
            Some("windows"),
        );
        assert!(
            wrapped.contains("import _req_0 from './windowsTerminal';")
                || wrapped.contains("import terminalCtor from './windowsTerminal';"),
            "expected live Windows require to stay hoisted, got:\n{}",
            wrapped
        );
        assert!(
            !wrapped.contains("from './unixTerminal'"),
            "dead Unix require must not become an eager ESM import on Windows, got:\n{}",
            wrapped
        );
        assert!(
            !wrapped.contains("if (specifier === './unixTerminal')"),
            "dead Unix require must not be dispatchable on Windows, got:\n{}",
            wrapped
        );
    }

    #[test]
    fn wrap_prunes_dead_process_platform_require_for_linux_target() {
        let src = r#"
var terminalCtor;
if (process.platform === 'win32') {
    terminalCtor = require('./windowsTerminal').WindowsTerminal;
}
else {
    terminalCtor = require('./unixTerminal').UnixTerminal;
}
exports.spawn = function spawn() { return terminalCtor; };
"#;
        let wrapped = wrap_commonjs_for_target(
            src,
            &PathBuf::from("/tmp/node_modules/node-pty/lib/index.js"),
            Some("linux"),
        );
        assert!(
            wrapped.contains("from './unixTerminal'"),
            "expected live Unix require to stay hoisted, got:\n{}",
            wrapped
        );
        assert!(
            !wrapped.contains("from './windowsTerminal'"),
            "dead Windows require must not become an eager ESM import on Linux, got:\n{}",
            wrapped
        );
    }

    #[test]
    fn wrap_rewrites_depd_dynamic_deprecation_wrapper() {
        let src = r#"function wrapfunction (fn, message) {
  var args = createArgumentsString(fn.length)
  var stack = getStack()
  var site = callSiteLocation(stack[1])

  site.name = fn.name

  // eslint-disable-next-line no-new-func
  var deprecatedfn = new Function('fn', 'log', 'deprecate', 'message', 'site',
    '"use strict"\n' +
    'return function (' + args + ') {' +
    'log.call(deprecate, message, site)\n' +
    'return fn.apply(this, arguments)\n' +
    '}')(fn, log, this, message, site)

  return deprecatedfn
}
module.exports = wrapfunction;"#;
        let wrapped = wrap_commonjs(src, &PathBuf::from("/tmp/app/node_modules/depd/index.js"));
        assert!(
            !wrapped.contains("new Function"),
            "depd dynamic wrapper must be compiled as a normal closure, got:\n{}",
            wrapped
        );
        assert!(
            wrapped.contains("return function () {"),
            "expected arity-erased wrapper closure, got:\n{}",
            wrapped
        );
        assert!(
            wrapped.contains("return fn.apply(this, arguments)"),
            "wrapper must preserve this/arguments forwarding, got:\n{}",
            wrapped
        );
        assert!(
            perry_parser::parse_typescript(&wrapped, "depd/index.js").is_ok(),
            "wrapped depd source must parse, got:\n{}",
            wrapped
        );
    }

    #[test]
    fn wrap_rewrites_function_bind_dynamic_wrapper() {
        let src = r#"module.exports = function bind(that) {
    var target = this;
    var args = slicy(arguments, 1);

    var bound;
    var binder = function () {
        if (this instanceof bound) {
            var result = target.apply(
                this,
                concatty(args, arguments)
            );
            if (Object(result) === result) {
                return result;
            }
            return this;
        }
        return target.apply(
            that,
            concatty(args, arguments)
        );

    };

    var boundLength = max(0, target.length - args.length);
    var boundArgs = [];
    for (var i = 0; i < boundLength; i++) {
        boundArgs[i] = '$' + i;
    }

    bound = Function('binder', 'return function (' + joiny(boundArgs, ',') + '){ return binder.apply(this,arguments); }')(binder);

    return bound;
};"#;
        let wrapped = wrap_commonjs(
            src,
            &PathBuf::from("/tmp/app/node_modules/function-bind/implementation.js"),
        );
        assert!(
            !wrapped.contains("Function('binder'"),
            "function-bind dynamic wrapper must be compiled as a normal closure, got:\n{}",
            wrapped
        );
        assert!(
            wrapped.contains("bound = function () {"),
            "expected arity-erased bound closure, got:\n{}",
            wrapped
        );
        assert!(
            wrapped.contains("return binder.apply(this, arguments);"),
            "wrapper must preserve this/arguments forwarding, got:\n{}",
            wrapped
        );
        assert!(
            perry_parser::parse_typescript(&wrapped, "function-bind/implementation.js").is_ok(),
            "wrapped function-bind source must parse, got:\n{}",
            wrapped
        );
    }

    #[test]
    fn wrap_rewrites_safer_buffer_private_binding_probe() {
        let src = r#"var safer = {}

if (!safer.kStringMaxLength) {
  try {
    safer.kStringMaxLength = process.binding('buffer').kStringMaxLength
  } catch (e) {
    // we can't determine kStringMaxLength in environments where process.binding
    // is unsupported, so let's not set it
  }
}

module.exports = safer;"#;
        let wrapped = wrap_commonjs(
            src,
            &PathBuf::from("/tmp/app/node_modules/safer-buffer/safer.js"),
        );
        assert!(
            !wrapped.contains("process.binding"),
            "safer-buffer private binding probe must be rewritten, got:\n{}",
            wrapped
        );
        assert!(
            wrapped.contains("safer.kStringMaxLength = 536870888"),
            "expected public max string length constant, got:\n{}",
            wrapped
        );
        assert!(
            perry_parser::parse_typescript(&wrapped, "safer-buffer/safer.js").is_ok(),
            "wrapped safer-buffer source must parse, got:\n{}",
            wrapped
        );
    }

    #[test]
    fn wrap_rewrites_safe_buffer_slow_buffer_fallback() {
        let src = r#"var buffer = require('buffer')
var Buffer = buffer.Buffer

SafeBuffer.allocUnsafeSlow = function (size) {
  if (typeof size !== 'number') {
    throw new TypeError('Argument must be a number')
  }
  return buffer.SlowBuffer(size)
}

module.exports = SafeBuffer;"#;
        let wrapped = wrap_commonjs(
            src,
            &PathBuf::from("/tmp/app/node_modules/safe-buffer/index.js"),
        );
        assert!(
            !wrapped.contains("buffer.SlowBuffer"),
            "safe-buffer fallback must avoid deprecated SlowBuffer, got:\n{}",
            wrapped
        );
        assert!(
            wrapped.contains("return Buffer.allocUnsafeSlow(size)"),
            "expected Buffer.allocUnsafeSlow fallback, got:\n{}",
            wrapped
        );
        assert!(
            perry_parser::parse_typescript(&wrapped, "safe-buffer/index.js").is_ok(),
            "wrapped safe-buffer source must parse, got:\n{}",
            wrapped
        );
    }

    #[test]
    fn issue_5251_class_reading_exports_stays_in_iife() {
        // #5251: a top-level class whose body reads the cjs_wrap-injected
        // `exports` binding (`exports.X` inside a method/ctor) must NOT be
        // hoisted out of the IIFE — hoisting severs its closure over the
        // injected `var exports`, so `exports.X` resolves as an unknown
        // global and lowers to the numeric `0` sentinel inside class methods.
        let src = "\"use strict\";\nexports.TAG = \"hi\";\nclass C { greet() { return exports.TAG + \"!\"; } }\nexports.mk = function () { return new C(); };\n";
        let wrapped = wrap_commonjs(src, &PathBuf::from("/tmp/app/node_modules/re/index.js"));
        let iife_start = wrapped
            .find("const _cjs = (function() {")
            .expect("expected an IIFE wrap (no flat default class), got:\n");
        let class_pos = wrapped
            .find("class C ")
            .expect("class C must survive in the wrapped output");
        assert!(
            class_pos > iife_start,
            "class reading `exports` must stay INSIDE the IIFE (after its \
             opener), not hoisted above it, got:\n{}",
            wrapped
        );
        assert!(
            perry_parser::parse_typescript(&wrapped, "re/index.js").is_ok(),
            "wrapped module must parse, got:\n{}",
            wrapped
        );
    }

    #[test]
    fn issue_5251_class_without_exports_still_hoists() {
        // Regression guard: a top-level class that does NOT touch the injected
        // `exports`/`module`/`require` bindings must still hoist above the
        // IIFE (so `import { D } from "pkg"` resolves to the real class).
        let src = "\"use strict\";\nclass D { val() { return 42; } }\nexports.mkD = function () { return new D(); };\n";
        let wrapped = wrap_commonjs(src, &PathBuf::from("/tmp/app/node_modules/re/index.js"));
        let iife_start = wrapped
            .find("const _cjs = (function() {")
            .expect("expected an IIFE wrap, got:\n");
        let class_pos = wrapped
            .find("class D ")
            .expect("class D must survive in the wrapped output");
        assert!(
            class_pos < iife_start,
            "a class not referencing exports/module/require must still hoist \
             above the IIFE, got:\n{}",
            wrapped
        );
    }

    #[test]
    fn issue_1721_blanks_adopted_alias_require_in_body() {
        // #1721: `const c = require('./common')` adopts `c` as the import
        // local name (so `import c from './common'`). The original body line
        // MUST be blanked — otherwise it redeclares `c` inside the IIFE and
        // the synthetic `require` (which returns `c`) resolves to that inner,
        // not-yet-initialized binding, so the consumer's
        // `const c = require('./common')` lands `undefined`. Regression:
        // before the fix this only happened when hoisting classes.
        let src = "const c = require('./common');\nconsole.log(c.x);";
        let wrapped = wrap_commonjs(src, &PathBuf::from("/tmp/test.js"));
        assert!(
            wrapped.contains("import c from './common';"),
            "expected hoisted import using the alias, got:\n{}",
            wrapped
        );
        assert!(
            !wrapped.contains("require('./common')") || !wrapped.contains("const c = require"),
            "adopted-alias body line must be blanked so it can't shadow the \
             import inside the IIFE, got:\n{}",
            wrapped
        );
        assert!(
            wrapped.contains("console.log(c.x);"),
            "body references to the binding must survive, got:\n{}",
            wrapped
        );
        // Sanity: the rewritten module still parses.
        assert!(
            perry_parser::parse_typescript(&wrapped, "test.js").is_ok(),
            "wrapped module must parse, got:\n{}",
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
    fn wrap_flat_emits_class_module_exports_that_closes_over_top_level_const() {
        // Issue #4933: `module.exports = StackUtils` where the class reads a
        // top-level `const` (so the #2310 hoist guard refuses to lift it). The
        // old path degraded to `export default _cjs`, losing class identity —
        // statics, `.prototype`, and the closure all read `undefined` on the
        // consumer side. The flat path drops the IIFE so the class stays a real
        // top-level declaration with full identity.
        let src = "const natives = ['a', 'b'];\n\
                   class StackUtils {\n\
                     static nodeInternals() { return natives.slice(); }\n\
                     clean(s) { return 'x' + s; }\n\
                   }\n\
                   module.exports = StackUtils;";
        let wrapped = wrap_commonjs(src, &PathBuf::from("/tmp/test.js"));
        assert!(
            wrapped.contains("export default StackUtils;"),
            "expected direct default export of StackUtils, got:\n{}",
            wrapped
        );
        assert!(
            wrapped.contains("export { StackUtils };"),
            "expected named export of StackUtils, got:\n{}",
            wrapped
        );
        assert!(
            !wrapped.contains("export default _cjs;"),
            "flat emission must not fall back to the opaque _cjs default, got:\n{}",
            wrapped
        );
        assert!(
            !wrapped.contains("const _cjs = (function()"),
            "flat emission must drop the IIFE wrapper, got:\n{}",
            wrapped
        );
        // The CommonJS runtime shims still run at module scope.
        assert!(wrapped.contains("const __cjs_module = { exports: {} };"));
        assert!(wrapped.contains("const _cjs = __cjs_module.exports;"));
    }

    #[test]
    fn top_level_class_names_lists_refused_and_hoisted_classes() {
        let src = "const t = 1;\nclass A { m(){ return t; } }\nclass B {}\n";
        let names = top_level_class_names(src);
        assert_eq!(names, vec!["A".to_string(), "B".to_string()]);
    }

    #[test]
    fn top_level_return_detection_ignores_returns_inside_bodies_and_regexes() {
        // No top-level return: every `return` sits inside a function/class body,
        // and the regex literal's brackets must not corrupt brace depth.
        let no_return = "const re = /^(.*?) \\[as (.*?)\\]$/;\n\
                         class C {\n\
                           m() { if (true) { return 1; } return 2; }\n\
                         }\n\
                         module.exports = C;";
        assert!(
            !source_has_top_level_return(no_return),
            "function-body returns must not count as top-level"
        );
        // A genuine module-top return keeps the IIFE.
        let yes_return = "if (!supported) return;\nmodule.exports = {};";
        assert!(source_has_top_level_return(yes_return));
    }

    #[test]
    fn wrap_keeps_iife_for_class_module_exports_with_top_level_return() {
        // A top-level `return` is legal in CommonJS but not at ESM module scope,
        // so the IIFE wrap must be retained even for `module.exports = <Class>`.
        let src = "const t = 1;\n\
                   if (!t) return;\n\
                   class C { m(){ return t; } }\n\
                   module.exports = C;";
        let wrapped = wrap_commonjs(src, &PathBuf::from("/tmp/test.js"));
        assert!(
            wrapped.contains("const _cjs = (function()"),
            "module with a top-level return must keep the IIFE, got:\n{}",
            wrapped
        );
    }

    #[test]
    fn wrap_keeps_cjs_default_when_module_exports_is_object_literal() {
        let src = "module.exports = { foo: 1, bar: 2 };";
        let wrapped = wrap_commonjs(src, &PathBuf::from("/tmp/test.js"));
        assert!(wrapped.contains("export default _cjs;"));
    }

    #[test]
    fn wrap_copies_named_exports_from_extensionless_reexport_target() {
        let tmp = tempfile::tempdir().expect("tmpdir");
        let lib_dir = tmp.path().join("lib");
        fs::create_dir_all(&lib_dir).expect("mkdir lib");
        fs::write(
            lib_dir.join("index.js"),
            "module.exports.parse = function parse() {};",
        )
        .expect("write target");

        let entry = tmp.path().join("index.js");
        let src = "module.exports = require('./lib/index');";
        let wrapped = wrap_commonjs(src, &entry);

        assert!(
            wrapped.contains("export const parse = _cjs.parse;"),
            "expected named export copied through extensionless CJS re-export, got:\n{}",
            wrapped
        );
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
    fn wrap_preserves_regex_control_unicode_escapes() {
        // Undici 8's lib/web/infra/index.js contains this CJS-body regex.
        // Perry normalizes Unicode identifier escapes before SWC parses; the
        // normalizer must not turn regex char-class escapes into source text.
        let src = "'use strict'\n\
                   const ASCII_WHITESPACE_REPLACE_REGEX = /[\\u0009\\u000A\\u000C\\u000D\\u0020]/g // eslint-disable-line no-control-regex\n\
                   if (!ASCII_WHITESPACE_REPLACE_REGEX.test(' ')) {\n\
                     throw new Error('unexpected regex result')\n\
                   }\n\
                   module.exports = ASCII_WHITESPACE_REPLACE_REGEX;\n";
        let wrapped = wrap_commonjs(src, &PathBuf::from("/tmp/undici-infra.js"));
        let parsed = perry_parser::parse_typescript(&wrapped, "undici-infra.js");

        assert!(
            parsed.is_ok(),
            "undici-style CJS regex wrap failed to parse: {:?}\nwrapped:\n{}",
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
    fn extract_exports_skips_inner_module_exports_param() {
        // next/dist/compiled/p-queue: webpack/ncc inner modules write to their
        // OWN exports object (`e.exports.X = …`), which is not a named export
        // of the outer bundle. Pre-fix the dot-boundary regex matched it, the
        // wrap emitted `export const TimeoutError = _cjs.TimeoutError;` at
        // module scope, and that const shadowed the inner class binding —
        // every inner reference to `TimeoutError` became undefined.
        let src = "var mods = { 816: (e, t, n) => {\n\
                       class TimeoutError extends Error {}\n\
                       const pTimeout = (p) => p;\n\
                       e.exports = pTimeout;\n\
                       e.exports.str = 'hello';\n\
                       e.exports.TimeoutError = TimeoutError;\n\
                   }};\n\
                   exports.real = 1;\n\
                   module.exports.alsoReal = 2;\n";
        let names = extract_exports_from_source(src);
        assert!(
            !names.contains(&"TimeoutError".to_string()),
            "`e.exports.X` is an inner module's exports, not ours: {:?}",
            names
        );
        assert!(!names.contains(&"str".to_string()), "got: {:?}", names);
        assert!(names.contains(&"real".to_string()));
        assert!(names.contains(&"alsoReal".to_string()));
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

    /// Issue #2310 / #4933 — a top-level class body that references a
    /// let/const declared at the IIFE's top level (the ws/lib/sender.js shape:
    /// `let pointer; class Sender { static next(){ … pointer++ } }`) cannot be
    /// *hoisted* above the IIFE — that would sever the closure and the compile
    /// hard-errors with `Undefined variable in update expression`.
    ///
    /// For a `module.exports = Sender` default-export class, the #4933 flat
    /// emission supersedes the old IIFE-retention mitigation: dropping the IIFE
    /// puts BOTH the class and `let pointer` at module scope, so the closure
    /// (including the `pointer++` mutation) survives AND the class keeps full
    /// identity — the consumer's default import sees its statics / `.prototype`
    /// instead of an opaque `_cjs`. Verify the wrap flat-emits the class
    /// (no IIFE, direct default export) and still parses.
    #[test]
    fn issue_2310_class_referencing_iife_let_flat_emits() {
        let src = "'use strict';\n\
                   const POOL_SIZE = 8;\n\
                   let pointer = 0;\n\
                   class Sender {\n\
                     static next() { return pointer++; }\n\
                   }\n\
                   module.exports = Sender;\n";
        let wrapped = wrap_commonjs(src, &PathBuf::from("/tmp/sender.js"));
        assert!(
            wrapped.contains("export default Sender;"),
            "expected flat default export of Sender, got:\n{}",
            wrapped
        );
        assert!(
            !wrapped.contains("const _cjs = (function()"),
            "expected the IIFE to be dropped for the flat default-export class, got:\n{}",
            wrapped
        );
        // `class Sender` and `let pointer` both land at module scope, so the
        // mutable closure is preserved (behavioral parity verified separately).
        assert!(wrapped.contains("class Sender"));
        assert!(wrapped.contains("let pointer = 0;"));
        let parsed = perry_parser::parse_typescript(&wrapped, "sender.js");
        assert!(
            parsed.is_ok(),
            "flat-emitted sender wrap failed to parse: {:?}\nwrapped:\n{}",
            parsed.err(),
            wrapped
        );
    }

    /// Issue #2310 — control case: a class that doesn't reference any
    /// IIFE-local binding STILL gets hoisted (the v0.5.x #652 behavior).
    /// Regression guard so the #2310 helper doesn't over-fire.
    #[test]
    fn issue_2310_self_contained_class_still_hoists() {
        let src = "class Pure {\n\
                     static greet() { return 'hi'; }\n\
                   }\n\
                   module.exports = Pure;\n";
        let wrapped = wrap_commonjs(src, &PathBuf::from("/tmp/pure.js"));
        let iife_open = wrapped
            .find("const _cjs = (function()")
            .expect("wrap must produce the IIFE wrapper");
        let class_pos = wrapped
            .find("class Pure")
            .expect("wrap must keep `class Pure` somewhere");
        assert!(
            class_pos < iife_open,
            "self-contained class must still hoist above the IIFE; got:\n{}",
            wrapped
        );
    }

    /// `module_reexport_specs` recognizes the trivial re-export wrapper
    /// shape (`module.exports = require('./X')`, incl. conditional / bare
    /// `exports =`) and ONLY that shape — a module that requires a sibling
    /// for its own use must not be treated as a re-export of it.
    #[test]
    fn module_reexport_specs_only_for_true_reexports() {
        // Trivial re-export wrappers.
        assert_eq!(
            module_reexport_specs("module.exports = require('./lib/index');"),
            vec!["./lib/index".to_string()]
        );
        assert_eq!(
            module_reexport_specs(
                "if (process.env.NODE_ENV === 'production') { module.exports = require('./prod'); } else { module.exports = require('./dev'); }"
            ),
            vec!["./prod".to_string(), "./dev".to_string()]
        );
        assert_eq!(
            module_reexport_specs("exports = require('./x');"),
            vec!["./x".to_string()]
        );

        // NOT re-export wrappers — semver's comparator.js shape: require a
        // sibling for internal use, then export a class. Forwarding ./re's
        // names here is exactly the `reading 'COMPARATOR'` bug.
        assert!(module_reexport_specs(
            "const { safeRe: re, t } = require('../internal/re');\nclass Comparator { parse() { return re[t.COMPARATOR]; } }\nmodule.exports = Comparator;"
        )
        .is_empty());
        // Member access / object-spread on the require result are not pure
        // re-exports either.
        assert!(module_reexport_specs("module.exports = require('./x').foo;").is_empty());
        assert!(module_reexport_specs("module.exports = { ...require('./x') };").is_empty());
    }

    /// Regression for the semver `Cannot read properties of undefined
    /// (reading 'COMPARATOR')` root: a module that requires a sibling for
    /// internal use (NOT a re-export wrapper) must not get the sibling's
    /// export names forwarded as spurious `export const X = _cjs.X;`
    /// declarations. Those both shadow the module's own destructured
    /// bindings and resolve to `undefined`.
    #[test]
    fn internal_require_does_not_forward_sibling_exports() {
        let dir =
            std::env::temp_dir().join(format!("perry_cjs_reexport_test_{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        // The required sibling exposes a `t` table (semver internal/re.js shape).
        fs::write(
            dir.join("re.js"),
            "module.exports = { t: { COMPARATOR: 0 } };",
        )
        .unwrap();
        let consumer = "const { t } = require('./re');\nclass Comparator { constructor() { this.r = t.COMPARATOR; } }\nmodule.exports = Comparator;\n";
        let wrapped = wrap_commonjs(consumer, &dir.join("comparator.js"));
        assert!(
            !wrapped.contains("export const t = _cjs.t;"),
            "internal require('./re') must NOT forward re.js's `t` export, got:\n{}",
            wrapped
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// `collect_top_level_let_const_var_names` (via the #2310 hoist guard)
    /// must recognize destructured top-level bindings so a class closing
    /// over them is not hoisted out of the IIFE (which would sever the
    /// closure). Indirectly asserted through the wrap: a class referencing
    /// a destructured IIFE-local stays inside the IIFE.
    #[test]
    fn destructured_iife_local_keeps_class_in_iife() {
        // `module.exports = { C }` (object aggregator, not a single-class
        // default) so the flat-emit path is NOT taken — exercising the
        // hoist-guard path specifically.
        let src = "const { tbl } = require('./re');\n\
                   class C { method() { return tbl.X; } }\n\
                   module.exports = { C };\n";
        let wrapped = wrap_commonjs(src, &PathBuf::from("/tmp/cmp.js"));
        // The class must NOT be hoisted above the IIFE — it closes over the
        // destructured `tbl`.
        if let Some(iife_open) = wrapped.find("const _cjs = (function()") {
            if let Some(class_pos) = wrapped.find("class C ") {
                assert!(
                    class_pos > iife_open,
                    "class closing over destructured IIFE-local `tbl` must stay inside the IIFE; got:\n{}",
                    wrapped
                );
            }
        }
    }

    /// Chain-aware hoist: a class that does NOT itself reference an IIFE-local
    /// but `extends` a sibling class that IS kept in the IIFE must ALSO stay in
    /// the IIFE — hoisting only the child out would leave its `extends <Parent>`
    /// unable to see the IIFE-local parent (ajv `codegen/index.js`'s
    /// `class AssignOp extends Assign` where `Assign` refs `code_1`). Asserts
    /// `extract_top_level_class_decls` hoists NEITHER class.
    #[test]
    fn hoist_keeps_inheritance_chain_with_iife_local_parent_together() {
        let src = "const code_1 = require('./code');\n\
                   class Node { kind() { return code_1.tag; } }\n\
                   class Assign extends Node { render() { return code_1.name; } }\n\
                   class AssignOp extends Assign {}\n\
                   module.exports = { AssignOp };\n";
        let (_blocks, hoisted_names, _rest) = extract_top_level_class_decls(src);
        // `Node`/`Assign` ref `code_1` (kept); `AssignOp` extends the kept
        // `Assign` so it must be kept too — none should be hoisted.
        assert!(
            hoisted_names.is_empty(),
            "no class should be hoisted (chain anchored to IIFE-local `code_1`); hoisted: {:?}",
            hoisted_names
        );
        // Control: a self-contained class with no IIFE-local refs and no kept
        // parent IS still hoistable.
        let src2 = "const code_1 = require('./code');\n\
                    class Plain {}\n\
                    module.exports = { Plain };\n";
        let (_b2, hoisted2, _r2) = extract_top_level_class_decls(src2);
        assert!(
            hoisted2.contains(&"Plain".to_string()),
            "a class with no IIFE-local ref and no kept parent should still hoist; hoisted: {:?}",
            hoisted2
        );
    }
}
