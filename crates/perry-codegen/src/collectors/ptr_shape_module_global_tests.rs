//! Rule 6 (#10769, #10803): which module-level `const` bindings may seed
//! `Ptr<Shape>`, and — the half that matters — which may not.
//!
//! Every negative test here names the clause it is guarding in its doc, so the
//! sabotage is reproducible: delete that clause and exactly that test reddens.
//! The positive test fails against the pre-#10769 collector, where the seed
//! filter (`collectors/escape_check.rs:27`) removed every module-global
//! binding before any containment walk could look at it.

use std::collections::HashMap;

use perry_hir::types::{FuncId, Type};
use perry_hir::{Class, ClassField, Export, Expr, Function, Module, Stmt};

use super::collect_module_global_shape_locals;
use super::module_global_seed_candidates;

fn field(name: &str) -> ClassField {
    ClassField {
        name: name.to_string(),
        key_expr: None,
        ty: Type::Number,
        init: None,
        is_private: false,
        is_readonly: false,
        decorators: Vec::new(),
    }
}

/// A pure record, exactly as a closed object literal `{a: 1, b: 2}` lowers.
fn anon_shape() -> Class {
    Class {
        id: 0,
        name: "__AnonShape_ab".to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: vec![field("a"), field("b")],
        constructor: None,
        methods: Vec::new(),
        getters: Vec::new(),
        setters: Vec::new(),
        static_fields: Vec::new(),
        static_methods: Vec::new(),
        computed_members: Vec::new(),
        decorators: Vec::new(),
        is_exported: false,
        aliases: Vec::new(),
        is_nested: false,
        alloc_width_hint: 0,
        specialized_from: None,
        static_accessor_names: Vec::new(),
        static_accessor_fn_ids: Vec::new(),
    }
}

fn new_anon() -> Expr {
    Expr::New {
        class_name: "__AnonShape_ab".to_string(),
        args: vec![Expr::Number(1.0), Expr::Number(2.0)],
        type_args: Vec::new(),
        byte_offset: 0,
        cap_args_appended: 0,
    }
}

/// `const O = {a: 1, b: 2};` at module top level. id 7 throughout.
fn const_o(mutable: bool) -> Stmt {
    Stmt::Let {
        id: 7,
        name: "O".to_string(),
        ty: Type::Any,
        mutable,
        init: Some(new_anon()),
    }
}

/// `O.a` — the contained use rule 2 permits.
fn read_o_a() -> Expr {
    Expr::PropertyGet {
        object: Box::new(Expr::LocalGet(7)),
        property: "a".to_string(),
        byte_offset: 0,
    }
}

fn function(id: FuncId, name: &str, body: Vec<Stmt>) -> Function {
    Function {
        id,
        name: name.to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Any,
        body,
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    }
}

/// A module with the anon-shape class, the given init and the given functions.
fn module_with(init: Vec<Stmt>, functions: Vec<Function>) -> Module {
    let mut hir = Module::new("m.ts");
    hir.classes.push(anon_shape());
    hir.init = init;
    hir.functions = functions;
    hir
}

/// Run the real rule-6 proof, with the real barrier facts.
fn proven(hir: &Module) -> HashMap<u32, crate::collectors::PtrShapeLocal> {
    let classes: HashMap<String, &Class> =
        hir.classes.iter().map(|c| (c.name.clone(), c)).collect();
    let dispatch = crate::collectors::collect_module_dispatch_facts(hir);
    collect_module_global_shape_locals(hir, &classes, &dispatch)
}

/// The shape the whole rule exists for: a module-level `const` record whose
/// only uses anywhere in the module are field reads, one of them from inside a
/// function — which is exactly what promotes it to `@perry_global_*` and is
/// what the old seed filter dropped.
#[test]
fn a_contained_module_const_read_from_a_function_is_proven() {
    let hir = module_with(
        vec![const_o(false)],
        vec![function(1, "run", vec![Stmt::Return(Some(read_o_a()))])],
    );
    let facts = proven(&hir);
    assert_eq!(
        facts.get(&7).map(|f| f.class_name.as_str()),
        Some("__AnonShape_ab"),
        "a contained module-level const must be proven"
    );
}

/// Rule 6, first increment: a module-level seed stands down from the numeric
/// claim, because the exhaustive-reachable-store obligation is module-wide and
/// each region sees only its own stores.
#[test]
fn a_module_const_seed_claims_no_numeric_fields() {
    let hir = module_with(
        vec![const_o(false)],
        vec![function(1, "run", vec![Stmt::Return(Some(read_o_a()))])],
    );
    assert!(
        proven(&hir)[&7].numeric_fields.is_empty(),
        "rule 6 must not claim a numeric field from one region's stores"
    );
}

/// Rule 6c, and the soundness test of the whole change: the escape is in a
/// DIFFERENT region from the declaration. A per-region walk over `hir.init`
/// alone sees only a contained `const`; only the module-wide walk sees
/// `keep(O)` handing the object to an unbounded caller.
///
/// Sabotage: drop `extra_regions` from the walk in
/// `collect_shape_proven_ptr_locals_impl` and this test goes green while the
/// object is aliased.
#[test]
fn an_escape_in_another_region_denies_the_module_const() {
    let hir = module_with(
        vec![const_o(false)],
        vec![function(
            1,
            "leak",
            vec![Stmt::Expr(Expr::Call {
                callee: Box::new(Expr::FuncRef(2)),
                args: vec![Expr::LocalGet(7)],
                type_args: Vec::new(),
                byte_offset: 0,
            })],
        )],
    );
    assert!(
        !proven(&hir).contains_key(&7),
        "a bare reference in any region of the module must deny the proof"
    );
}

