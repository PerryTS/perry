//! #7848 — a generic CLASS instantiated with a type **alias** argument must
//! still resolve to its monomorphized specialization.
//!
//! ## The invariant, and why it is not obvious
//!
//! Two independent lowerings read the SAME `new C<…>()` type-argument list:
//!
//! * `lower/expr_new.rs` builds `Expr::New::type_args` with
//!   `extract_ts_type_with_ctx(t, Some(ctx))`, which **expands type aliases**.
//!   Monomorphization keys the specialization on those (`Registry$fn_num`).
//! * `lower_types.rs`'s `infer_type_from_expr` builds the binding's INFERRED
//!   declared type. It used to call the context-free `extract_ts_type(t)`,
//!   which leaves an alias as `Type::Named("Stage")`.
//!
//! Codegen re-derives the specialization from the declared type
//! (`type_analysis/predicates.rs::receiver_class_name` ->
//! `generate_specialized_name`). When the two manglings disagree the lookup
//! misses and it **silently falls back to the generic TEMPLATE class**.
//!
//! Nothing goes red when that happens: the emitted class-id + keys-token guard
//! is a real runtime check, so a wrong class degrades to a guard that never
//! passes, not to a wrong answer. The only symptom is that the guarded
//! direct-dispatch arm AND the guard-free `Ptr<Shape>` arm (which requires
//! `fact.class_name == class_name`, `lower_call/property_get/dynamic_dispatch.rs`)
//! both become unreachable, and every method call on the binding takes
//! `js_native_call_method_by_id` forever. On `gc-handoff/apps/pipeline.ts` that
//! was 53.8% of the program.
//!
//! ## What these tests assert
//!
//! Exactly the property codegen depends on, stated as an equation rather than
//! as a spelling: for a `const x = new C<…>()`, the name
//! `generate_specialized_name` derives from the binding's declared type MUST
//! equal the class the `New` was rewritten to. That is checked directly, so a
//! future change to `mangle_type` or to the specialization naming scheme
//! cannot make the tests pass while the two sides drift apart again.
//!
//! Every alias ARM is covered, not a sample of spellings — the four that were
//! broken (primitive / function / object-literal / union aliases) and the two
//! that always round-tripped (a class, an interface), because `mangle_type`
//! maps `Named(n) -> n` and those are genuinely `Named`. A regression that
//! fixed only the arm `pipeline` happens to use would leave the rest silent.

#![cfg(test)]

use crate::monomorph::generate_specialized_name;
use crate::types::Type;
use crate::{lower_module, Expr, Module, Stmt};
use perry_diagnostics::SourceCache;
use perry_parser::parse_typescript_with_cache;

/// Lowering is deeply recursive; the default 2 MB test thread SIGABRTs.
///
/// ★ `monomorphize_module` is **not optional here**, and leaving it out is how the
/// first version of these tests measured nothing: `lower_module` alone leaves every
/// `Expr::New::class_name` at the generic BASE (`Reg`), because rewriting it to the
/// specialization is `monomorph::update_call_sites`' job. Without this call all seven
/// cases "fail" identically — including the class / interface / builtin controls that
/// were never broken — which is the tell that the harness, not the compiler, is wrong.
fn lower_src(src: &str) -> Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed = parse_typescript_with_cache(&src, "test.ts", &mut cache)
                .expect("parse should succeed");
            let mut module =
                lower_module(&parsed.module, "test", "test.ts").expect("lowering should succeed");
            crate::monomorphize_module(&mut module);
            module
        })
        .expect("spawn")
        .join()
        .expect("lowering thread")
}

