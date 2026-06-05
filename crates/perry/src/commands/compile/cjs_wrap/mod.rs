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
};
pub(self) use extract_requires::{extract_require_aliases_with_ranges, extract_require_specifiers};
pub(self) use hoist_classes::{
    extract_top_level_class_decls, rewrite_module_exports_class_expression,
};

// Public API consumed by `compile.rs` / `collect_modules.rs`.
pub(super) use detect::is_commonjs;
pub(super) use wrap::wrap_commonjs;

#[cfg(test)]
mod tests {
    use super::detect::is_commonjs;
    use super::extract_exports::{
        extract_exports_from_source, extract_named_exports_from_require,
        extract_object_literal_exports_from_require, extract_single_module_exports_assignment,
    };
    use super::extract_requires::{
        extract_require_aliases_with_ranges, extract_require_specifiers,
    };
    use super::wrap::wrap_commonjs;
    use std::fs;
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

    /// Issue #2310 — when a top-level class body references a let/const
    /// declared at the IIFE's top level (the ws/lib/sender.js shape:
    /// `let randomPoolPointer; class Sender { static frame(){ … r++ } }`),
    /// hoisting the class out of the IIFE would sever the closure over
    /// `randomPoolPointer` and the compile hard-errors with
    /// `Undefined variable in update expression`. Verify the class stays
    /// inside the IIFE body and is NOT in the hoisted-class block.
    #[test]
    fn issue_2310_class_referencing_iife_let_is_not_hoisted() {
        let src = "'use strict';\n\
                   const POOL_SIZE = 8;\n\
                   let pointer = 0;\n\
                   class Sender {\n\
                     static next() { return pointer++; }\n\
                   }\n\
                   module.exports = Sender;\n";
        let wrapped = wrap_commonjs(src, &PathBuf::from("/tmp/sender.js"));
        // The IIFE body must still contain the class — i.e. the wrap must
        // not lift it above the `const _cjs = (function() { ... })()` line.
        let iife_open = wrapped
            .find("const _cjs = (function()")
            .expect("wrap must produce the IIFE wrapper");
        let class_pos = wrapped
            .find("class Sender")
            .expect("wrap must keep `class Sender` somewhere");
        assert!(
            class_pos > iife_open,
            "expected `class Sender` to stay inside the IIFE for #2310; got:\n{}",
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
}
