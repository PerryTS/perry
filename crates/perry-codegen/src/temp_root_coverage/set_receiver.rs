//! #9523: the `Expr::SetHas` / `Expr::SetDelete` RECEIVER is a rooted
//! temporary, not an SSA register.
//!
//! `expr/bigint_set.rs` lowered the receiver, unboxed it to a raw `i64` handle,
//! THEN lowered the value expression — arbitrary user code that can allocate —
//! and consumed the handle after. That is the #6970 shape `MapGet` / `MapHas`
//! / `MapDelete` were fixed for (`math_simple.rs`, `logical_collections.rs`),
//! found live in these two twins by #9522's audit. An evacuating minor inside
//! the value's lowering moves the Set; the handle keeps the pre-move address,
//! and `js_set_has` reads a from-space header. `test-files/test_gap_9523_set_
//! receiver_roots_across_value.ts` is the end-to-end half.
//!
//! # Non-vacuity
//!
//! The positive assertions name the VALUE — the register `js_set_alloc`
//! produced went into a rooted slot, and the consuming helper read its operand
//! back OUT of that slot — so a compiler that roots nothing cannot satisfy
//! them by emitting nothing. The receiver is deliberately a fresh `SetNew`
//! rather than a local read: a load out of a shadow slot is a re-readable
//! location that `root_reload` already re-derives below the collection point,
//! so a `LocalGet` receiver would pass against the unfixed compiler. (The
//! typed-arm test below uses a local on purpose and pins the *temp* slot the
//! fix adds, which `root_reload` never emits.)
//!
//! The negative controls hold the other side: a value that cannot collect
//! leaves no window, so the lowering must stay on its pre-#9523 IR with no
//! temp slot at all.
//!
//! Sabotage: reverting `bigint_set.rs`'s two arms to the eager
//! `unbox_to_i64(lower_expr(set))` fails the positive tests with "is never
//! stored into a rooted slot — it lives its whole life in an SSA register" and
//! the typed-arm test with a temp-slot count of 0.

use super::{allocating, main_ir_for, under_both_lowerings};
use crate::testing::temp_slots::{
    assert_no_temp_rooting, assert_rooted_across, first_call_result, temp_root_slots,
};
use perry_hir::types::Type;
use perry_hir::{Expr, Stmt};

const STRING_SET_LOCAL: u32 = 910;

fn set_has(set: Expr, value: Expr) -> Stmt {
    Stmt::Expr(Expr::SetHas {
        set: Box::new(set),
        value: Box::new(value),
    })
}

fn set_delete(set: Expr, value: Expr) -> Stmt {
    Stmt::Expr(Expr::SetDelete {
        set: Box::new(set),
        value: Box::new(value),
    })
}

/// `const s: Set<string> = new Set()` — the receiver shape the frontend
/// produces (`Expr::SetHas { set: LocalGet(..) }`), typed so the string arm
/// (`js_set_has_string`) is selected.
fn string_set_local() -> Stmt {
    Stmt::Let {
        id: STRING_SET_LOCAL,
        name: "s".to_string(),
        ty: Type::Generic {
            base: "Set".to_string(),
            type_args: vec![Type::String],
        },
        mutable: false,
        init: Some(Expr::SetNew),
    }
}

/// `String({})` — runtime-guaranteed to be a string, so the typed string arm
/// is selected, and a collection point: `ToPrimitive` on an object can run user
/// code, and the object literal itself allocates.
fn allocating_string() -> Expr {
    Expr::StringCoerce(Box::new(allocating()))
}

/// THE GAP (#9523), `has`: the receiver is produced before the value and
/// consumed after it, so it must live in a rooted slot and `js_set_has` must
/// read it back out of that slot.
#[test]
fn a_set_has_receiver_is_rooted_across_an_allocating_value() {
    under_both_lowerings(|lowering| {
        let ir = main_ir_for(
            "set_has_receiver_rooted.ts",
            vec![set_has(Expr::SetNew, allocating())],
        );
        let set = first_call_result(&ir, "js_set_alloc").unwrap_or_else(|| {
            panic!(
                "{lowering}: no call to `js_set_alloc` in `main` — this test has no subject:\n{ir}"
            )
        });
        assert_rooted_across(
            &ir,
            &set,
            "js_set_has",
            &format!(
                "{lowering}: #9523 — the Set receiver is live across the value, which allocates"
            ),
        );
    });
}

