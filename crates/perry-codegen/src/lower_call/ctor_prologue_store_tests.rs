//! Constructor-free construction — IR-census tests for
//! [`super::ctor_prologue_stores`].
//!
//! These are the "assert the subject was live" kind (CLAUDE.md). The change is
//! invisible to every behavioural test: a program whose predicate silently
//! answers `false` everywhere still compiles, still prints the right answer, and
//! is merely as slow as it was before. `js_gc_declare_typed_shape_layout` was
//! 30% of `churn_alloc` and nothing but a profile said so — the same shape of
//! mistake is available here, so the positive test asserts the fast arm exists
//! AND that it carries the stores, and the negatives assert it is absent for
//! each shape that must keep the call.
//!
//! The module reuses the `#7834` bake tests' fixtures verbatim
//! (`typed_shape_bake_tests`), because the qualifying population is a subset of
//! that ticket's: `typed_layout_baked` is the first thing
//! `prologue_store_plan` tests.

use super::typed_shape_bake_tests::{emit, loop_new_module};
use perry_hir::types::Type;
use perry_hir::Expr;

/// The label the fast arm's basic block carries.
const FAST_BLOCK: &str = "ctor_prologue.fast";

/// How many `store double` instructions appear inside the first
/// `ctor_prologue.fast` block — the arm is a straight line, so counting to its
/// terminator is exact.
fn fast_arm_double_stores(ir: &str) -> usize {
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

/// The control. `class Link { a: number; b: Link | null }` has a non-empty
/// pointer mask, so `typed_layout_baked` is false, so none of the header
/// constants this change reads as proof were written — and the arm must not be
/// emitted at all.
#[test]
fn a_pointer_bearing_shape_keeps_the_constructor_call() {
    let ir = emit(&loop_new_module(
        "Link",
        Type::Union(vec![Type::Named("Link".to_string()), Type::Null]),
        Expr::Null,
    ));
    assert!(
        !ir.contains(FAST_BLOCK),
        "a pointer-bearing shape took the constructor-free arm. Its header is \
         GC_LAYOUT_SIDE_MASK with no INTACT bit, so the precheck conditions \
         this arm skips are NOT statically true for it:\n{ir}"
    );
}
