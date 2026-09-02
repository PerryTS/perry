//! PropertySet (obj.prop = v).
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.
//!
//! # Rooting (Layer 1, slice 4)
//!
//! Migrated onto [`crate::rooting`]; this module names no `expr::temp_root`
//! symbol. Every store here is the same shape — the receiver is lowered first
//! because `Set(O, k, v)` evaluates the reference before the value, so it sits
//! in an SSA register while arbitrary user code runs — and that shape is
//! exactly one call to [`crate::rooting::with_operands_rooted`] (the value is
//! lowered by `lower_expr`) or
//! [`crate::rooting::with_operands_rooted_across`] (the value is lowered by
//! `lower_value_for_dynamic_property_set`, which this API cannot produce).
//! The class-field family has one deliberate split: `LocalGet` / `This`
//! receivers keep their existing zero-cost `root_reload` repair, while a
//! compound receiver uses an operand group because that pass cannot rederive
//! its phi result (#7640 section C).
//!
//! The migration found one arm that had no guard at all: `arr.length = f()`
//! (#7637). It is the same #7154 window every sibling arm already closed, and
//! it is closed here by putting the receiver in an operand group rather than by
//! a fourth hand-written guard.

use anyhow::Result;
use perry_hir::Expr;

use crate::nanbox::POINTER_MASK_I64;
use crate::native_value::{
    BoundsState, BufferAccessMode, ExpectedNativeRep, LoweredValue, MaterializationReason,
    NativeRep, SemanticKind,
};
use crate::rooting;
use crate::type_analysis::{
    expr_may_return_boxed_value_from_raw_f64_fallback, is_numeric_expr, receiver_class_name,
};
use crate::types::{DOUBLE, I1, I32, I64, I8, PTR};

use super::{
    class_field_store_layout_note_is_conforming, class_field_store_needs_layout_note,
    class_field_store_needs_string_addref, emit_jsvalue_slot_store_pointer_tested,
    emit_typed_feedback_register_site, expr_produces_non_pointer_bits_by_construction, lower_expr,
    lower_expr_native, raw_f64_layout_fact, try_lower_pod_field_set,
    typed_feedback_emission_enabled, unbox_to_i64, FnCtx, TypedFeedbackContract, TypedFeedbackKind,
};

/// Metadata-only class candidate for the runtime-guarded plain-field store.
/// Accessor calls still require a real receiver proof; a lying annotation is
/// routed to the by-name setter path instead.
fn guarded_declared_class_store_candidate(ctx: &FnCtx<'_>, object: &Expr) -> Option<String> {
    let Expr::LocalGet(id) = object else {
        return None;
    };
    let perry_hir::types::Type::Named(name) = ctx.local_type_hint(id)? else {
        return None;
    };
    ctx.classes.contains_key(name).then(|| name.clone())
}

fn canonicalize_raw_f64_numeric_store_value(
    blk: &mut crate::block::LlBlock,
    value_double: &str,
) -> String {
    blk.call(
        DOUBLE,
        "js_array_numeric_value_to_raw_f64",
        &[(DOUBLE, value_double)],
    )
}

/// Lower the receiver/value pair for the class-field and setter fast paths.
///
/// `root_reload` already repairs the common bare-local / `this` receiver, and
/// keeping that path direct preserves its hot IR. A compound receiver is a call
/// result/phi with no storage the reload pass can name, so #7640 requires an
/// explicit operand group whenever the RHS can collect. The group itself keeps
/// inert RHSs byte-identical by answering `Reuse`.
fn with_class_store_operands<'f, R>(
    ctx: &mut FnCtx<'f>,
    object: &Expr,
    value: &Expr,
    body: impl FnOnce(&mut FnCtx<'f>, String, String) -> Result<R>,
) -> Result<R> {
    if matches!(object, Expr::LocalGet(_) | Expr::This) {
        let recv_box = lower_expr(ctx, object)?;
        let val_double = lower_expr(ctx, value)?;
        return body(ctx, recv_box, val_double);
    }
    rooting::with_operands_rooted(ctx, &[object, value], |ctx, vals| {
        body(ctx, vals[0].clone(), vals[1].clone())
    })
}

pub(crate) fn class_has_computed_runtime_members(ctx: &FnCtx<'_>, class_name: &str) -> bool {
    ctx.classes
        .get(class_name)
        .is_some_and(|class| !class.computed_members.is_empty())
}

/// #7288: the SLOPPY-mode arm of the #5093 class-field raw-f64 store.
///
/// `put_value_static_property_fast_path` bars sloppy code from the whole
/// class-field route (#6542) because that route's terminal fallback is
/// `js_object_set_field_by_name`, which throws unconditionally on a
/// non-writable slot — correct for strict `PutValue`, wrong for sloppy, where a
/// rejected write is a silent no-op.
///
/// That bail is far wider than the hazard, and the width is user-visible: an
/// identical `.ts` file compiles to a 46× slower object depending only on
/// whether an upward walk from the source finds a `package.json` with
/// `"type": "module"` (which makes the module ESM, hence strict). Inside the
/// Perry checkout it does; in a user's scratch directory it does not, so
/// `benchmarks/suite/09_method_calls.ts` measured 83 ms in-tree and 3.8 s
/// anywhere else.
///
/// The fast arm never needed the bail. The #5093 inline precheck
/// (`emit_class_field_inline_precheck`) already rejects every receiver whose
/// store could be *rejected* — `OBJ_FLAG_FROZEN`, `OBJ_FLAG_HAS_DESCRIPTORS`, a
/// mismatched class id or keys token, a cleared typed-layout-intact bit — plus
/// every value that is not a plain finite number, and the process-global gate
/// is flipped by any prototype-level descriptor install naming a declared
/// field. A store that reaches the raw slot is therefore one that could not
/// have been rejected in either mode, so the fast arm is mode-independent.
/// Only the fallback needed strict-awareness, and this sends every miss to
/// `js_put_value_set(..., strict = 0)` — the sloppy-correct runtime the
/// surrounding `PutValueSet` lowering already uses — instead of the throwing
/// by-name setter.
///
/// Scope: a declared field on a known class, receiver == target — raw-f64
/// (`number`) slots and boxed slots alike.
///
/// The boxed half is P1 (#5094). #7288 originally took only the raw-f64 slots
/// because "boxed slots need the layout note and write barrier that the
/// guard-call path emits" — but those are emitted by
/// [`emit_jsvalue_slot_store_pointer_tested`], not by the guard, and this arm
/// calls it with the identical value-side predicates the strict arm uses. What
/// the guard call actually contributes is descriptor-aware dispatch and the
/// setter-in-chain walk, and the inline precheck refuses every receiver that
/// needs either.
///
/// Leaving the boxed slots out was the more expensive half of the omission:
/// a `next: LNode | null` store fell through to the `PutValue` write IC
/// (`expr/proxy_reflect.rs`), whose miss path is `js_put_value_set` →
/// `js_object_set_field_by_name` — by-name dispatch, a `RuntimeHandleScope`,
/// and a per-object side-table touch, for a store whose slot index is a
/// compile-time constant. On `deeplist.ts` that one store was the benchmark.
mod sloppy_class_field;
pub(crate) use sloppy_class_field::try_lower_sloppy_class_field_store;

fn lower_runtime_property_set_by_name(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    value: &Expr,
    // #9459: `js_object_set_field_by_property_id` resolves the dispatch id and
    // hands the key to `js_object_set_field_by_name`, which has no `strict`
    // parameter and rejects by throwing. Correct for a strict `PutValue`, wrong
    // for sloppy.
    assignment_strict: bool,
) -> Result<String> {
    if !assignment_strict {
        return lower_put_value_property_set_by_name(ctx, object, property, value, false);
    }
    // #7154: root the receiver across the value's evaluation, which allocates.
    // The group re-reads it as part of emitting the store, so no register of
    // the receiver exists across the window.
    rooting::with_operands_rooted(ctx, &[object, value], |ctx, vals| {
        let (recv_box, val_double) = (&vals[0], &vals[1]);
        let key_idx = ctx.strings.intern(property);
        let dispatch_global = ctx.strings.static_dispatch_global(key_idx);
        let blk = ctx.block();
        let obj_bits = blk.bitcast_double_to_i64(recv_box);
        let property_id = crate::strings::emit_static_dispatch_id(blk, &dispatch_global);
        blk.call_void(
            "js_object_set_field_by_property_id",
            &[(I64, &obj_bits), (I64, &property_id), (DOUBLE, val_double)],
        );
        Ok(val_double.clone())
    })
}