/// THE GAP (#9523), `delete`: same window, same contract, the other twin.
#[test]
fn a_set_delete_receiver_is_rooted_across_an_allocating_value() {
    under_both_lowerings(|lowering| {
        let ir = main_ir_for(
            "set_delete_receiver_rooted.ts",
            vec![set_delete(Expr::SetNew, allocating())],
        );
        let set = first_call_result(&ir, "js_set_alloc").unwrap_or_else(|| {
            panic!(
                "{lowering}: no call to `js_set_alloc` in `main` — this test has no subject:\n{ir}"
            )
        });
        assert_rooted_across(
            &ir,
            &set,
            "js_set_delete",
            &format!(
                "{lowering}: #9523 — the Set receiver is live across the value, which allocates"
            ),
        );
    });
}

/// The control on the other side: a value that cannot collect leaves no
/// window, so the receiver must not pay a temp slot and the IR is exactly what
/// it was before #9523. Without this half, a lowering that roots every receiver
/// unconditionally would pass the tests above and pay for it on every `has`.
#[test]
fn a_set_has_with_a_non_allocating_value_pays_no_temp_slot() {
    under_both_lowerings(|lowering| {
        for (name, stmt) in [
            ("set_has_no_gc.ts", set_has(Expr::SetNew, Expr::Integer(7))),
            (
                "set_delete_no_gc.ts",
                set_delete(Expr::SetNew, Expr::Integer(7)),
            ),
        ] {
            let ir = main_ir_for(name, vec![stmt]);
            assert!(
                ir.contains("@js_set_has(") || ir.contains("@js_set_delete("),
                "{lowering}: {name} must reach the generic Set helper, or this proves nothing:\n{ir}"
            );
            assert_no_temp_rooting(
                &ir,
                &format!("{lowering}: {name} — #9523 gate: nothing after the receiver can collect"),
            );
        }
    });
}

/// The typed arms take the same window. A `Set<string>` local with a
/// string-guaranteed, allocating value selects `js_set_has_string`, and the
/// receiver — a plain local read — must be pushed into a TEMP slot for the
/// value's duration. `root_reload` would re-derive the shadow-slot load on its
/// own, but it never emits a temp slot, so the count below is the fix's own
/// signature: exactly one temp slot with the value, none without it.
#[test]
fn a_typed_string_set_receiver_is_temp_rooted_only_when_the_value_collects() {
    under_both_lowerings(|lowering| {
        let rooted = main_ir_for(
            "set_has_string_arm_rooted.ts",
            vec![
                string_set_local(),
                set_has(Expr::LocalGet(STRING_SET_LOCAL), allocating_string()),
            ],
        );
        assert!(
            rooted.contains("@js_set_has_string("),
            "{lowering}: the fixture must select the string arm, or this proves nothing:\n{rooted}"
        );
        let rooted_slots = temp_root_slots(&rooted);
        assert_eq!(
            rooted_slots.len(),
            1,
            "{lowering}: #9523 — the string-arm receiver must be the ONE temp root across \
             the allocating value; got {rooted_slots:?}:\n{rooted}"
        );

        let unrooted = main_ir_for(
            "set_has_string_arm_no_gc.ts",
            vec![
                string_set_local(),
                set_has(
                    Expr::LocalGet(STRING_SET_LOCAL),
                    Expr::String("k".to_string()),
                ),
            ],
        );
        assert!(
            unrooted.contains("@js_set_has_string("),
            "{lowering}: the control must select the same arm:\n{unrooted}"
        );
        assert_no_temp_rooting(
            &unrooted,
            &format!("{lowering}: a literal value cannot collect, so the receiver pays no slot"),
        );
    });
}
