//! #6986: `lower_builtin_new`'s (`lower_call/builtin.rs`) multi-argument
//! constructor arms threaded plain `lower_expr` sequences the same way
//! `lower_new_impl_inner`'s three non-class branches did before #7699 —
//! argument 0 finished, sat in a bare SSA register, and a later argument's
//! lowering (or a discard loop lowering the tail for side effects) could
//! collect before argument 0 was read.
//!
//! `Expr::New { class_name, .. }` reaches `lower_builtin_new` whenever
//! `class_name` is not a key in `module.classes` — every fixture below
//! declares no classes at all, so a plain, ungated builtin name (`RegExp`,
//! `EventEmitter`, `DataView`, `SuppressedError`, `WeakMap`) dispatches
//! straight through the built-in path without the `imported_class_sources`
//! setup the pg/sqlite/redis/mongo/decimal/rate-limiter/cron arms need.
//!
//! Each positive test below picks one of the three `adopt_*` shapes
//! `builtin.rs` now uses and asserts the #6951 contract through
//! [`assert_rooted_across`]: the first operand is stored into a rooted slot
//! before the later operand's (or the discard loop's) allocation, and the
//! consuming runtime call re-reads it from that slot rather than taking the
//! producer's own now-possibly-stale register. One negative gate (`RegExp`
//! with two non-allocating arguments) is the differential control every test
//! in this family is paired against — it proves the positive is not vacuous
//! by showing the same lowering emits nothing when nothing can collect.

use super::{allocating, main_ir_for, under_both_lowerings};
use crate::testing::temp_slots::{assert_no_temp_rooting, assert_rooted_across, first_call_result};
use perry_hir::{Expr, Stmt};

/// `Expr::New` for a built-in constructor name — no `Class` is declared for
/// it anywhere in the module, so `ctx.classes.contains_key(class_name)` is
/// false and `lower_new_impl_inner` falls through to `lower_builtin_new`.
fn new_builtin(class_name: &str, args: Vec<Expr>) -> Stmt {
    Stmt::Expr(Expr::New {
        class_name: class_name.to_string(),
        args,
        type_args: Vec::new(),
        byte_offset: 0,
        cap_args_appended: 0,
    })
}

/// `new RegExp(<allocating>, <allocating>)`: `pattern_box` must survive
/// `flags_box`'s lowering, which is itself an allocation.
#[test]
fn regexp_pattern_is_rooted_across_flags_lowering() {
    under_both_lowerings(|lowering| {
        let ir = main_ir_for(
            "regexp_ctor_rooted.ts",
            vec![new_builtin("RegExp", vec![allocating(), allocating()])],
        );
        let pattern = first_call_result(&ir, "js_object_alloc")
            .unwrap_or_else(|| panic!("{lowering}: pattern must allocate:\n{ir}"));
        assert_rooted_across(&ir, &pattern, "js_regexp_construct", lowering);
    });
}

/// The differential control: two non-allocating `RegExp` arguments must cost
/// nothing at all, so the positive test above is not vacuously satisfied by a
/// compiler that roots everything.
#[test]
fn regexp_with_non_allocating_args_emits_no_rooting() {
    under_both_lowerings(|lowering| {
        let ir = main_ir_for(
            "regexp_ctor_no_gc.ts",
            vec![new_builtin(
                "RegExp",
                vec![
                    Expr::String("a+".to_string()),
                    Expr::String("g".to_string()),
                ],
            )],
        );
        assert_no_temp_rooting(&ir, lowering);
    });
}

/// `new EventEmitter(<allocating>, <allocating>)`: this arm's second
/// argument is lowered for its side effects only (`adopt_leading_arg_discard_rest`'s
/// discard loop), never named in the call — `opts` still has to survive that
/// lowering.
#[test]
fn event_emitter_options_is_rooted_across_the_discard_loop() {
    under_both_lowerings(|lowering| {
        let ir = main_ir_for(
            "event_emitter_ctor_rooted.ts",
            vec![new_builtin(
                "EventEmitter",
                vec![allocating(), allocating()],
            )],
        );
        let opts = first_call_result(&ir, "js_object_alloc")
            .unwrap_or_else(|| panic!("{lowering}: opts must allocate:\n{ir}"));
        assert_rooted_across(&ir, &opts, "js_event_emitter_new_with_options", lowering);
    });
}

/// `new DataView(<allocating>, <allocating>, <allocating>)`: three plain
/// sequential operands, no discard loop — `view_box` must survive both
/// `offset_box`'s and `length_box`'s lowering.
#[test]
fn data_view_view_arg_is_rooted_across_offset_and_length() {
    under_both_lowerings(|lowering| {
        let ir = main_ir_for(
            "dataview_ctor_rooted.ts",
            vec![new_builtin(
                "DataView",
                vec![allocating(), allocating(), allocating()],
            )],
        );
        let view = first_call_result(&ir, "js_object_alloc")
            .unwrap_or_else(|| panic!("{lowering}: view_box must allocate:\n{ir}"));
        assert_rooted_across(&ir, &view, "js_data_view_new", lowering);
    });
}

/// `new SuppressedError(<allocating>, <allocating>, <allocating>)`: `error`
/// must survive both `suppressed`'s and `message`'s lowering.
#[test]
fn suppressed_error_first_arg_is_rooted_across_the_others() {
    under_both_lowerings(|lowering| {
        let ir = main_ir_for(
            "suppressed_error_ctor_rooted.ts",
            vec![new_builtin(
                "SuppressedError",
                vec![allocating(), allocating(), allocating()],
            )],
        );
        let error = first_call_result(&ir, "js_object_alloc")
            .unwrap_or_else(|| panic!("{lowering}: error must allocate:\n{ir}"));
        assert_rooted_across(&ir, &error, "js_suppressed_error_new", lowering);
    });
}

/// `new WeakMap(<allocating>)`: the hazard here is not another ARGUMENT —
/// `WeakMap` takes one — it is `js_weakmap_new` itself, an unconditional
/// allocation that always runs between the iterable's lowering and its use in
/// `js_weakmap_init_iterable`. Pre-fix this arm eagerly `.map(lower_expr)`'d
/// every argument, called `js_weakmap_new`, and only THEN read
/// `lowered_args.first()` back out of its now-possibly-stale register.
#[test]
fn weakmap_iterable_is_rooted_across_the_allocation_call() {
    under_both_lowerings(|lowering| {
        let ir = main_ir_for(
            "weakmap_ctor_rooted.ts",
            vec![new_builtin("WeakMap", vec![allocating()])],
        );
        let iterable = first_call_result(&ir, "js_object_alloc")
            .unwrap_or_else(|| panic!("{lowering}: iterable must allocate:\n{ir}"));
        assert_rooted_across(&ir, &iterable, "js_weakmap_init_iterable", lowering);
    });
}
