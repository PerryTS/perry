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
//!
//! #7152 adds the same coupling for the preamble's own ALLOCATIONS. The
//! recogniser there keys on the `__cjs_module` binding name, the
//! `{ exports: {} }` literal, and the `var module = __cjs_module` alias that
//! denies it — three more things this file's template controls and nothing
//! else checks. Its failure mode is quieter still: the report silently goes
//! back to charging Perry's own scaffolding to the user, which is what #7139
//! and #7149 both misread as evidence about dependency code.

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

// ── #7152: the preamble's own allocations ──────────────────────────────────

/// The record `Let` plus the four allocating preamble statements behind it:
/// `defineProperty(require, 'name', …)`, `require.cache = {}`,
/// `require.extensions = { … }`, and the transpiler's
/// `defineProperty(exports, "__esModule", …)`.
const EXPECTED_PREAMBLE_ALLOC_STMTS: usize = 5;

/// The #7152 half of the canary. Red means `wrap.rs` and
/// `perry-codegen/src/collectors/cjs_scaffolding.rs` disagree about what the
/// preamble looks like — fix one or the other, do not delete this test.
#[test]
fn the_cjs_preamble_is_still_recognised_as_scaffolding_allocation() {
    let path = Path::new("/tmp/perry-canary/node_modules/dep/index.js");
    let wrapped = wrap_commonjs_for_target(CJS_FIXTURE, path, None);

    // Anti-vacuity on the template, one assertion per recogniser conjunct, so
    // a template edit names the conjunct it broke rather than failing as an
    // opaque count mismatch.
    for (needle, conjunct) in [
        (
            "const __cjs_module = { exports: {} };",
            "R1/R2 (the record and its `{ exports: {} }` literal)",
        ),
        (
            "var module = __cjs_module;",
            "R4 (the alias that denies the record)",
        ),
        ("require.cache = {}", "the `require.cache` allocation"),
        (
            "require.extensions = {",
            "the `require.extensions` allocation",
        ),
    ] {
        assert!(
            wrapped.contains(needle),
            "the CJS preamble no longer emits `{needle}`.\n\
             That is conjunct {conjunct} of the #7152 recogniser in \
             perry-codegen/src/collectors/cjs_scaffolding.rs. Either update the \
             recogniser to match the new template, or — if the statement is \
             gone for good — delete its arm there and lower \
             EXPECTED_PREAMBLE_ALLOC_STMTS here."
        );
    }

    let hir = wrap_and_lower(CJS_FIXTURE);
    let census = perry_codegen::cjs_preamble_census(&hir);
    assert_eq!(
        census.module_records, 1,
        "the `Ptr<Shape>` report no longer recognises `const __cjs_module = \
         {{ exports: {{}} }}` as Perry's own scaffolding. Every CommonJS module \
         is now back to reporting it as a denied user candidate (#7152)."
    );
    assert_eq!(
        census.preamble_alloc_stmts, EXPECTED_PREAMBLE_ALLOC_STMTS,
        "the number of recognised preamble allocation statements changed. \
         Compare `cjs_preamble` in wrap.rs against \
         `CjsPreamble::stmt_allocates_only_scaffolding`."
    );
}

/// Positive control: the recogniser is per module, not a constant. A module
/// that was never `cjs_wrap`ped has no preamble at all — without this, the
/// assertions above would pass just as well against a recogniser that counted
/// every module as scaffolding.
#[test]
fn a_module_that_was_never_cjs_wrapped_has_no_preamble() {
    let ast = perry_parser::parse_typescript(
        "class Point { x = 0; }\nexport const p = new Point();\n",
        "plain.ts",
    )
    .expect("fixture must parse");
    let hir = perry_hir::lower_module(&ast, "plain", "/tmp/perry-canary/plain.ts")
        .expect("fixture must lower");
    let census = perry_codegen::cjs_preamble_census(&hir);
    assert_eq!(census.module_records, 0);
    assert_eq!(census.preamble_alloc_stmts, 0);
}

#[test]
fn path_module_wrap_publishes_partial_then_final_exports_and_tracks_undefined() {
    let path = Path::new("/tmp/perry-canary/.next/server/chunks/lazy.js");
    let marker = "exports.ready = true;";
    let wrapped = wrap_commonjs_for_target(marker, path, None);

    let partial = wrapped
        .find("__perry_register_path_module_partial(")
        .expect("CJS wrapper must publish its initial exports object");
    let body = wrapped
        .find(marker)
        .expect("fixture body must remain in the wrapper");
    let final_publish = wrapped
        .rfind("__perry_register_path_module(")
        .expect("CJS wrapper must publish its final module.exports value");
    assert!(partial < body && body < final_publish, "{wrapped}");
    // #8040: both the value lookup and the presence probe must consult the
    // SAME resolved specifier. A computed relative request is joined against
    // the module's directory before either call (`__perry_path_spec`), so a
    // mismatch here would resolve the value from one path and the
    // exists-but-undefined bit from another.
    assert!(
        wrapped.contains(
            "__perry_path_mod !== undefined || __perry_has_path_module(__perry_path_spec)"
        ),
        "an exported undefined value must not be mistaken for a registry miss\n{wrapped}"
    );

    // Run the real parser/lowerer as an anti-vacuity check for the registry
    // intrinsics, including the boolean presence probe in the require shim.
    let ast = perry_parser::parse_typescript(&wrapped, "lazy.js").unwrap();
    perry_hir::lower_module(&ast, "lazy", &path.to_string_lossy()).unwrap();
}

