//! #8159: the callee's rooting window at a call site is COMPUTED, not assumed.
//!
//! #8084 fixed a real defect — three call arms lowered the CALLEE into a bare
//! register, lowered the arguments below it, and handed the original register
//! to the consuming call, so under the shipping statepoint lowering nothing
//! marked it and nothing relocated it. It paid for the fix with a hardcoded
//! `collects = true` at every one of those sites, which buys a slot, a re-read
//! and a release per call whether or not anything in the window can collect.
//! On `pipeline` — three closure-typed local calls per record, all with
//! `LocalGet` arguments — that cost 3.9% of the program's instructions, enough
//! to consume another PR's entire measured win on that row.
//!
//! `operand_protection` has always been able to answer this: its `Reuse` arm
//! emits no push, no re-read and no truncate, keeping the pre-#8084 IR byte for
//! byte. What it needed was the truthful window, which is what these tests pin
//! — from BOTH sides, because either half alone passes for nothing. A compiler
//! that roots nothing fails the allocating-argument tests; one that roots
//! unconditionally (#8084's shape) fails the inert-argument tests. The two
//! fixtures in each pair differ in exactly one thing: the argument's kind.

use std::collections::{BTreeMap, BTreeSet};

use super::{allocating, main_ir_for, under_both_lowerings};
use crate::testing::temp_slots::{call_operands, slot_traffic, temp_root_slots, SlotEvent};
use perry_hir::types::{FunctionType, Type};
use perry_hir::{Expr, Stmt};

/// `%reg -> <def>` for every definition in `fn_ir`.
fn defs(fn_ir: &str) -> BTreeMap<String, String> {
    fn_ir
        .lines()
        .map(str::trim)
        .filter_map(|line| line.split_once(" = "))
        .map(|(dst, def)| (dst.trim().to_string(), def.trim().to_string()))
        .collect()
}

/// Does operand `n` of the first call to `consumer` come out of a **pooled
/// temp-root** slot?
///
/// `temp_slots::derives_from_slot_load` cannot answer this one. `slot_traffic`
/// sees every alloca, and the callee here is a `LocalGet` — always a load out
/// of the local's OWN slot — so that helper answers `true` on both sides of
/// this differential and the negative would be vacuous. [`temp_root_slots`] is
/// the filter that separates a pooled temporary from a named local's slot; the
/// def-chain walk below is the one `derives_from_slot_load` performs, because
/// the pooled load is an `i64` while the call wants a NaN-boxed `double` (one
/// `bitcast`) or a masked handle (a `bitcast` and an `and`).
fn operand_comes_from_a_temp_root(fn_ir: &str, consumer: &str, n: usize) -> bool {
    let temps: BTreeSet<String> = temp_root_slots(fn_ir).into_iter().collect();
    let loaded: BTreeSet<String> = slot_traffic(fn_ir)
        .into_iter()
        .filter(|(slot, _)| temps.contains(slot))
        .flat_map(|(_, events)| events)
        .filter_map(|event| match event {
            SlotEvent::Load { into, .. } => Some(into),
            _ => None,
        })
        .collect();
    let defs = defs(fn_ir);
    let Some(mut reg) = call_operands(fn_ir, consumer).and_then(|ops| ops.get(n).cloned()) else {
        return false;
    };
    for _ in 0..6 {
        if loaded.contains(&reg) {
            return true;
        }
        let Some(def) = defs.get(&reg) else {
            return false;
        };
        let Some(next) = def
            .split(|c: char| !(c.is_alphanumeric() || c == '%' || c == '.' || c == '_'))
            .find(|word| word.starts_with('%'))
        else {
            return false;
        };
        reg = next.to_string();
    }
    false
}

/// An `any` local initialized to `undefined` — inert to LOWER (a load), so it
/// is the argument that puts nothing in the callee's window.
fn any_local(id: u32, name: &str) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty: Type::Any,
        mutable: false,
        init: Some(Expr::Undefined),
    }
}

/// A CALLEE local, initialized to an object literal.
///
/// The initializer is load-bearing and cost a first draft of this file its
/// meaning. `expr_is_known_non_pointer_shadow_value` suppresses rooting for a
/// `LocalGet` with no reserved shadow slot whose type proof is not
/// pointer-bearing — so a callee local initialized to `Expr::Undefined` is
/// never rooted no matter what the window says. Both POSITIVE tests failed
/// against a correct compiler, and, worse, both NEGATIVES were passing for
/// that reason rather than for the one they claim. A heap initializer reserves
/// the slot and makes the rooting question apply at all.
fn callee_local(id: u32, name: &str, ty: Type) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty,
        mutable: false,
        init: Some(Expr::Object(Vec::new())),
    }
}

/// The `(r: any) => any` annotation, which is the whole gate on
/// `try_lower_closure_typed_local_call`'s guarded dispatch.
fn stage_type() -> Type {
    Type::Function(FunctionType {
        params: vec![("r".to_string(), Type::Any, false)],
        return_type: Box::new(Type::Any),
        is_async: false,
        is_generator: false,
    })
}

