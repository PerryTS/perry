//! Dynamic-parent registration for a mixin applied to a mixin (#9079). Split
//! from `tests.rs` for the 2000-line file cap.

use super::*;

/// Index of the `RegisterClassParentDynamic` for `class_name` in `init`.
fn register_at(init: &[Stmt], class_name: &str) -> Option<usize> {
    init.iter().position(|stmt| {
        matches!(
            stmt,
            Stmt::Expr(Expr::RegisterClassParentDynamic { class_name: n, .. }) if n == class_name
        )
    })
}

/// Index of the value binding (`const M = <class>`) for `name` in `init`.
fn binding_at(init: &[Stmt], name: &str) -> Option<usize> {
    init.iter()
        .position(|stmt| matches!(stmt, Stmt::Let { name: n, .. } if n == name))
}

/// #9079: `const Mixed2 = mixin(Mixed)` — a mixin applied to a previous
/// mixin's RESULT — synthesizes a class whose parent is a lexical value
/// binding, so it lowers with `extends_expr` (a dynamic parent). The mixin
/// fast path bound that class without the declaration-time
/// `RegisterClassParentDynamic` its sibling `const X = class …` path emits, so
/// the class had no registered parent at all: `js_fetch_or_value_super` fell
/// back to the most-derived receiver, re-selected the same class, and recursed
/// until the stack overflowed (SIGSEGV, not a wrong value).
///
/// The registration must sit between the PARENT's binding (it reads that
/// local) and the synthesized class's own binding, exactly where the sibling
/// class-expression path puts it.
#[test]
fn mixin_of_a_mixin_registers_its_dynamic_parent_before_its_own_binding() {
    let source = r#"
        class Root { r = 1; }
        function mixin(Base: any) { return class extends Base { m() { return 1; } }; }
        const Mixed = mixin(Root);
        const Mixed2 = mixin(Mixed);
        class Deep extends Mixed2 { d = 4; constructor() { super(); } }
        console.log(new Deep().d);
    "#;
    let module = perry_parser::parse_typescript(source, "mixin-chain.ts").expect("source parses");
    let hir = super::super::lower_module(&module, "mixin-chain", "mixin-chain.ts")
        .expect("source lowers");

    let mixed_binding =
        binding_at(&hir.init, "Mixed").expect("`Mixed` gets a class-expression value binding");
    let mixed2_binding =
        binding_at(&hir.init, "Mixed2").expect("`Mixed2` gets a class-expression value binding");
    let mixed2_register = register_at(&hir.init, "Mixed2").unwrap_or_else(|| {
        panic!(
            "`Mixed2` extends the lexical value `Mixed` and must register that parent \
             at declaration time; module init was:\n{:#?}",
            hir.init
        )
    });

    assert!(
        mixed_binding < mixed2_register && mixed2_register < mixed2_binding,
        "the registration reads the parent local and must precede its own binding: \
         Mixed@{mixed_binding} register@{mixed2_register} Mixed2@{mixed2_binding}"
    );

    // Level 1's parent is the real class `Root`, which resolves statically, so
    // no dynamic parent is captured and none is registered. Asserting the
    // negative keeps the fix scoped to the case that needs it.
    assert!(
        register_at(&hir.init, "Mixed").is_none(),
        "`Mixed` extends the static class `Root`; it must not gain a dynamic parent:\n{:#?}",
        hir.init
    );
}