/// #8040: Next's production webpack runtime loads lazy chunks with a *computed*
/// relative specifier — `.next/server/webpack-runtime.js` calls
/// `require("./chunks/" + g.u(a))`. The path->module registry is keyed by each
/// module's ABSOLUTE source path, so handing it the raw `./chunks/2.js` could
/// never hit: the compiled App Route died at startup with
/// `Cannot find module './chunks/2.js'` even though that chunk had been
/// compiled into the image alongside the other 103 modules.
///
/// Statically-known relative specifiers are resolved at compile time and never
/// reach that branch, which is why only the real production route exposed it.
#[test]
fn computed_relative_requires_are_joined_against_the_module_dir() {
    let path = Path::new("/tmp/perry-canary/.next/server/webpack-runtime.js");
    let wrapped = wrap_commonjs_for_target(CJS_FIXTURE, path, None);

    // Anti-vacuity: if the wrap stops consulting the registry at all, the
    // assertions below would be about a branch that no longer exists.
    assert!(
        wrapped.contains("__perry_require_path_module("),
        "the wrap no longer consults the path->module registry:\n{wrapped}"
    );
    // The join needs the module's own directory as a literal.
    assert!(
        wrapped.contains("/tmp/perry-canary/.next/server"),
        "the wrap lost the module-dir literal the join needs:\n{wrapped}"
    );
    // The registry lookup must use the JOINED path...
    assert!(
        wrapped.contains("__perry_require_path_module(__perry_path_spec)"),
        "computed relative requires are not joined before the registry lookup (#8040):\n{wrapped}"
    );
    // ...and must not still be handed the raw specifier, which is the shape
    // that made every lazy chunk miss.
    assert!(
        !wrapped.contains("__perry_require_path_module(specifier)"),
        "the raw-specifier registry lookup is still present; a computed \
         './chunks/N.js' will miss it (#8040)"
    );
}

// ── #9412: the wrap's synthetic `createRequire` import ─────────────────────

/// The entry codegen decides `process.nextTick`-vs-microtask ordering from
/// "is this entry an ES module?", and the wrap answers yes for every CommonJS
/// file because it injects an import and an export. #9412 re-gates that on
/// `collectors::is_cjs_wrapped_module`, which recognises a wrapped module by
/// the local name the synthetic `node:module` import binds.
///
/// Nothing else links the two. Rename the local in `wrap.rs` and every
/// CommonJS entry silently goes back to ES-module tick ordering — ticks after
/// the promise queue, where Node runs them first — with no test failing and no
/// error, which is exactly how #9412 survived in the first place.
#[test]
fn the_wrap_still_binds_the_local_the_cjs_entry_recogniser_keys_on() {
    let local = perry_codegen::cjs_wrap_create_require_local();
    let path = Path::new("/tmp/perry-canary/node_modules/dep/index.js");
    let wrapped = wrap_commonjs_for_target(CJS_FIXTURE, path, None);

    // Anti-vacuity: the template must still emit the binding at all.
    assert!(
        wrapped.contains(local),
        "the CJS wrap no longer binds `{local}`.\n\
         `is_cjs_wrapped_module` in perry-codegen/src/collectors/cjs_scaffolding.rs \
         keys on that local to tell a CommonJS entry from a hand-written ES module; \
         without it every CommonJS entry runs `process.nextTick` after the promise \
         queue again (#9412). Rename it in both places, or replace the recogniser."
    );

    let hir = wrap_and_lower(CJS_FIXTURE);
    assert!(
        perry_codegen::module_is_cjs_wrapped(&hir),
        "a wrapped CommonJS module is no longer recognised as one. The wrap's \
         synthetic `node:module` import survived the source, but not lowering — \
         compare `is_cjs_wrapped_module`'s specifier match against what \
         `perry_hir::lower_module` produces for the wrap's import prefix."
    );
}

/// Negative control: a genuine ES module must NOT be mistaken for a wrapped
/// CommonJS one, or #9412's fix would give real ESM entries CommonJS tick
/// ordering — the same bug pointed the other way.
#[test]
fn a_hand_written_es_module_is_not_recognised_as_cjs_wrapped() {
    const ESM_FIXTURE: &str = r#"import { createRequire } from 'node:module';
const require2 = createRequire(import.meta.url);
export const value = 1;
"#;
    let ast = perry_parser::parse_typescript(ESM_FIXTURE, "esm.ts")
        .expect("fixture must parse");
    let hir = perry_hir::lower_module(&ast, "esm", "/tmp/perry-canary/esm.ts")
        .expect("fixture must lower");
    assert!(
        !perry_codegen::module_is_cjs_wrapped(&hir),
        "a user's own `import {{ createRequire }} from 'node:module'` was taken for \
         Perry's wrap. The recogniser must key on the aliased local \
         (`{}`), not on the module specifier.",
        perry_codegen::cjs_wrap_create_require_local()
    );
}
