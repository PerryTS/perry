//! Constructor-free construction — IR-census tests for
//! [`super::ctor_prologue_stores`].
//!
//! These are the "assert the subject was live" kind (CLAUDE.md). The change is
//! invisible to every behavioural test: a program whose predicate silently
//! answers `false` everywhere still compiles, still prints the right answer, and
//! is merely as slow as it was before. `js_gc_declare_typed_shape_layout` was
//! 30% of `churn_alloc` and nothing but a profile said so — the same shape of
//! mistake is available here, so the positive test asserts the fast arm exists
//! AND that it carries the stores, while the negative fixtures assert it is
//! absent for each shape that must keep the call.
//!
//! The module reuses the `#7834` bake tests' fixtures verbatim
//! (`typed_shape_bake_tests`) so the pointer-free baked path and the
//! pointer-bearing runtime-declared path differ by one field type only.

use super::typed_shape_bake_tests::{emit, loop_new_module, outlined_new_module};
use perry_hir::types::Type;
use perry_hir::{BinaryOp, Expr, Stmt};

/// The label the fast arm's basic block carries.
const FAST_BLOCK: &str = "ctor_prologue.fast";

/// How many `store double` instructions appear inside the first
/// `ctor_prologue.fast` block — the arm is a straight line, so counting to its
/// terminator is exact.
fn fast_arm_double_stores(ir: &str) -> usize {
    if ir.contains("phi i1 [ true, %alloc.fast.") {
        let start = ir
            .find("\nalloc.fast.")
            .expect("complete birth must be live");
        let body = &ir[start..];
        let end = body.find("\n  br ").unwrap();
        assert!(!body[..end].contains("call "), "{body}");
        // The two-field fixture writes both final values in one vector.
        return body[..end].matches("insertelement <2 x i64>").count();
    }
    let Some(start) = ir.find(FAST_BLOCK) else {
        return 0;
    };
    let body = &ir[start..];
    let end = body.find("\n  br ").unwrap_or(body.len());
    body[..end].matches("store double").count()
}

/// `class Pair { a: number; b: number }` constructed in a loop: the whole
/// constructor is `this.a = a; this.b = b`, the allocation baked its header, so
/// the two stores land here and the call is on the cold arm.
#[test]
fn a_prologue_only_ctor_stores_its_fields_at_the_new_site() {
    let ir = emit(&loop_new_module("Pair", Type::Number, Expr::Integer(2)));
    assert!(
        ir.contains(FAST_BLOCK),
        "no constructor-free arm was emitted for a two-`number` class whose \
         whole constructor is `this.a = a; this.b = b` — the predicate answered \
         `false` and every construction still pays the call plus two full \
         class-field prechecks:\n{ir}"
    );
    assert_eq!(
        fast_arm_double_stores(&ir),
        2,
        "the constructor-free arm exists but does not store both fields — an \
         arm that stores fewer fields than the constructor did is a WRONG \
         ANSWER, not a slow one:\n{ir}"
    );
}

#[test]
fn a_boolean_only_shape_uses_the_allocation_contract_without_gc_mask_profitability() {
    let mut module = loop_new_module("__AnonShape_boolean", Type::Boolean, Expr::Bool(true));
    for field in &mut module.classes[0].fields {
        field.ty = Type::Boolean;
    }
    for param in &mut module.classes[0].constructor.as_mut().unwrap().params {
        param.ty = Type::Boolean;
    }
    module.classes[0].constructor.as_mut().unwrap().is_strict = true;
    let Stmt::For { body, .. } = &mut module.init[0] else {
        panic!("fixture must retain its loop");
    };
    let Stmt::Let {
        init: Some(Expr::New { args, .. }),
        ..
    } = &mut body[0]
    else {
        panic!("fixture must retain its birth");
    };
    args[0] = Expr::Bool(false);
    assert!(module.classes[0].is_literal_shape());
    let ir = emit(&module);
    assert!(
        ir.contains(FAST_BLOCK),
        "boolean-only shape lost its direct stores:\n{ir}"
    );
    assert_eq!(fast_arm_double_stores(&ir), 2, "{ir}");
    assert!(
        !ir.lines()
            .any(|line| line.contains("call ") && line.contains("@js_ctor_return_override")),
        "a complete field-store constructor cannot override its instance:\n{ir}"
    );
    let mut dynamic = module.clone();
    let Stmt::For { body, .. } = &mut dynamic.init[0] else {
        unreachable!()
    };
    let Stmt::Let {
        init: Some(Expr::New { args, .. }),
        ..
    } = &mut body[0]
    else {
        unreachable!()
    };
    args[0] = Expr::LocalGet(7);
    let dynamic_ir = emit(&dynamic);
    assert!(
        dynamic_ir
            .lines()
            .any(|line| line.contains("call ") && line.contains("@js_ctor_return_override")),
        "the ordinary constructor witness must retain its return check"
    );
    assert!(
        !dynamic_ir.contains(FAST_BLOCK),
        "dynamic boxed arguments exceed the replacement budget:\n{dynamic_ir}"
    );
    assert!(
        dynamic_ir.contains("@perry_birth_class"),
        "shared initializer must remain live"
    );
    module.classes[0].constructor.as_mut().unwrap().is_strict = false;
    assert!(!module.classes[0].is_literal_shape());
    assert!(!emit(&module).contains(FAST_BLOCK));
}