/// #9459 / #9495: the terminal by-name store for `Expr::PropertySet`, in BOTH
/// modes.
///
/// `Set(O, P, V, Throw)` -- ordinary `[[Set]]` WITH the receiver. When the
/// receiver has no own property, ES2024 SS10.1.9.2 (`OrdinarySetWithOwnDescriptor`)
/// walks to the parent and lets the PARENT's descriptor decide: an inherited
/// non-writable data property or getter-only accessor rejects, an inherited
/// setter runs with the original receiver, and only a writable data property
/// (or the end of the chain) creates a new own property on the receiver. The
/// rejection is thrown iff `Throw` (SS6.2.5.7 `PutValue`,
/// `Throw = IsStrictReference`). That is exactly
/// `js_put_value_set(target, key, value, receiver, strict)`, the entry `o.x = v`
/// has always used through `Expr::PutValueSet`; routing every `PropertySet`
/// spelling here -- `o.x += 1`, `for (o.x of it)`, `[o.x] = arr` -- makes them
/// agree with it instead of diverging by lane.
///
/// #9459 brought the SLOPPY half here: its old tail rejected by throwing. #9495
/// brings the STRICT half: its old tail,
/// `js_typed_feedback_object_set_field_by_name_fast` ->
/// `js_object_set_field_by_name`, was an OWN-property store with no prototype
/// walk, so a strict `o.x += 1` against an inherited setter never ran the setter,
/// an inherited non-writable / getter-only property never threw, and an own
/// property materialised where the spec creates none.
///
/// The typed-feedback `PropertySet` site moves with the store
/// (`emit_typed_feedback_property_set_observation`): it describes the store,
/// not the store's strictness, so both modes register it.
///
/// `target` and `receiver` are the same expression, evaluated ONCE -- the
/// property reference's base is one evaluation, and `with_operands_rooted_across`
/// hands the single lowered box to both operand slots.
///
/// Rooting is the #7154 window: the receiver is live across `value`'s
/// lowering, which is arbitrary user code and can drive an evacuating minor.
fn lower_put_value_property_set_by_name(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    value: &Expr,
    assignment_strict: bool,
) -> Result<String> {
    rooting::with_operands_rooted_across(
        ctx,
        &[object],
        &[value],
        |ctx| {
            lower_value_for_dynamic_property_set(
                ctx,
                value,
                "property_set.dynamic_value_bits",
                "dynamic_property_set_helper_edge",
            )
        },
        |ctx, vals, (val_double, _val_bits)| {
            let obj_box = vals[0].clone();
            let key_idx = ctx.strings.intern(property);
            let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
            let obj_bits = ctx.block().bitcast_double_to_i64(&obj_box);
            // `undefined.x = 1` is a TypeError in BOTH modes: `GetValue` on the
            // base runs before `PutValue` ever consults `Throw`.
            emit_nullish_write_guard(ctx, &obj_bits, property, "pset");
            let key_box = ctx.block().load(DOUBLE, &key_handle_global);
            emit_typed_feedback_property_set_observation(ctx, property, &obj_bits, |ctx| {
                // The key is an interned heap string, so its `StringHeader*` is
                // a mask, never an allocation.
                let key_bits = ctx.block().bitcast_double_to_i64(&key_box);
                ctx.block().and(I64, &key_bits, POINTER_MASK_I64)
            });
            let strict_flag = if assignment_strict { "1" } else { "0" };
            let _ = ctx.block().call(
                DOUBLE,
                "js_put_value_set",
                &[
                    (DOUBLE, &obj_box),
                    (DOUBLE, &key_box),
                    (DOUBLE, &val_double),
                    (DOUBLE, &obj_box),
                    (I32, strict_flag),
                ],
            );
            Ok(val_double)
        },
    )
}

