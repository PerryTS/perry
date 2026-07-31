//! #7139 template-change canary: the CommonJS preamble this module emits must
//! stay recognisable to `perry-codegen`'s `collectors::cjs_scaffolding`.
//!
//! `Ptr<Shape>` rule 5 disables all shape promotion in a module containing any
//! `defineProperty`-family site. Two sites are exempted as module scaffolding:
//! the transpiler's `Object.defineProperty(exports, "__esModule", …)` and
//! **this file's own** `Object.defineProperty(require, 'name', …)` preamble
//! (`wrap.rs`, the `cjs_preamble` literal). The second one is emitted into
//! every wrapped module, so before #7139 it armed the barrier in 100 % of the
//! CommonJS dependency graph.
//!
//! The recogniser matches on binding name, initializer shape and property key.
//! Nothing links it to the template: rename the `require` local, change the
//! key, or bind it through anything other than a function declaration, and the
//! exemption silently stops applying. Nothing breaks, no test fails, and the
//! entire #7139 win evaporates with no symptom — the "gate that cannot fail"
//! shape CLAUDE.md documents.
//!
//! This canary closes that. It runs the real template through the real
//! recogniser: wrap → parse → lower → `module_has_ptr_shape_barrier`.

use std::path::Path;

use super::wrap::wrap_commonjs_for_target;

/// A minimal transpiled-CJS module: the `__esModule` marker, one class, one
/// function. Deliberately free of every barrier family, so the ONLY thing that
/// can arm the flag is the wrap's own preamble.
const CJS_FIXTURE: &str = r#""use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.compute = void 0;
class Point {
    constructor(x, y) { this.x = x; this.y = y; }
    sum() { return this.x + this.y; }
}
function compute(n) {
    const p = new Point(n, n + 1);
    return p.sum();
}
exports.compute = compute;
"#;

fn wrap_and_lower(body: &str) -> perry_hir::Module {
    let path = Path::new("/tmp/perry-canary/node_modules/dep/index.js");
    let wrapped = wrap_commonjs_for_target(body, path, None);
    let ast = perry_parser::parse_typescript(&wrapped, "index.js")
        .expect("the wrap template must produce parseable ESM");
    perry_hir::lower_module(&ast, "dep", &path.to_string_lossy())
        .expect("the wrap template must produce lowerable HIR")
}

/// The whole point. If this goes red, the CommonJS preamble in `wrap.rs` no
/// longer matches what `perry-codegen/src/collectors/cjs_scaffolding.rs`
/// recognises — fix one or the other, do not delete this test.
#[test]
fn cjs_preamble_does_not_arm_the_ptr_shape_module_barrier() {
    let path = Path::new("/tmp/perry-canary/node_modules/dep/index.js");
    let wrapped = wrap_commonjs_for_target(CJS_FIXTURE, path, None);

    // Anti-vacuity, and the more precise failure of the two: assert the
    // preamble still HAS the site the recogniser is written for. Without this
    // the test would pass trivially the day the template stops emitting it,
    // leaving `cjs_scaffolding.rs`'s `require` / `"name"` arm as dead code
    // nobody notices.
    assert!(
        wrapped.contains("defineProperty(require,"),
        "the CJS preamble no longer emits `Object.defineProperty(require, 'name', …)`.\n\
         Either it was renamed — in which case `REQUIRE_BINDING` / `REQUIRE_KEY` in \
         perry-codegen/src/collectors/cjs_scaffolding.rs must follow, or the rule-5 \
         barrier re-arms for every CommonJS module — or it was removed, in which case \
         that arm of the recogniser is now dead code and should go."
    );
    assert!(
        wrapped.contains(r#"defineProperty(exports, "__esModule""#),
        "the wrap no longer passes the transpiler's `__esModule` marker through verbatim; \
         `EXPORTS_BINDING` / `EXPORTS_KEY` in cjs_scaffolding.rs may need to follow"
    );

    let hir = wrap_and_lower(CJS_FIXTURE);
    assert!(
        !perry_codegen::module_has_ptr_shape_barrier(&hir),
        "the CommonJS scaffolding re-armed the Ptr<Shape> rule-5 module barrier.\n\
         Every `Ptr<Shape>` promotion in every CommonJS module is now disabled \
         (#7139). Compare the `exports` / `require` bindings the wrap emits against \
         the predicate in perry-codegen/src/collectors/cjs_scaffolding.rs."
    );
}

/// Positive control: the same wrap → parse → lower → collect chain DOES report
/// a barrier when the body contains a real one. Without this, an empty or
/// failed lowering would make the canary above pass for the wrong reason.
#[test]
fn the_canary_chain_still_reports_a_genuine_barrier() {
    let with_barrier = format!("{CJS_FIXTURE}\nconst o = {{ k: 1 }};\ndelete o.k;\n");
    let hir = wrap_and_lower(&with_barrier);
    assert!(
        perry_codegen::module_has_ptr_shape_barrier(&hir),
        "a `delete` in the module body must still arm the barrier — if it does not, \
         the canary above is passing vacuously (parse/lower produced nothing, or the \
         exemption is far wider than #7139 intended)"
    );
}
