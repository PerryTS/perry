//! Regression tests for #10662: `agent-base`'s TypeScript `namespace
//! createAgent { export class Agent extends EventEmitter { … } } export =
//! createAgent;` — a `function createAgent()` merged with a namespace of the
//! same name, exported via TS's `export =` form. Perry's HIR lowers the
//! namespace's exported `Agent` class as a static-field-set against a
//! synthetic class entity distinct from the runtime function value
//! `export =` actually exports, so a downstream `https-proxy-agent extends
//! agent_base_1.Agent` (an `axios` transitive dependency) sees `.Agent` as
//! `undefined` and throws "Class extends value is not a constructor".
//!
//! `has_top_level_namespace_or_module_block` /
//! `has_top_level_export_equals` detect this shape so
//! `is_hybrid_cjs_emit_input` (`resolve.rs`) can fall back to the package's
//! compiled JS emit — the same emit Node itself runs, since
//! `--experimental-strip-types` can't execute raw `namespace`/`export =`
//! syntax either.

use super::detect::{
    has_top_level_export_equals, has_top_level_namespace_or_module_block,
    strip_comments_and_strings,
};

#[test]
fn namespace_block_detects_the_agent_base_shape() {
    let src = strip_comments_and_strings(
        "function createAgent(opts) {\n  return new createAgent.Agent(opts);\n}\n\nnamespace createAgent {\n  export class Agent extends EventEmitter {}\n}\n\nexport = createAgent;\n",
    );
    assert!(has_top_level_namespace_or_module_block(&src));
    assert!(has_top_level_export_equals(&src));
}

#[test]
fn namespace_block_accepts_legacy_module_keyword_and_dotted_names() {
    let src = strip_comments_and_strings("module Foo.Bar {\n  export const x = 1;\n}\n");
    assert!(has_top_level_namespace_or_module_block(&src));
}

#[test]
fn ambient_declare_namespace_is_not_flagged() {
    // `declare namespace X { … }` is type-only — it never emits runtime
    // code, so it cannot be the cause of a namespace/function merge going
    // missing at runtime, and must not trigger the JS-emit fallback.
    let src = strip_comments_and_strings(
        "declare namespace createAgent {\n  export class Agent {}\n}\nexport = createAgent;\n",
    );
    assert!(!has_top_level_namespace_or_module_block(&src));
}

#[test]
fn ordinary_cjs_module_exports_object_literal_is_not_flagged() {
    // `module.exports = { … }` is the single most common CommonJS shape —
    // `module` is followed by `.`, never by whitespace then an identifier,
    // so it must never be mistaken for a `namespace`/`module X {` block.
    let src = strip_comments_and_strings(
        "function build() { return 1; }\nmodule.exports = { build: build, value: 42 };\n",
    );
    assert!(!has_top_level_namespace_or_module_block(&src));
}

#[test]
fn export_equals_matches_the_export_equals_form_only() {
    assert!(has_top_level_export_equals(&strip_comments_and_strings(
        "export = createAgent;\n"
    )));
    assert!(has_top_level_export_equals(&strip_comments_and_strings(
        "export=createAgent;\n"
    )));

    // Ordinary ESM export forms must not match — none of these are the
    // CJS-interop `export =` shape.
    assert!(!has_top_level_export_equals(&strip_comments_and_strings(
        "export const x = 1;\n"
    )));
    assert!(!has_top_level_export_equals(&strip_comments_and_strings(
        "export class Foo {}\n"
    )));
    assert!(!has_top_level_export_equals(&strip_comments_and_strings(
        "export default Foo;\n"
    )));
    assert!(!has_top_level_export_equals(&strip_comments_and_strings(
        "export { Foo };\n"
    )));
}

#[test]
fn plain_esm_or_cjs_source_without_the_merge_shape_is_unaffected() {
    // A normal ESM file with a class extending an imported native builtin —
    // the overwhelmingly common case — must not be flagged: no namespace
    // block, no `export =`.
    let esm = strip_comments_and_strings(
        "import { EventEmitter } from 'events';\nexport class Agent extends EventEmitter {}\n",
    );
    assert!(!has_top_level_namespace_or_module_block(&esm));
    assert!(!has_top_level_export_equals(&esm));

    // A normal CJS file.
    let cjs =
        strip_comments_and_strings("'use strict';\nclass Agent {}\nmodule.exports = { Agent };\n");
    assert!(!has_top_level_namespace_or_module_block(&cjs));
    assert!(!has_top_level_export_equals(&cjs));
}