/// `const out = stage(<arg>)` — `pipeline`'s inner loop, once.
///
/// The result is BOUND rather than discarded: a discarded expression statement
/// goes through a different tail (#7590), and this contract is about the call
/// that a real program's value flows out of.
fn closure_call_ir(name: &str, arg: Expr) -> String {
    main_ir_for(
        name,
        vec![
            callee_local(0, "stage", stage_type()),
            any_local(1, "rec"),
            Stmt::Let {
                id: 2,
                name: "out".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::Call {
                    callee: Box::new(Expr::LocalGet(0)),
                    args: vec![arg],
                    type_args: Vec::new(),
                    byte_offset: 0,
                }),
            },
        ],
    )
}

/// `const inst = new ctor(<arg>)` on an `any`-typed callee — the shape that
/// routes to `js_new_function_construct`.
fn dynamic_new_ir(name: &str, arg: Expr) -> String {
    main_ir_for(
        name,
        vec![
            callee_local(0, "ctor", Type::Any),
            any_local(1, "x"),
            Stmt::Let {
                id: 2,
                name: "inst".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::NewDynamic {
                    callee: Box::new(Expr::LocalGet(0)),
                    args: vec![arg],
                    byte_offset: 0,
                }),
            },
        ],
    )
}

/// `stage(rec)` — nothing between the callee's lowering and the unmask can
/// collect, so the callee must reach `js_closure_call1` in the register it was
/// lowered into.
#[test]
fn a_closure_typed_local_call_with_an_inert_argument_roots_no_callee() {
    under_both_lowerings(|lowering| {
        let ir = closure_call_ir("closure_call_inert_arg.ts", Expr::LocalGet(1));
        assert!(
            ir.contains("@js_closure_call1("),
            "{lowering}: the fixture must reach the closure-typed local call \
             arm, or this proves nothing:\n{ir}"
        );
        assert!(
            !operand_comes_from_a_temp_root(&ir, "js_closure_call1", 0),
            "{lowering}: a `LocalGet` argument reaches no collection point, so \
             the callee's window is empty and rooting it is pure cost — a \
             store, a re-read and a clear on every call (#8159):\n{ir}"
        );
    });
}

/// …and the same call with an ALLOCATING argument must root it: the object
/// literal is a collection point, and the register the callee was lowered into
/// is in no live bundle (#7803/#8084).
#[test]
fn a_closure_typed_local_call_roots_the_callee_across_an_allocating_argument() {
    under_both_lowerings(|lowering| {
        let ir = closure_call_ir("closure_call_allocating_arg.ts", allocating());
        assert!(
            ir.contains("@js_closure_call1("),
            "{lowering}: the fixture must reach the closure-typed local call \
             arm, or this proves nothing:\n{ir}"
        );
        assert!(
            operand_comes_from_a_temp_root(&ir, "js_closure_call1", 0),
            "{lowering}: the callee outlives an allocating argument, so it must \
             be re-read from a rooted slot — a bare register is in no live \
             bundle and nothing relocates it (#7803):\n{ir}"
        );
    });
}

/// `new ctor(x)` — same claim on the `js_new_function_construct` arm, whose
/// window is the arguments plus `lower_js_args_array` (an entry alloca and
/// stores, which collect nothing).
#[test]
fn a_dynamic_construction_with_an_inert_argument_roots_neither_callee_nor_argument() {
    under_both_lowerings(|lowering| {
        let ir = dynamic_new_ir("new_dynamic_inert_arg.ts", Expr::LocalGet(1));
        assert!(
            ir.contains("@js_new_function_construct("),
            "{lowering}: the fixture must reach the dynamic-construct arm, or \
             this proves nothing:\n{ir}"
        );
        // Stronger than the closure pair's negative, and available here because
        // this arm saves no implicit `this`: NOTHING in the lowering is rooted.
        crate::testing::temp_slots::assert_no_temp_rooting(&ir, lowering);
    });
}

/// …and the allocating-argument twin, which must still root the callee across
/// it — the 21-hazard population #8084 measured in zod's `schemas.ts`.
#[test]
fn a_dynamic_construction_roots_the_callee_across_an_allocating_argument() {
    under_both_lowerings(|lowering| {
        let ir = dynamic_new_ir("new_dynamic_allocating_arg.ts", allocating());
        assert!(
            ir.contains("@js_new_function_construct("),
            "{lowering}: the fixture must reach the dynamic-construct arm, or \
             this proves nothing:\n{ir}"
        );
        assert!(
            operand_comes_from_a_temp_root(&ir, "js_new_function_construct", 0),
            "{lowering}: the constructor outlives an allocating argument and \
             must be re-read from a rooted slot (#7803):\n{ir}"
        );
    });
}