/// The `(declared type, constructed class)` pair for the `const r = new …`
/// binding, found wherever in the module it was lowered to.
fn reg_binding(module: &Module) -> (Type, String) {
    fn walk(stmts: &[Stmt], out: &mut Option<(Type, String)>) {
        for stmt in stmts {
            if let Stmt::Let {
                name,
                ty,
                init: Some(init),
                ..
            } = stmt
            {
                if name == "r" {
                    if let Expr::New { class_name, .. } = init {
                        *out = Some((ty.clone(), class_name.clone()));
                    }
                }
            }
            match stmt {
                Stmt::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    walk(then_branch, out);
                    if let Some(e) = else_branch {
                        walk(e, out);
                    }
                }
                Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::For { body, .. } => {
                    walk(body, out)
                }
                _ => {}
            }
        }
    }
    let mut found = None;
    for func in &module.functions {
        walk(&func.body, &mut found);
    }
    found.expect("no `const r = new …` binding found in lowered module")
}

fn source_with(prelude: &str, type_arg: &str, key_value: &str) -> String {
    format!(
        r#"{prelude}
class Reg<K, V> {{
  private ks: K[] = [];
  private vs: V[] = [];
  set(k: K, v: V): void {{ this.ks.push(k); this.vs.push(v); }}
  size(): number {{ return this.ks.length; }}
}}
function main(): void {{
  const r = new Reg<{type_arg}, number>();
  r.set({key_value}, 1);
  console.log(r.size());
}}
main();
"#
    )
}

/// ★ The whole point. `generate_specialized_name` applied to the binding's
/// declared type must name the class the `New` actually constructs — otherwise
/// `receiver_class_name` degrades to the template and both fast tiers die.
#[track_caller]
fn assert_declared_type_names_the_constructed_class(
    prelude: &str,
    type_arg: &str,
    key_value: &str,
) {
    let module = lower_src(&source_with(prelude, type_arg, key_value));
    let (declared, constructed) = reg_binding(&module);

    let Type::Generic { base, type_args } = &declared else {
        panic!("`const r = new Reg<{type_arg}, number>()` should infer a Generic declared type, got {declared:?}");
    };
    let derived = generate_specialized_name(base, type_args);
    assert_eq!(
        derived, constructed,
        "declared type {declared:?} re-derives `{derived}` but the `New` \
         constructs `{constructed}` — codegen's receiver_class_name will miss \
         and silently fall back to the template class `{base}` (#7848)"
    );
}

#[test]
fn alias_of_a_function_type_resolves_to_the_specialization() {
    // `gc-handoff/apps/pipeline.ts`'s exact shape: `type Stage = (r) => Record`.
    assert_declared_type_names_the_constructed_class(
        "type Rec = { id: number };\ntype Stage = (r: Rec) => Rec;",
        "Stage",
        "((x: Rec) => x)",
    );
}

#[test]
fn alias_of_a_primitive_resolves_to_the_specialization() {
    assert_declared_type_names_the_constructed_class("type S = string;", "S", "\"a\"");
}

#[test]
fn alias_of_an_object_literal_type_resolves_to_the_specialization() {
    assert_declared_type_names_the_constructed_class("type O = { a: number };", "O", "({ a: 1 })");
}

#[test]
fn alias_of_a_union_resolves_to_the_specialization() {
    assert_declared_type_names_the_constructed_class("type U = string | number;", "U", "\"a\"");
}

// The two arms that were ALWAYS correct — `mangle_type` maps `Named(n) -> n`,
// and a class / interface reference genuinely lowers to `Named`. They are here
// so a future "just expand everything" change that broke them would be caught.

#[test]
fn a_class_type_argument_still_resolves_to_the_specialization() {
    assert_declared_type_names_the_constructed_class("class C { a = 1; }", "C", "new C()");
}

#[test]
fn an_interface_type_argument_still_resolves_to_the_specialization() {
    assert_declared_type_names_the_constructed_class(
        "interface I { a: number }",
        "I",
        "({ a: 1 })",
    );
}

/// A builtin (non-alias) argument is the control: it always worked, and it is
/// what the broken arms are being brought level with.
#[test]
fn a_builtin_type_argument_resolves_to_the_specialization() {
    assert_declared_type_names_the_constructed_class("", "string", "\"a\"");
}