/// `class Link { a: number; b: Link | null }` has a non-empty pointer mask.
/// Its birth ShapeId carries the rep, so the new site can store both fields
/// directly and skip the constructor call.
#[test]
fn a_pointer_bearing_shape_stores_at_the_new_site_without_header_layout() {
    let ir = emit(&loop_new_module(
        "Link",
        Type::Union(vec![Type::Named("Link".to_string()), Type::Null]),
        Expr::Null,
    ));
    assert!(
        ir.contains(FAST_BLOCK),
        "a declared pointer-bearing shape did not get a constructor-free arm:\n{ir}"
    );
    assert_eq!(
        fast_arm_double_stores(&ir),
        2,
        "the mixed-layout fast arm must store both the raw-f64 and pointer fields:\n{ir}"
    );
    assert!(
        !ir.contains("and i16 ") || !ir.contains(", 4096"),
        "the mixed-layout arm still tests the retired object INTACT bit:\n{ir}"
    );
}

/// Wide records commonly initialize adjacent numeric fields as `seed + k`.
/// Keep that pure typed arithmetic at the construction site instead of making
/// those otherwise-trivial constructors ineligible as a group.
#[test]
fn a_finite_numeric_param_offset_is_stored_at_the_new_site() {
    let mut module = loop_new_module("Pair", Type::Number, Expr::Integer(2));
    let ctor = module.classes[0].constructor.as_mut().unwrap();
    let seed = match &ctor.body[0] {
        Stmt::Expr(Expr::PropertySet { value, .. }) => value.clone(),
        other => panic!("unexpected first constructor statement: {other:?}"),
    };
    match &mut ctor.body[1] {
        Stmt::Expr(Expr::PropertySet { value, .. }) => {
            *value = Box::new(Expr::Binary {
                op: BinaryOp::Add,
                left: seed,
                right: Box::new(Expr::Integer(1)),
            });
        }
        other => panic!("unexpected second constructor statement: {other:?}"),
    }
    let ir = emit(&module);
    assert!(ir.contains(FAST_BLOCK), "{ir}");
    assert_eq!(fast_arm_double_stores(&ir), 2, "{ir}");
    assert!(
        ir.contains("fadd double"),
        "the derived numeric field was not rebuilt at the new site:\n{ir}"
    );
}

/// A pointer-bearing shape on the stamped outlined allocator must retain the
/// constructor's pointer-barrier semantics, including the incremental-marking
/// gate.
#[test]
fn an_outlined_pointer_bearing_shape_keeps_its_write_barrier() {
    let ir = emit(&outlined_new_module(
        "Link",
        Type::Union(vec![Type::Named("Link".to_string()), Type::Null]),
        Expr::Null,
    ));
    assert!(
        ir.contains(FAST_BLOCK),
        "a stamped outlined allocation did not get a constructor-free arm:\n{ir}"
    );
    assert_eq!(fast_arm_double_stores(&ir), 2, "{ir}");
    assert!(
        ir.contains("and i8")
            && ir.contains(", 32")
            && ir.contains("@PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT"),
        "the outlined mixed-layout arm must retain every required pointer barrier:\n{ir}"
    );
    assert!(
        ir.contains("ctor_prologue.barrier.maybe")
            && ir.contains("call void @js_write_barrier_slot"),
        "the direct pointer store must reach the ordinary write-barrier call:\n{ir}"
    );
}