/// #9495: the typed-feedback `PropertySet` observation for a by-name store
/// whose tail is `js_put_value_set`.
///
/// The old strict tail folded the observation into a DISPATCHING wrapper
/// (`js_typed_feedback_object_set_field_by_name_fast`), emitted in every build
/// because it also chose between the shape-transition fast path and the
/// by-name setter. The receiver-aware `[[Set]]` makes that choice inside
/// `js_put_value_set`, so nothing is left for a wrapper to decide and the
/// observation stands alone -- a pure-recording helper, compile-gated on
/// `PERRY_TYPED_FEEDBACK` / `_TRACE` like every other one since #7480 step 4. A
/// default build emits the bare `js_put_value_set` call and nothing else.
///
/// The site is always REGISTERED (a no-op call-free counter bump in a default
/// build) so that `ic_site_counter` -- and with it every later inline-cache
/// global name in the function -- is numbered exactly as it was when the
/// dispatching wrapper stood here.
///
/// `key_raw` derives the bare `StringHeader*` the observer hashes, and is only
/// run in an emitting build. It runs BEFORE the observe call and after nothing
/// else, so a derivation that can allocate (an SSO key materialising onto the
/// heap -- #7640 section D) precedes no raw receiver handle: `obj_bits` is the
/// NaN-boxed receiver the rooting group re-read.
pub(super) fn emit_typed_feedback_property_set_observation(
    ctx: &mut FnCtx<'_>,
    operation: &str,
    obj_bits: &str,
    key_raw: impl FnOnce(&mut FnCtx<'_>) -> String,
) {
    let site_id = emit_typed_feedback_register_site(
        ctx,
        TypedFeedbackKind::PropertySet,
        operation,
        TypedFeedbackContract::put_value_set(),
    );
    if !typed_feedback_emission_enabled() {
        return;
    }
    let key_raw = key_raw(ctx);
    ctx.block().call_void(
        "js_typed_feedback_observe_property_set",
        &[(I64, &site_id), (I64, obj_bits), (I64, &key_raw)],
    );
}

fn lower_value_for_dynamic_property_set(
    ctx: &mut FnCtx<'_>,
    value: &Expr,
    consumer: &str,
    boxed_at: &str,
) -> Result<(String, String)> {
    let lowered = lower_expr_native(ctx, value, ExpectedNativeRep::JsValueBits)?;
    let value_bits = lowered.value.clone();
    let value_double = ctx.block().bitcast_i64_to_double(&value_bits);
    ctx.record_lowered_value(
        "PropertySet",
        None,
        consumer,
        &lowered,
        None,
        None,
        None,
        false,
        false,
        vec![format!("boxed_at={boxed_at}")],
    );
    Ok((value_double, value_bits))
}

pub(crate) fn emit_nullish_write_guard(
    ctx: &mut FnCtx<'_>,
    obj_bits: &str,
    property: &str,
    label_prefix: &str,
) {
    let is_undef = ctx
        .block()
        .icmp_eq(I64, obj_bits, crate::nanbox::TAG_UNDEFINED_I64);
    let is_null = ctx
        .block()
        .icmp_eq(I64, obj_bits, crate::nanbox::TAG_NULL_I64);
    let is_nullish = ctx.block().or(I1, &is_undef, &is_null);
    let throw_idx = ctx.new_block(&format!("{}.throw_nullish", label_prefix));
    let ok_idx = ctx.new_block(&format!("{}.recv_ok", label_prefix));
    let throw_label = ctx.block_label(throw_idx);
    let ok_label = ctx.block_label(ok_idx);
    ctx.block().cond_br(&is_nullish, &throw_label, &ok_label);

    ctx.current_block = throw_idx;
    let key_idx = ctx.strings.intern(property);
    let prop_entry = ctx.strings.entry(key_idx);
    let prop_bytes_global = format!("@{}", prop_entry.bytes_global);
    let prop_len_str = prop_entry.byte_len.to_string();
    let is_null_i32 = ctx.block().zext(I1, &is_null, I32);
    ctx.block().call_void(
        "js_throw_type_error_property_access",
        &[
            (I32, &is_null_i32),
            (PTR, &prop_bytes_global),
            (I64, &prop_len_str),
        ],
    );
    ctx.block().unreachable();

    ctx.current_block = ok_idx;
}

/// Lower an `Expr::PropertySet`.
///
/// `assignment_strict` is the assignment's own `Throw` flag (ES2024 SS6.2.5.7
/// `PutValue` calls `Set(O, P, V, Throw)` with `Throw = IsStrictReference`).
/// #9459: the HIR node carries no strictness, so it comes from the caller --
/// `ctx.is_strict_fn` for the ordinary dispatch (`expr/dispatch.rs`, the same
/// source `Expr::IndexSet` uses since #9426), and `PutValueSet::strict` for the
/// two routes that synthesize a `PropertySet` from a `PutValue`
/// (`expr/proxy_reflect.rs`). A rejected SLOPPY `[[Set]]` is a silent no-op, so
/// every arm below whose runtime entry rejects by THROWING is strict-only; the
/// sloppy twin of each is the strictness-aware `js_put_value_set(..., 0)` that
/// the surrounding `PutValueSet` lowering already uses for `o.x = v`.
///
/// #9495: the generic by-name tail is that same receiver-aware `[[Set]]` in BOTH
/// modes (`lower_put_value_property_set_by_name`). The strict tail used to be an
/// own-property store that skipped the prototype walk; see the helper.
pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr, assignment_strict: bool) -> Result<String> {
    match expr {
        Expr::PropertySet {
            object,
            property,
            value,
        } => {
            if let Expr::LocalGet(id) = object.as_ref() {
                if ctx.pod_records.get(id).is_some_and(|local| {
                    local
                        .layout
                        .fields
                        .iter()
                        .any(|field| field.name == *property)
                }) {
                    if let Some(value) = try_lower_pod_field_set(ctx, *id, property, value)? {
                        return Ok(value);
                    }
                }
            }
            // Closes #304: `arr.length = N` must mutate the ArrayHeader, not
            // set a "length" field in the object dispatch. Pre-fix the generic
            // `js_object_set_field_by_name(arr, "length", N)` path silently
            // recorded a property on the array's hidden dispatch object but
            // never touched the real ArrayHeader.length, so subsequent reads
            // of `arr.length` returned the stale original count and the
            // elements stayed live. Statically Array-typed receivers route to
            // `js_array_set_length` which truncates / extends the header.
            // Open question: dynamic `Any`-typed receivers that happen to be
            // arrays at runtime still hit the generic path and miss the fix —
            // they'd need a runtime-side check inside js_object_set_field_by_name
            // (route to js_array_set_length when the target is registered as
            // an array). Deliberately out of scope here; the static-typed
            // case covers the issue's repro.
            //
            // #9459: strict only. `js_array_set_length_strict` is named for the
            // `Throw` flag it hard-codes -- `Set(O, "length", n, true)`. A SLOPPY
            // `arr.length` write that `OrdinarySet` rejects (frozen array, or an
            // explicit `writable: false` on `length`) must be a silent no-op, so
            // sloppy falls through to the generic `js_put_value_set(..., 0)` tail
            // below. That is already where sloppy `arr.length = 0` goes today --
            // `put_value_static_property_fast_path` refuses this arm for sloppy
            // references (`expr/proxy_reflect.rs`), and #9422's fixture pins the
            // result -- so the two spellings agree rather than diverging by lane.
            if assignment_strict
                && property == "length"
                && crate::type_analysis::is_array_expr(ctx, object)
            {
                // #7637: this arm had NO store-operand guard, while every other
                // `PropertySet` arm in this file has had one since #7154. It is
                // the same window: `arr.length = f()` lowers the receiver first
                // (spec order), `f()` allocates and can drive an evacuating
                // minor, and `js_array_set_length_strict` then truncates through
                // a pre-move `ArrayHeader*` — the array the program keeps is
                // left at its old length. The receiver's own slot is a root the
                // collector rewrites; the register is not.
                return rooting::with_operands_rooted(ctx, &[object, value], |ctx, vals| {
                    let (arr_box, val_double) = (&vals[0], &vals[1]);
                    let blk = ctx.block();
                    let arr_bits = blk.bitcast_double_to_i64(arr_box);
                    let arr_handle = blk.and(I64, &arr_bits, POINTER_MASK_I64);
                    // `arr.length = v` is a strict `Set(O,"length",v,true)`: a
                    // frozen array's `length` is non-writable, so route to the
                    // throwing variant instead of the silent internal helper.
                    blk.call_void(
                        "js_array_set_length_strict",
                        &[(I64, &arr_handle), (DOUBLE, val_double)],
                    );
                    Ok(val_double.clone())
                });
            }
            // #1344: `process.env.X = v` must persist to the real OS
            // environment, not just a cached ProcessEnv object backing.
            // Pre-fix the generic `js_object_set_field_by_name` path
            // stored on the cached dict but `process.env.X` (`EnvGet`)
            // reads from `std::env::var` directly, so the value never
            // round-tripped and child processes inherited the
            // unmodified parent env.
            //
            // Route the store through `js_setenv(key, value)` (writes
            // via `std::env::set_var`, coerces non-string values to
            // strings via `js_jsvalue_to_string`). Reads still go
            // through `js_getenv_value`, so the round-trip works.
            if matches!(object.as_ref(), Expr::ProcessEnv) {
                let key_idx = ctx.strings.intern(property);
                let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
                let val_double = lower_expr(ctx, value)?;
                let blk = ctx.block();
                let key_box = blk.load(DOUBLE, &key_handle_global);
                let key_handle = unbox_to_i64(blk, &key_box);
                blk.call_void("js_setenv", &[(I64, &key_handle), (DOUBLE, &val_double)]);
                return Ok(val_double);
            }
            // Scalar replacement fast path: store to the field's alloca.
            if let Expr::LocalGet(id) = object.as_ref() {
                if let Some(slot) = ctx
                    .scalar_replaced
                    .get(id)
                    .and_then(|fs| fs.get(property.as_str()))
                    .cloned()
                {
                    let raw_f64_field = crate::type_analysis::scalar_replaced_field_is_raw_f64(
                        ctx,
                        object.as_ref(),
                        property,
                    );
                    let numeric_store = raw_f64_field
                        && is_numeric_expr(ctx, value)
                        && !expr_may_return_boxed_value_from_raw_f64_fallback(ctx, value);
                    let val_double = lower_expr(ctx, value)?;
                    let stored_value = if numeric_store {
                        canonicalize_raw_f64_numeric_store_value(ctx.block(), &val_double)
                    } else {
                        val_double.clone()
                    };
                    ctx.block().store(DOUBLE, &stored_value, &slot);
                    // #6968: bind the field alloca as a precise GC root, the
                    // same treatment `emit_shadow_slot_update_for_expr` gives
                    // an ordinary pointer-typed local. Skipped for a
                    // `numeric_store`, whose stored bits are a canonicalized
                    // raw `f64` by construction.
                    if !numeric_store {
                        crate::expr::root_scalar_replaced_slot(ctx, &slot, value);
                    }
                    // String-alias fix (mirror of `let y = x` in stmt/let_stmt.rs):
                    // a string-typed local stored into a scalar-replaced field's
                    // alloca slot aliases the same heap buffer. The runtime
                    // write-barrier choke point (runtime_store_jsvalue_slot) can't
                    // see this store because scalar replacement elides the real
                    // heap object, so mark the buffer shared here. Otherwise a
                    // later `s = s + suffix` mutates it in-place via
                    // js_string_append's refcount==1 fast path and corrupts this
                    // field. The helper checks the runtime tag, so apply it to
                    // every local source: an erased non-string annotation is
                    // not proof that the current value cannot be a string.
                    if matches!(&**value, Expr::LocalGet(_)) {
                        super::helpers::emit_string_addref_if_heap_string(ctx, &val_double);
                    }
                    let lowered_js = LoweredValue {
                        semantic: SemanticKind::JsValue,
                        rep: NativeRep::JsValue,
                        llvm_ty: DOUBLE,
                        value: val_double.clone(),
                    };
                    ctx.record_lowered_value_with_access_mode(
                        "ScalarObjectFieldSet",
                        Some(*id),
                        "scalar_object_field_store",
                        &lowered_js,
                        None,
                        None,
                        None,
                        None,
                        false,
                        false,
                        vec![
                            format!("field={}", property),
                            format!("raw_f64_field={}", raw_f64_field as u8),
                        ],
                    );
                    if numeric_store {
                        let lowered_f64 = LoweredValue::f64(stored_value.clone());
                        ctx.record_lowered_value_with_access_mode(
                            "ScalarObjectFieldSet",
                            Some(*id),
                            "scalar_object_field_store.raw_f64",
                            &lowered_f64,
                            None,
                            None,
                            None,
                            None,
                            false,
                            false,
                            vec![format!("field={}", property), "raw_f64_field=1".to_string()],
                        );
                    }
                    return Ok(val_double);
                }
            }
            // #9460: the local IS scalar-replaced, but this property has no
            // field slot -- so there is nothing to store into, and nothing that
            // could ever read it back.
            //
            // `stmt/let_stmt.rs`'s scalar-replacement arm creates slots for a
            // synthetic `__AnonShape_*` class (an object literal) only for the
            // fields in `non_escaping_new_used_fields`, which by design tracks
            // READS: "writes still need their RHS evaluated for JS side effects,
            // but the scalar slot/store can be elided when the field is never
            // observed" (`collectors/escape_news.rs`). It then registers
            // `ctx.locals[id]` as a DUMMY entry-block alloca that is never
            // initialized, because the binding has stopped being an object at
            // all, and overwrites `local_types[id]` with the synthetic class.
            //
            // Without this arm a slotless store fell through to the class-field /
            // `Ptr<Shape>` lowerings below, which load that dummy slot as an
            // `ObjectHeader*` and store through `null + <header size>` --
            // SIGSEGV on `const o: any = {x:1}; o.x = 7;` with no later read of
            // `o.x`, in BOTH modes. The read side has had the matching guard
            // since the synthetic-shape work (`expr/property_get.rs`, whose
            // comment names the same hazard: "the generic runtime helper that
            // crashes on the dummy slot"); the write side never got it, which is
            // why adding a `console.log(o.x)` made the crash disappear -- the
            // read is what creates the slot.
            //
            // Discarding the store is what the contract above promises and is
            // unobservable: the receiver is a non-escaping fresh literal, so no
            // alias exists, and a read of a slotless field already answers
            // `undefined` on the read side. The RHS is still lowered, so its side
            // effects happen -- same shape as the `this` arm just below, which
            // has always evaluated the value and dropped the store when the
            // inlined constructor's target field has no slot.
            if let Expr::LocalGet(id) = object.as_ref() {
                if ctx.scalar_replaced.contains_key(id) {
                    let val_double = lower_expr(ctx, value)?;
                    let lowered = LoweredValue {
                        semantic: SemanticKind::JsValue,
                        rep: NativeRep::JsValue,
                        llvm_ty: DOUBLE,
                        value: val_double.clone(),
                    };
                    ctx.record_lowered_value_with_access_mode(
                        "ScalarObjectFieldSetElided",
                        Some(*id),
                        "scalar_object_field_store.unobserved",
                        &lowered,
                        None,
                        None,
                        None,
                        None,
                        false,
                        false,
                        vec![
                            format!("field={}", property),
                            "reason=field_never_read_no_scalar_slot".to_string(),
                        ],
                    );
                    return Ok(val_double);
                }
            }
            // Handle `this` during scalar-replaced constructor inlining:
            if let Expr::This = object.as_ref() {
                if let Some(target_id) = ctx.scalar_ctor_target.last().copied() {
                    let maybe_slot = ctx
                        .scalar_replaced
                        .get(&target_id)
                        .and_then(|slots| slots.get(property.as_str()).cloned());
                    let raw_f64_field = crate::type_analysis::scalar_replaced_field_is_raw_f64(
                        ctx,
                        object.as_ref(),
                        property,
                    );
                    let numeric_store = raw_f64_field
                        && is_numeric_expr(ctx, value)
                        && !expr_may_return_boxed_value_from_raw_f64_fallback(ctx, value);
                    let val_double = lower_expr(ctx, value)?;
                    if let Some(slot) = maybe_slot {
                        let stored_value = if numeric_store {
                            canonicalize_raw_f64_numeric_store_value(ctx.block(), &val_double)
                        } else {
                            val_double.clone()
                        };
                        ctx.block().store(DOUBLE, &stored_value, &slot);
                        // #6968: see the `ScalarObjectFieldSet` path above —
                        // an inlined constructor's `this.f = …` writes the
                        // same kind of unrooted per-field alloca.
                        if !numeric_store {
                            crate::expr::root_scalar_replaced_slot(ctx, &slot, value);
                        }
                        // String-alias fix: see the ScalarObjectFieldSet path
                        // above. `this.field = s` into a scalar-replaced ctor slot
                        // aliases the string buffer; mark it shared so a later
                        // self-append doesn't mutate it in-place and corrupt the
                        // field.
                        if matches!(&**value, Expr::LocalGet(_)) {
                            super::helpers::emit_string_addref_if_heap_string(ctx, &val_double);
                        }
                        let lowered_js = LoweredValue {
                            semantic: SemanticKind::JsValue,
                            rep: NativeRep::JsValue,
                            llvm_ty: DOUBLE,
                            value: val_double.clone(),
                        };
                        ctx.record_lowered_value_with_access_mode(
                            "ScalarThisFieldSet",
                            Some(target_id),
                            "scalar_object_field_store",
                            &lowered_js,
                            None,
                            None,
                            None,
                            None,
                            false,
                            false,
                            vec![
                                format!("field={}", property),
                                format!("raw_f64_field={}", raw_f64_field as u8),
                            ],
                        );
                        if numeric_store {
                            let lowered_f64 = LoweredValue::f64(stored_value.clone());
                            ctx.record_lowered_value_with_access_mode(
                                "ScalarThisFieldSet",
                                Some(target_id),
                                "scalar_object_field_store.raw_f64",
                                &lowered_f64,
                                None,
                                None,
                                None,
                                None,
                                false,
                                false,
                                vec![format!("field={}", property), "raw_f64_field=1".to_string()],
                            );
                        }
                    }
                    return Ok(val_double);
                }
            }
            // Setter dispatch: if the receiver is a known class and the
            // property is registered as a setter, call the synthesized
            // __set_<property> method instead of doing a raw field
            // store. The setter takes (this, value) and returns
            // undefined; we forward `value` as the expression result.
            //
            // #7640 section C: `recv_box` below is lowered before `value`
            // exactly like the class-field arms. Keep the zero-cost
            // `LocalGet`/`This` path, and conditionally root a compound
            // receiver across an allocating value expression.
            let proven_class_name = receiver_class_name(ctx, object);
            if let Some(class_name) = proven_class_name
                .clone()
                .or_else(|| guarded_declared_class_store_candidate(ctx, object))
            {
                // #9369, store twin of the read gate in `property_get.rs`:
                // the computed-member route strips the receiver NaN-box to a
                // raw `ObjectHeader*`, which is only meaningful for an
                // INSTANCE. A static body's `this` is the class ref, so the
                // store landed on the bare class id and was lost.
                if class_has_computed_runtime_members(ctx, &class_name)
                    && !ctx.is_static_class_this(object)
                {
                    return lower_runtime_property_set_by_name(
                        ctx,
                        object,
                        property,
                        value,
                        assignment_strict,
                    );
                }
                let setter_key = (class_name.clone(), format!("__set_{}", property));
                // STATIC accessors compile under the static (no-`this`)
                // convention — see the matching gate in property_get.rs.
                let is_static_accessor = ctx
                    .classes
                    .get(&class_name)
                    .map(|c| c.static_accessor_names.iter().any(|n| n == property))
                    .unwrap_or(false);
                if !is_static_accessor {
                    if let Some(fn_name) = ctx.methods.get(&setter_key).cloned() {
                        if proven_class_name.is_none() {
                            return lower_runtime_property_set_by_name(
                                ctx,
                                object,
                                property,
                                value,
                                assignment_strict,
                            );
                        }
                        return with_class_store_operands(
                            ctx,
                            object,
                            value,
                            |ctx, recv_box, val_double| {
                                let _ = ctx.block().call(
                                    DOUBLE,
                                    &fn_name,
                                    &[(DOUBLE, &recv_box), (DOUBLE, &val_double)],
                                );
                                Ok(val_double)
                            },
                        );
                    }
                }
                // #9459: SLOPPY code stops here. Every class-field arm below
                // terminates in `js_class_field_set_ic` /
                // `js_class_field_set_fallback`, whose miss path is
                // `js_object_set_field_by_name` -- no `strict` parameter, rejects
                // by throwing. That is the same reason
                // `put_value_static_property_fast_path` bars sloppy references
                // from this route for `PutValueSet` (#6542), and the recovery is
                // the same one #7288/#5094 built for that lowering: the #5093
                // inline precheck declines every receiver whose store could be
                // REJECTED (frozen, descriptor-bearing, wrong class or keys token,
                // accessor in the chain), so its fast arm is mode-independent and
                // only its miss needed a sloppy-correct tail. A decline lands on
                // the same `js_put_value_set(..., 0)` the generic sloppy tail uses,
                // so the two are one behaviour with two speeds.
                //
                // The setter-dispatch arm above is deliberately AHEAD of this: a
                // compiled `__set_<property>` accessor runs in both modes, and a
                // setter that throws does so because of its own body, not because
                // of the assignment's `Throw` flag.
                if !assignment_strict {
                    if matches!(object.as_ref(), Expr::LocalGet(_) | Expr::This) {
                        if let Some(result) =
                            try_lower_sloppy_class_field_store(ctx, object, property, value)?
                        {
                            return Ok(result);
                        }
                    }
                    return lower_put_value_property_set_by_name(
                        ctx, object, property, value, false,
                    );
                }
                // Fast path: known class instance + plain instance field.
                // The runtime guard checks the receiver's class/shape and
                // descriptor state before this block touches the raw slot.
                //
                // This is "the strict class-field arm below" the sloppy arms
                // in `try_lower_sloppy_class_field_store` name — same
                // `recv_box`-before-`value` order, same #7640 section C split:
                // direct `LocalGet`/`This` stays on root-reload, while a compound
                // receiver is explicitly rooted by `with_class_store_operands`.
                if let Some(field_index) =
                    crate::type_analysis::class_field_global_index(ctx, &class_name, property)
                {
                    if let (Some(&expected_class_id), Some(keys_global_name)) = (
                        ctx.class_ids.get(&class_name),
                        ctx.class_keys_globals.get(&class_name).cloned(),
                    ) {
                        return with_class_store_operands(
                            ctx,
                            object,
                            value,
                            |ctx, recv_box, val_double| {
                                let key_idx = ctx.strings.intern(property);
                                let key_handle_global =
                                    format!("@{}", ctx.strings.entry(key_idx).handle_global);
                                let site_id = emit_typed_feedback_register_site(
                                    ctx,
                                    TypedFeedbackKind::PropertySet,
                                    property,
                                    TypedFeedbackContract::class_field_set(),
                                );
                                let field_idx_str = field_index.to_string();
                                let expected_class_id_str = expected_class_id.to_string();
                                let requires_raw_f64 =
                                    crate::type_analysis::class_field_declared_type(
                                        ctx,
                                        &class_name,
                                        property,
                                    )
                                    .as_ref()
                                    .is_some_and(crate::typed_shape::type_is_raw_f64_candidate);
                                let requires_raw_f64_str = if requires_raw_f64 { "1" } else { "0" };
                                // #5093 loop versioning: inside the fast clone of a
                                // class-field versioned loop, a tracked raw-f64 field
                                // store on the proven receiver lowers to an inline
                                // plain-finite value check + bare slot store on the
                                // preheader-cached object pointer. A value that is
                                // not a plain finite double (±Inf/NaN, or any NaN-box
                                // tag — including INT32-boxed integers) side-exits to
                                // the slow clone's preheader BEFORE the store, so the
                                // slow clone re-executes the whole iteration and
                                // routes the value through the runtime guard exactly
                                // as today (downgrade semantics preserved).
                                if requires_raw_f64 {
                                    let loop_fact =
                                        match object.as_ref() {
                                            Expr::LocalGet(recv_id) => {
                                                crate::expr::class_field_loop_fact_lookup(
                                                    &ctx.class_field_loop_facts,
                                                    *recv_id,
                                                    &class_name,
                                                    property,
                                                )
                                                .filter(|(_, loop_idx)| *loop_idx == field_index)
                                                .map(|(fact, _)| {
                                                    (
                                                        fact.obj_ptr.clone(),
                                                        fact.side_exit_label.clone(),
                                                    )
                                                })
                                            }
                                            _ => None,
                                        };
                                    if let Some((obj_ptr, side_exit_label)) = loop_fact {
                                        let field_idx_str = field_index.to_string();
                                        let store_idx =
                                            ctx.new_block("class_field_loop_store.fast");
                                        let store_label = ctx.block_label(store_idx);
                                        {
                                            let blk = ctx.block();
                                            let val_bits = blk.bitcast_double_to_i64(&val_double);
                                            let finite = crate::expr::class_field_inline_guard::
                                        emit_plain_finite_number_check(blk, &val_bits);
                                            blk.cond_br(&finite, &store_label, &side_exit_label);
                                        }
                                        ctx.current_block = store_idx;
                                        {
                                            let header_skip =
                                                crate::target_layout::object_header_size_bytes(
                                                    ctx.target_triple,
                                                )
                                                .to_string();
                                            let blk = ctx.block();
                                            let fields_base =
                                                blk.gep(I8, &obj_ptr, &[(I64, &header_skip)]);
                                            let field_ptr = blk.gep(
                                                DOUBLE,
                                                &fields_base,
                                                &[(I64, &field_idx_str)],
                                            );
                                            // No raw-f64 canonicalization call is needed:
                                            // INT32-boxed and NaN values — the only
                                            // inputs `js_array_numeric_value_to_raw_f64`
                                            // rewrites — cannot pass the finite check.
                                            //
                                            // GC_STORE_AUDIT(POINTER_FREE): the inline
                                            // finite check proved `val_double` is a
                                            // genuine (unboxed, finite) double, never a
                                            // heap pointer — no edge, no write barrier.
                                            blk.store(DOUBLE, &val_double, &field_ptr);
                                        }
                                        let stored = LoweredValue {
                                            semantic: SemanticKind::JsNumber,
                                            rep: NativeRep::F64,
                                            llvm_ty: DOUBLE,
                                            value: val_double.clone(),
                                        };
                                        ctx.record_lowered_value_with_access_mode_and_facts(
                                            "ClassFieldSet",
                                            None,
                                            "class_field_set.loop_raw_f64_store",
                                            &stored,
                                            Some(BoundsState::Guarded {
                                                guard_id: "class_field_loop_preheader_check"
                                                    .to_string(),
                                            }),
                                            None,
                                            Some(BufferAccessMode::CheckedNative),
                                            None,
                                            None,
                                            None,
                                            vec![raw_f64_layout_fact(
                                                None,
                                                "consumed",
                                                "class_field_loop_preheader_check",
                                                None,
                                            )],
                                            Vec::new(),
                                            false,
                                            false,
                                            vec![
                                                format!("class={}", class_name),
                                                format!("field={}", property),
                                                format!("field_index={}", field_idx_str),
                                                "receiver_proof=loop_preheader_shape_check"
                                                    .to_string(),
                                                "field_layout=raw_f64_slot_array".to_string(),
                                                "loop_versioning=class_field_fast_clone"
                                                    .to_string(),
                                                "rhs_numeric_guard=inline_plain_finite_check"
                                                    .to_string(),
                                                "store_guard_failure=side_exit_slow_restart"
                                                    .to_string(),
                                            ],
                                        );
                                        return Ok(val_double);
                                    }
                                }
                                // Representation-selection Phase 3b: shape-proven
                                // Ptr<Shape> receiver (collectors/ptr_shape.rs) — no
                                // guard call, no shape diamond. Raw-f64 slots keep the
                                // inline plain-finite value check with a cold
                                // `js_class_field_set_fallback` arm (a NaN/Inf/boxed
                                // value must never be stored raw into a scalar-masked
                                // slot — the runtime setter performs the layout
                                // downgrade the GC scan relies on). Boxed slots store
                                // inline with the existing generational write barrier
                                // for possibly-pointer values.
                                // Phase 5a routes `this` here too. The freeze-family
                                // module-wide kill (collectors/proven_this.rs) is what
                                // makes a guard-free STORE through a proven `this`
                                // sound: unlike a Phase 3b local the receiver is
                                // caller-owned and therefore aliased, so a frozen or
                                // sealed target would otherwise silently accept a raw
                                // store where the spec requires a strict TypeError.
                                let ptr_shape_proven = ctx
                                    .ptr_shape_receiver_fact(object.as_ref())
                                    .map(|fact| fact.class_name == class_name)
                                    .unwrap_or(false);
                                if ptr_shape_proven {
                                    ctx.note_ptr_shape_consumed(object.as_ref(), "ptr_shape_set");
                                    let header_skip =
                                        crate::target_layout::object_header_size_bytes(
                                            ctx.target_triple,
                                        )
                                        .to_string();
                                    let field_set_barrier_needed =
                                        !expr_produces_non_pointer_bits_by_construction(ctx, value);
                                    let (obj_bits, obj_handle, field_ptr, val_bits) = {
                                        let blk = ctx.block();
                                        let obj_bits = blk.bitcast_double_to_i64(&recv_box);
                                        let obj_handle = blk.and(I64, &obj_bits, POINTER_MASK_I64);
                                        let obj_ptr = blk.inttoptr(I64, &obj_handle);
                                        let fields_base =
                                            blk.gep(I8, &obj_ptr, &[(I64, &header_skip)]);
                                        let field_ptr =
                                            blk.gep(DOUBLE, &fields_base, &[(I64, &field_idx_str)]);
                                        let val_bits = blk.bitcast_double_to_i64(&val_double);
                                        (obj_bits, obj_handle, field_ptr, val_bits)
                                    };
                                    if requires_raw_f64 {
                                        let store_idx = ctx.new_block("ptr_shape_set.raw_store");
                                        let cold_idx = ctx.new_block("ptr_shape_set.downgrade");
                                        let merge_idx = ctx.new_block("ptr_shape_set.merge");
                                        let store_label = ctx.block_label(store_idx);
                                        let cold_label = ctx.block_label(cold_idx);
                                        let merge_label = ctx.block_label(merge_idx);
                                        {
                                            let blk = ctx.block();
                                            let finite = crate::expr::class_field_inline_guard::
                                        emit_plain_finite_number_check(blk, &val_bits);
                                            blk.cond_br(&finite, &store_label, &cold_label);
                                        }
                                        ctx.current_block = store_idx;
                                        {
                                            // The finite check proved a genuine unboxed
                                            // double (INT32-boxed and every NaN-box tag
                                            // share the all-ones exponent), so no
                                            // canonicalization call is needed.
                                            let blk = ctx.block();
                                            // GC_STORE_AUDIT(POINTER_FREE): pointer-free
                                            // by that proof — no GC pointer reaches the
                                            // slot, so no write barrier.
                                            blk.store(DOUBLE, &val_double, &field_ptr);
                                            blk.br(&merge_label);
                                        }
                                        ctx.current_block = cold_idx;
                                        {
                                            let blk = ctx.block();
                                            let key_box = blk.load(DOUBLE, &key_handle_global);
                                            let key_bits = blk.bitcast_double_to_i64(&key_box);
                                            let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
                                            blk.call_void(
                                                "js_class_field_set_fallback",
                                                &[
                                                    (I64, &site_id),
                                                    (I64, &obj_bits),
                                                    (I64, &key_raw),
                                                    (DOUBLE, &val_double),
                                                ],
                                            );
                                            blk.br(&merge_label);
                                        }
                                        ctx.current_block = merge_idx;
                                    } else {
                                        // Repsel Phase 4b.1: retire the two bookkeeping
                                        // calls that are provably dead here.
                                        //
                                        // The receiver being `Ptr<Shape>`-proven is
                                        // what licenses the layout-note elision. Three
                                        // facts close it:
                                        //
                                        // Both are decided from the VALUE expression,
                                        // and gated independently because they are dead
                                        // under different conditions: the note needs
                                        // "not a pointer", the addref only "not a heap
                                        // string". Neither is keyed on the declared
                                        // field type — Perry does not enforce declared
                                        // types at runtime, so a `boolean` field can
                                        // legitimately receive a string through an
                                        // `any`, and a wrong addref elision there
                                        // silently corrupts it on the next in-place
                                        // append.
                                        //
                                        // `requires_raw_f64` is false on this arm, so
                                        // the raw-f64-mask arm of `layout_note_slot` —
                                        // the one that *must* downgrade — is
                                        // unreachable from here. The full per-layout-
                                        // state argument, including why a pointer store
                                        // into a pointer-masked slot is deliberately
                                        // NOT elided, is on
                                        // `class_field_store_needs_layout_note`.
                                        let layout_note_needed =
                                            class_field_store_needs_layout_note(ctx, value);
                                        let string_addref_needed =
                                            class_field_store_needs_string_addref(ctx, value);
                                        let field_addr = ctx.block().ptrtoint(&field_ptr, I64);
                                        // #7511: whatever these three flags could not
                                        // be proved away statically is decided by ONE
                                        // live test of the stored bits — see
                                        // `emit_jsvalue_slot_store_pointer_tested`.
                                        emit_jsvalue_slot_store_pointer_tested(
                                            ctx,
                                            &field_ptr,
                                            &val_double,
                                            &obj_handle,
                                            &field_idx_str,
                                            string_addref_needed,
                                            layout_note_needed,
                                            &obj_bits,
                                            &field_addr,
                                            field_set_barrier_needed,
                                            class_field_store_layout_note_is_conforming(
                                                ctx,
                                                &class_name,
                                                field_index,
                                            ),
                                            "class_field_set",
                                        );
                                    }
                                    let (semantic, rep) = if requires_raw_f64 {
                                        (SemanticKind::JsNumber, NativeRep::F64)
                                    } else {
                                        (SemanticKind::JsValue, NativeRep::JsValue)
                                    };
                                    let stored = LoweredValue {
                                        semantic,
                                        rep,
                                        llvm_ty: DOUBLE,
                                        value: val_double.clone(),
                                    };
                                    ctx.record_lowered_value_with_access_mode_and_facts(
                                        "ClassFieldSet",
                                        None,
                                        "class_field_set.shape_proven_store",
                                        &stored,
                                        Some(BoundsState::Guarded {
                                            guard_id: "ptr_shape_static_proof".to_string(),
                                        }),
                                        None,
                                        Some(BufferAccessMode::CheckedNative),
                                        None,
                                        None,
                                        None,
                                        if requires_raw_f64 {
                                            vec![raw_f64_layout_fact(
                                                None,
                                                "consumed",
                                                "ptr_shape_static_proof",
                                                None,
                                            )]
                                        } else {
                                            Vec::new()
                                        },
                                        Vec::new(),
                                        false,
                                        false,
                                        vec![
                                            format!("class={}", class_name),
                                            format!("field={}", property),
                                            format!("field_index={}", field_idx_str),
                                            "receiver_proof=ptr_shape_local".to_string(),
                                            format!("field_layout_raw_f64={}", requires_raw_f64),
                                        ],
                                    );
                                    return Ok(val_double);
                                }
                                // #5334 lever B: oversized modules full-outline the entire
                                // class-field-SET IC diamond (guard + fast store +
                                // fallback) to a single `js_class_field_set_ic(...)` call.
                                // This trades a call frame on the (cold, startup-
                                // dominated) field-set path for a large per-site IR
                                // reduction, keeping the function tractable for LLVM's
                                // `-O3` pipeline.
                                // Only the call's own operands are materialized (the key
                                // handle + expected ShapeId), not the inline-store scaffolding.
                                let expected_shape_id = crate::typed_shape::load_class_shape_id(
                                    ctx,
                                    &class_name,
                                    &keys_global_name,
                                );
                                if crate::codegen::full_outline_ic_enabled() {
                                    let key_raw = {
                                        let blk = ctx.block();
                                        let key_box = blk.load(DOUBLE, &key_handle_global);
                                        let key_bits = blk.bitcast_double_to_i64(&key_box);
                                        blk.and(I64, &key_bits, POINTER_MASK_I64)
                                    };
                                    ctx.block().call_void(
                                        "js_class_field_set_ic",
                                        &[
                                            (I64, &site_id),
                                            (DOUBLE, &recv_box),
                                            (I32, &expected_class_id_str),
                                            (I32, &expected_shape_id),
                                            (I64, &key_raw),
                                            (I32, &field_idx_str),
                                            (DOUBLE, &val_double),
                                            (I32, requires_raw_f64_str),
                                        ],
                                    );
                                    return Ok(val_double);
                                }
                                // #5093: build the guard operands once, up front, so both
                                // the inline shape pre-check and the guard-call fallback
                                // can reference them.
                                let (obj_bits, obj_handle, key_raw, val_bits) = {
                                    let blk = ctx.block();
                                    let obj_bits = blk.bitcast_double_to_i64(&recv_box);
                                    let obj_handle = blk.and(I64, &obj_bits, POINTER_MASK_I64);
                                    let key_box = blk.load(DOUBLE, &key_handle_global);
                                    let key_bits = blk.bitcast_double_to_i64(&key_box);
                                    let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
                                    let val_bits = blk.bitcast_double_to_i64(&val_double);
                                    (obj_bits, obj_handle, key_raw, val_bits)
                                };
                                let fast_idx = ctx.new_block("class_field_set.fast");
                                let fallback_idx = ctx.new_block("class_field_set.fallback");
                                let merge_idx = ctx.new_block("class_field_set.merge");
                                let fast_label = ctx.block_label(fast_idx);
                                let fallback_label = ctx.block_label(fallback_idx);
                                let merge_label = ctx.block_label(merge_idx);

                                // #5093: inline shape pre-check. On a hit this branches
                                // straight to the store, skipping the call; on a miss the
                                // guard-call path below runs unchanged.
                                //
                                // #7854: this used to be gated on `requires_raw_f64`,
                                // leaving every BOXED declared field (`string`, a class
                                // type, a union — i.e. most fields of most objects) paying
                                // an unconditional cross-crate
                                // `js_typed_feedback_class_field_set_guard` call per
                                // store, including the synthesized
                                // `__AnonShape_*_constructor` that every closed-shape
                                // object literal runs. The stated reason — "its setter-in-
                                // chain handling and write barrier aren't reproduced
                                // inline" — is answered by
                                // `try_lower_sloppy_class_field_boxed_store`, which has
                                // taken the boxed inline precheck since #7288: the write
                                // barrier, layout note and string demote come from
                                // `emit_jsvalue_slot_store_pointer_tested` (which the
                                // shared `fast_label` block below calls, with the very
                                // same value-side predicates), NOT from the guard; and a
                                // setter in the chain is already refused upstream by
                                // `class_field_global_index`'s `accessor_in_chain`.
                                //
                                // What the precheck proves is a strict subset of the
                                // runtime `class_field_fast_contract`: on a hit the guard
                                // call would have answered "fast" too, so this only
                                // removes a call, never changes which store happens. Every
                                // miss still lands on the guardcall block and the
                                // unchanged strict fallback, so `[[Set]]` rejection and
                                // descriptor dispatch are untouched. `require_raw_f64` is
                                // forwarded rather than hardcoded, so a boxed slot skips
                                // the plain-finite value test (a boxed slot accepts any
                                // `JSValue`) but still proves not-frozen / no per-object
                                // descriptors via `set_value_bits: Some`.
                                //
                                // #7861: and the shape test it emits is widened from the
                                // DECLARED class to that class's subclass closure. Without
                                // this the boxed arm #7854 just un-gated would still miss
                                // 100% of the time for a store in a base class's own
                                // constructor, where `this` is only ever a subclass. The
                                // arms are computed with `requires_raw_f64` rather than a
                                // literal, so a candidate whose declared type disagrees
                                // about the slot's representation is dropped.
                                let subclass_arms =
                             crate::expr::class_field_inline_guard::class_field_subclass_arms(
                                 ctx,
                                 &class_name,
                                 property,
                                 field_index,
                                 requires_raw_f64,
                             );
                                let _guardcall_label =
                            crate::expr::class_field_inline_guard::emit_class_field_inline_precheck(
                                ctx,
                                &obj_bits,
                                &obj_handle,
                                &expected_class_id_str,
                                &expected_shape_id,
                                requires_raw_f64,
                                Some(&val_bits),
                                &fast_label,
                                &subclass_arms,
                            );
                                let guard_ok = ctx.block().call(
                                    I32,
                                    "js_typed_feedback_class_field_set_guard",
                                    &[
                                        (I64, &site_id),
                                        (DOUBLE, &recv_box),
                                        (I32, &expected_class_id_str),
                                        (I32, &expected_shape_id),
                                        (I64, &key_raw),
                                        (I32, &field_idx_str),
                                        (DOUBLE, &val_double),
                                        (I32, requires_raw_f64_str),
                                    ],
                                );
                                let guard_pass = ctx.block().icmp_ne(I32, &guard_ok, "0");
                                ctx.block()
                                    .cond_br(&guard_pass, &fast_label, &fallback_label);

                                ctx.current_block = fast_idx;
                                // #5334 lever D: a value that is a non-pointer by
                                // construction (number / bool / undefined / null /
                                // comparison / arithmetic) creates no parent→child heap
                                // reference, so the generational write barrier is a
                                // semantic no-op and can be skipped. Computed before the
                                // block builder is borrowed below. The LAYOUT NOTE is
                                // kept regardless: it records the slot's pointer-ness for
                                // minor-scan skipping, and a non-pointer write into a
                                // slot that previously held a pointer is a real
                                // transition the GC must observe. Same soundness standard
                                // as the array-store barrier elision.
                                let field_set_barrier_needed =
                                    !expr_produces_non_pointer_bits_by_construction(ctx, value);
                                // #7469: value-side elision of the addref and layout
                                // note on the guarded arm — computed here because the
                                // predicates take `&FnCtx` and the block builder is
                                // borrowed below.
                                let guarded_note_needed =
                                    class_field_store_needs_layout_note(ctx, value);
                                let guarded_addref_needed =
                                    class_field_store_needs_string_addref(ctx, value);
                                let raw_stored_value = {
                                    // arm64_32 watchOS: the object fields region begins at
                                    // `size_of::<ObjectHeader>()` past the user pointer — 16 on
                                    // both LP64 and ILP32 since #8047. A hardcoded offset writes
                                    // class fields to the wrong word when the header changes; the paired inline read
                                    // (`property_get`) and the runtime setter must agree, so
                                    // derive it from the target triple (no-op on 64-bit; see
                                    // `target_layout`).
                                    let header_skip =
                                        crate::target_layout::object_header_size_bytes(
                                            ctx.target_triple,
                                        )
                                        .to_string();
                                    let field_ptr = {
                                        let blk = ctx.block();
                                        let obj_ptr = blk.inttoptr(I64, &obj_handle);
                                        let fields_base =
                                            blk.gep(I8, &obj_ptr, &[(I64, &header_skip)]);
                                        blk.gep(DOUBLE, &fields_base, &[(I64, &field_idx_str)])
                                    };
                                    let raw_stored_value = if requires_raw_f64 {
                                        // Guarded raw-f64 slots are pointer-free by typed
                                        // shape descriptor; non-number writes miss the
                                        // guard and use the boxed setter fallback.
                                        let blk = ctx.block();
                                        let numeric_value =
                                            canonicalize_raw_f64_numeric_store_value(
                                                blk,
                                                &val_double,
                                            );
                                        // GC_STORE_AUDIT(POINTER_FREE): typed raw-f64 class
                                        // slots contain numbers only.
                                        blk.store(DOUBLE, &numeric_value, &field_ptr);
                                        Some(numeric_value)
                                    } else {
                                        // #5334 lever D: skip the barrier when the value
                                        // is a non-pointer by construction. #7469 extends
                                        // the same value-expression gating to the addref
                                        // and layout note — the Phase 4b.1 predicates are
                                        // value-side-only proofs (see their docs: safe in
                                        // every layout state the receiver can be in), so
                                        // they apply on this guarded arm exactly as on
                                        // the ptr-shape-proven arm above. The guard
                                        // passing does not change what the VALUE can be;
                                        // `requires_raw_f64` is false here, which is the
                                        // precondition `class_field_store_needs_layout_note`
                                        // documents.
                                        //
                                        // #7511: this is the arm the shared
                                        // `<class>_constructor` symbol lands on, where the
                                        // value is an opaque function parameter and lever D
                                        // can never fire. Whatever survives it is decided by
                                        // ONE live test of the stored bits instead of three
                                        // cross-crate calls that each re-ask the same
                                        // question — see
                                        // `emit_jsvalue_slot_store_pointer_tested`.
                                        let field_addr = ctx.block().ptrtoint(&field_ptr, I64);
                                        emit_jsvalue_slot_store_pointer_tested(
                                            ctx,
                                            &field_ptr,
                                            &val_double,
                                            &obj_handle,
                                            &field_idx_str,
                                            guarded_addref_needed,
                                            guarded_note_needed,
                                            &obj_bits,
                                            &field_addr,
                                            field_set_barrier_needed,
                                            class_field_store_layout_note_is_conforming(
                                                ctx,
                                                &class_name,
                                                field_index,
                                            ),
                                            "class_field_set",
                                        );
                                        None
                                    };
                                    ctx.block().br(&merge_label);
                                    raw_stored_value
                                };
                                if let Some(numeric_value) = raw_stored_value {
                                    let stored = LoweredValue {
                                        semantic: SemanticKind::JsNumber,
                                        rep: NativeRep::F64,
                                        llvm_ty: DOUBLE,
                                        value: numeric_value.clone(),
                                    };
                                    ctx.record_lowered_value_with_access_mode_and_facts(
                                        "ClassFieldSet",
                                        None,
                                        "class_field_set.raw_f64_store",
                                        &stored,
                                        Some(BoundsState::Guarded {
                                            guard_id: "class_field_set_guard".to_string(),
                                        }),
                                        None,
                                        Some(BufferAccessMode::CheckedNative),
                                        None,
                                        None,
                                        None,
                                        vec![raw_f64_layout_fact(
                                            None,
                                            "consumed",
                                            "class_field_set_guard",
                                            None,
                                        )],
                                        Vec::new(),
                                        false,
                                        false,
                                        vec![
                                    format!("class={}", class_name),
                                    format!("class_id={}", expected_class_id_str),
                                    format!("field={}", property),
                                    format!("field_index={}", field_idx_str),
                                    "receiver_proof=declared_named_receiver_guarded_exact_class"
                                        .to_string(),
                                    "field_layout=raw_f64_slot_array".to_string(),
                                    "pointer_bitmap=non_pointer".to_string(),
                                ],
                                    );
                                    ctx.record_lowered_value_with_access_mode(
                                        "WriteBarrierElided",
                                        None,
                                        "write_barrier.elided_raw_f64_class_field",
                                        &stored,
                                        None,
                                        None,
                                        None,
                                        None,
                                        false,
                                        false,
                                        vec![
                                    "reason=raw_f64_class_field_pointer_free".to_string(),
                                    format!("class={}", class_name),
                                    format!("class_id={}", expected_class_id_str),
                                    format!("field={}", property),
                                    format!("field_index={}", field_idx_str),
                                    "receiver_proof=declared_named_receiver_guarded_exact_class"
                                        .to_string(),
                                    "field_layout=raw_f64_slot_array".to_string(),
                                    "pointer_bitmap=non_pointer".to_string(),
                                ],
                                    );
                                }

                                ctx.current_block = fallback_idx;
                                let blk = ctx.block();
                                // #5334 lever A: the guard already ran and FAILED in the
                                // entry block, so this cold arm is a pure guard-miss
                                // fallback. Outline the two operations it used to emit
                                // inline (record_fallback + by-name set) into ONE
                                // `js_class_field_set_fallback` call. Semantics are
                                // byte-identical; only the emitted IR shrinks (cold path
                                // → zero hot-loop cost). `obj_bits` keeps the full
                                // NaN-box tag; `key_raw` is POINTER_MASK-stripped — the
                                // same operands the two calls received.
                                blk.call_void(
                                    "js_class_field_set_fallback",
                                    &[
                                        (I64, &site_id),
                                        (I64, &obj_bits),
                                        (I64, &key_raw),
                                        (DOUBLE, &val_double),
                                    ],
                                );
                                blk.br(&merge_label);
                                if requires_raw_f64 {
                                    let fallback = LoweredValue {
                                        semantic: SemanticKind::JsValue,
                                        rep: NativeRep::JsValue,
                                        llvm_ty: DOUBLE,
                                        value: val_double.clone(),
                                    };
                                    ctx.record_lowered_value_with_access_mode_and_facts(
                                        "ClassFieldSet",
                                        None,
                                        "js_object_set_field_by_name",
                                        &fallback,
                                        Some(BoundsState::Unknown),
                                        None,
                                        Some(BufferAccessMode::DynamicFallback),
                                        Some(MaterializationReason::RuntimeApi),
                                        None,
                                        None,
                                        Vec::new(),
                                        vec![
                                            raw_f64_layout_fact(
                                                None,
                                                "rejected",
                                                "class_field_set_guard",
                                                Some(MaterializationReason::RuntimeApi),
                                            ),
                                            raw_f64_layout_fact(
                                                None,
                                                "invalidated",
                                                "runtime_api",
                                                Some(MaterializationReason::RuntimeApi),
                                            ),
                                        ],
                                        false,
                                        false,
                                        vec![
                                            format!("class={}", class_name),
                                            format!("field={}", property),
                                            format!("field_index={}", field_idx_str),
                                        ],
                                    );
                                }

                                ctx.current_block = merge_idx;
                                Ok(val_double)
                            },
                        );
                    }
                }
            }
            // #9459 / #9495: the generic by-name tail is the receiver-aware
            // `[[Set]]` with the assignment's own `Throw` flag, in BOTH modes.
            //
            // `caller`/`arguments` are excluded in BOTH modes: those two names are
            // routed here from `PutValueSet` specifically to reach
            // `js_object_set_field_by_name`'s poisoned-accessor handling, which is
            // neither a `Throw`-flag nor a prototype-walk decision, and diverting
            // them would change behaviour neither issue is about.
            if !matches!(property.as_str(), "caller" | "arguments") {
                return lower_put_value_property_set_by_name(
                    ctx,
                    object,
                    property,
                    value,
                    assignment_strict,
                );
            }
            // #7154: the value expression can collect, and an evacuating minor
            // inside it relocates the receiver out from under `obj_box` --
            // `obj.k = f()` then writes `k` into abandoned from-space memory
            // and the field never appears on the object the program keeps.
            //
            // `across` rather than the plain form because the value is lowered
            // by `lower_value_for_dynamic_property_set` (an
            // `ExpectedNativeRep::JsValueBits` lowering plus a recorded
            // materialisation), which the operand list cannot produce.
            rooting::with_operands_rooted_across(
                ctx,
                &[object.as_ref()],
                &[value.as_ref()],
                |ctx| {
                    lower_value_for_dynamic_property_set(
                        ctx,
                        value,
                        "property_set.dynamic_value_bits",
                        "dynamic_property_set_helper_edge",
                    )
                },
                |ctx, vals, (val_double, _val_bits)| {
                    let obj_box = &vals[0];
                    // Intern the field name in the StringPool (same one the
                    // matching getter uses, so they share the global string).
                    let key_idx = ctx.strings.intern(property);
                    let key_handle_global =
                        format!("@{}", ctx.strings.entry(key_idx).handle_global);
                    let obj_bits = ctx.block().bitcast_double_to_i64(obj_box);
                    emit_nullish_write_guard(ctx, &obj_bits, property, "pset");
                    // Issue #618-followup: pass the FULL bits (including NaN-box
                    // tag) so the runtime can detect INT32-tagged class refs
                    // (`SQL.Aliased = Aliased` IIFE-static-property pattern from
                    // drizzle-orm). Pre-fix the AND-with-POINTER_MASK_I64 stripped
                    // the 0x7FFE tag, leaving the runtime with a small integer
                    // (the class id) — which fell into the small-handle dispatch
                    // path and silently dropped the assignment. The runtime now
                    // checks for top16 == 0x7FFE and routes to CLASS_DYNAMIC_PROPS.
                    let key_box = ctx.block().load(DOUBLE, &key_handle_global);
                    let key_bits = ctx.block().bitcast_double_to_i64(&key_box);
                    let key_raw = ctx.block().and(I64, &key_bits, POINTER_MASK_I64);
                    ctx.block().call_void(
                        "js_object_set_field_by_name",
                        &[(I64, &obj_bits), (I64, &key_raw), (DOUBLE, &val_double)],
                    );
                    Ok(val_double)
                },
            )
        }

        // `obj.field` — generic object field read. We get the key string
        // handle from the StringPool (interned, so the same key across
        // multiple sites shares one allocation), unbox both the object
        // pointer and the key handle, then call
        // `js_object_get_field_by_name_f64`. The result is a raw f64
        // (which IS the NaN-boxed value for non-number fields — same bit
        // pattern, runtime callers re-interpret based on context).
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