/// Rule 6c, the exemption that does NOT transfer. Rule 2 exempts `return
/// <the local>` because a return is a terminator for a FUNCTION-LOCAL: the
/// caller cannot have touched the object at any access the pass licenses. For
/// a module-level binding the return is a terminator for `leak`, not for `O`
/// -- the caller may reshape the record while every other region keeps reading
/// it at a fixed offset.
///
/// Sabotage: drop the `lifetime_bounded` test from the `Stmt::Return` arm of
/// `UseWalk` and this test goes green while the record is handed out.
#[test]
fn returning_the_module_const_from_a_function_denies_it() {
    let hir = module_with(
        vec![const_o(false)],
        vec![
            function(1, "leak", vec![Stmt::Return(Some(Expr::LocalGet(7)))]),
            function(2, "run", vec![Stmt::Return(Some(read_o_a()))]),
        ],
    );
    assert!(
        !proven(&hir).contains_key(&7),
        "rule 2's return exemption is a function-local argument and must not \
         apply to a module-level binding"
    );
}

/// Rule 6: the seed population is bindings that OUTLIVE module init. One that
/// only module init names is not promoted to a global cell, and its own
/// region's pass already proves it with a numeric-field claim rule 6 stands
/// down from -- seeding it here would be a 5-instruction regression, not a win.
#[test]
fn a_binding_no_other_region_names_is_not_a_rule_6_seed() {
    let hir = module_with(vec![const_o(false)], Vec::new());
    assert!(
        !proven(&hir).contains_key(&7),
        "an init-only binding belongs to the per-region pass"
    );
}

/// Rule 6b: an exported binding gets external linkage plus a
/// `perry_fn_<prefix>__<name>` getter, and becomes a live-accessor entry in
/// this module's namespace object — two second names, both outside the module.
#[test]
fn an_exported_module_const_is_not_a_seed() {
    let mut hir = module_with(
        vec![const_o(false)],
        vec![function(1, "run", vec![Stmt::Return(Some(read_o_a()))])],
    );
    hir.exports.push(Export::Named {
        local: "O".to_string(),
        exported: "O".to_string(),
    });
    assert!(
        module_global_seed_candidates(&hir).is_empty(),
        "an exported binding is reachable under a second name"
    );
}

/// Rule 6b again, from the other side: a module that exports ANYTHING can be
/// re-entered by an importing module mid-init through a cycle, so no
/// init-dominance argument holds for it.
#[test]
fn a_module_that_exports_a_function_is_not_a_seed_site() {
    let mut hir = module_with(
        vec![const_o(false)],
        vec![function(1, "run", vec![Stmt::Return(Some(read_o_a()))])],
    );
    hir.functions[0].is_exported = true;
    assert!(
        module_global_seed_candidates(&hir).is_empty(),
        "an exporting module can be re-entered before its own init finishes"
    );
}

/// Rule 6d, the obligation a function-local never carries. Perry does not
/// enforce TDZ on a module-level const: the cell holds TAG_UNDEFINED and only
/// the GUARDED read path catches it. A user call before the declaration is
/// exactly the window in which a guard-free read would do
/// `TAG_UNDEFINED & POINTER_MASK`, gep and load.
#[test]
fn a_user_call_before_the_declaration_denies_the_module_const() {
    let hir = module_with(
        vec![
            Stmt::Expr(Expr::Call {
                callee: Box::new(Expr::FuncRef(1)),
                args: Vec::new(),
                type_args: Vec::new(),
                byte_offset: 0,
            }),
            const_o(false),
        ],
        vec![function(1, "run", vec![Stmt::Return(Some(read_o_a()))])],
    );
    assert!(
        module_global_seed_candidates(&hir).is_empty(),
        "a reader can have run before the Let, so the cell can be undefined"
    );
}

/// Rule 6d must not be so blunt that it denies the common shape. Allocating a
/// pure record runs no user code, so a second module-level record is still a
/// seed — `const A = {…}; const B = {…};` must prove BOTH.
#[test]
fn a_preceding_record_allocation_does_not_spoil_the_prefix() {
    let mut first = const_o(false);
    if let Stmt::Let { id, name, .. } = &mut first {
        *id = 6;
        *name = "A".to_string();
    }
    let hir = module_with(
        vec![first, const_o(false)],
        vec![function(1, "run", vec![Stmt::Return(Some(read_o_a()))])],
    );
    let seeds = module_global_seed_candidates(&hir);
    assert!(
        seeds.contains_key(&6) && seeds.contains_key(&7),
        "{seeds:?}"
    );
}

/// Rule 6a: `mutable` is what separates `const` from `let`/`var`, and `var` is
/// the binding form a Script mirrors onto the global object.
#[test]
fn a_mutable_module_binding_is_not_a_seed() {
    let hir = module_with(
        vec![const_o(true)],
        vec![function(1, "run", vec![Stmt::Return(Some(read_o_a()))])],
    );
    assert!(module_global_seed_candidates(&hir).is_empty());
}

/// Rule 6a: a binding written anywhere in the module is not single-provenance,
/// and `reassigned_locals_in_module` is the module-wide scan that says so.
#[test]
fn a_module_wide_reassignment_denies_the_seed() {
    let hir = module_with(
        vec![const_o(false)],
        vec![function(
            1,
            "clobber",
            vec![Stmt::Expr(Expr::LocalSet(7, Box::new(new_anon())))],
        )],
    );
    assert!(module_global_seed_candidates(&hir).is_empty());
}
