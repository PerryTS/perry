//! `is_commonjs` detection tests: plain CJS/ESM classification, masking of
//! strings/templates/regexes, and the #851 / #5498 hybrid-module cases.
//! Split out of `cjs_wrap/tests.rs` to stay under the 2,000-line cap (#10750).

use super::{is_commonjs, wrap_commonjs, PathBuf};

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
fn issue_5498_minified_mid_line_import_is_esm() {
    // esbuild ESM bundles (the OpenAI Codex CLI) are minified: every
    // top-level statement is joined onto one giant line, so the real
    // `import{createRequire …}from"module"` lands mid-line, after a `;`
    // that terminates the prior statement — never at a line start. A
    // line-based scan misses it and the file was misclassified as CJS.
    let src = "var a=Object.create;var Ke=(e=>typeof require<\"u\")(function(){});\
                   import{createRequire as NDe}from\"module\";var b=1;";
    assert!(
        !is_commonjs(src),
        "minified bundle with a mid-line top-level import must be ESM"
    );
}

#[test]
fn issue_5498_esbuild_cjs_shims_do_not_force_cjs() {
    // The Codex bundle inlines CJS deps, so esbuild emits its
    // `__commonJS`/`createRequire`/`require(` helper machinery alongside a
    // genuine top-level `import`. The top-level import must win — the
    // helper tokens are just identifiers in nested bodies.
    let src = "#!/usr/bin/env node\n\
                   import{createRequire as NDe}from\"module\";\
                   var __require=NDe(import.meta.url);\
                   var __commonJS=(cb,mod)=>function(){return mod||(0,cb[__getOwnPropNames(cb)[0]])((mod={exports:{}}).exports,mod),mod.exports};\
                   var x=__commonJS({\"a.js\"(exports,module){module.exports=require(\"fs\")}});";
    assert!(
        !is_commonjs(src),
        "esbuild ESM bundle with CJS helper shims must be classified as ESM"
    );
}

#[test]
fn issue_5498_shebang_cjs_wraps_and_parses() {
    // A genuine CommonJS file carrying a leading shebang (CLI entry point)
    // must still wrap cleanly: the `#!` is neutralized to a `//` line
    // comment in place so it does not land mid-template as an illegal
    // token. Without the fix SWC errors `ExpectedIdent` on the buried `#`.
    let src = "#!/usr/bin/env node\nmodule.exports = function greet(n) { return n; };\n";
    assert!(is_commonjs(src));
    let wrapped = wrap_commonjs(src, &PathBuf::from("/tmp/cli/index.js"));
    assert!(
        !wrapped.contains("#!"),
        "shebang must be neutralized, got:\n{}",
        wrapped
    );
    assert!(
        perry_parser::parse_typescript(&wrapped, "cli/index.js").is_ok(),
        "wrapped shebang module must parse, got:\n{}",
        wrapped
    );
}
