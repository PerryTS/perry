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
    BoundsState, BufferAccessMode, ExpectedNativeRep, LoweredValue, NativeRep, SemanticKind,
};
use crate::rooting;
use crate::type_analysis::{
    expr_may_return_boxed_value_from_raw_f64_fallback, is_numeric_expr, receiver_class_name,
};
use crate::types::{DOUBLE, I1, I32, I64, PTR};

use super::{
    class_field_store_needs_string_addref, emit_jsvalue_slot_store_pointer_tested,
    emit_typed_feedback_register_site, expr_produces_non_pointer_bits_by_construction, lower_expr,
    lower_expr_native, raw_f64_layout_fact, try_lower_pod_field_set,
    typed_feedback_emission_enabled, unbox_to_i64, FnCtx, TypedFeedbackContract, TypedFeedbackKind,
};

/// Metadata-only class candidate for the runtime-guarded plain-field store.
/// Accessor calls still require a real receiver proof; a lying annotation is
/// routed to the by-name setter path instead.
fn guarded_declared_class_store_candidate(ctx: &FnCtx<'_>, object: &Expr) -> Option<String> {
    let name = match object {
        Expr::LocalGet(id) => match ctx.local_type_hint(id)? {
            perry_hir::types::Type::Named(name) => name,
            _ => return None,
        },
        // #10906: `this` in a closed-shape object-literal method.
        Expr::This => ctx.guarded_this_class.as_ref()?,
        _ => return None,
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

/// #10907: store a numeric value into a raw-f64 class-field slot without
/// paying the out-of-line `js_array_numeric_value_to_raw_f64` call on the
/// common path.
///
/// Used where the stored value is NOT already known to be a plain finite
/// double: the scalar-replaced field stores. Canonicalizing unconditionally made every `o.f = <number>` into a
/// `: number` field ~31 instructions dearer than the same store into a `: any`
/// field. The call is the identity on any finite double and on any value
/// `expr_produces_canonical_raw_f64` admits, so it is skipped statically for
/// the latter and, otherwise, kept only on the arm an inline plain-finite test
/// of the bits cannot rule out.
///
/// Each arm does its own store instead of merging through a `phi`: a phi
/// operand is never re-materialised by `root_reload`, while an ordinary store
/// operand is, so this keeps the value's uses in the shape the rooting passes
/// and `gc_root_dominance_check.py` already understand. Leaves
/// `ctx.current_block` on a block with no terminator.
fn emit_raw_f64_class_field_slot_store(
    ctx: &mut FnCtx<'_>,
    value: &Expr,
    value_double: &str,
    slot_ptr: &str,
) {
    if crate::type_analysis::expr_produces_canonical_raw_f64(ctx, value) {
        // GC_STORE_AUDIT(POINTER_FREE): canonical raw f64 by construction —
        // never a NaN-boxed heap pointer.
        ctx.block().store(DOUBLE, value_double, slot_ptr);
        return;
    }
    let plain_idx = ctx.new_block("class_field_set.raw_plain");
    let canon_idx = ctx.new_block("class_field_set.raw_canonicalize");
    let join_idx = ctx.new_block("class_field_set.raw_join");
    let plain_label = ctx.block_label(plain_idx);
    let canon_label = ctx.block_label(canon_idx);
    let join_label = ctx.block_label(join_idx);
    {
        let blk = ctx.block();
        let value_bits = blk.bitcast_double_to_i64(value_double);
        let finite =
            crate::expr::class_field_inline_guard::emit_plain_finite_number_check(blk, &value_bits);
        blk.cond_br(&finite, &plain_label, &canon_label);
    }
    ctx.current_block = plain_idx;
    {
        let blk = ctx.block();
        // GC_STORE_AUDIT(POINTER_FREE): the plain-finite test proved a genuine
        // unboxed double (every NaN-box tag has the all-ones exponent).
        blk.store(DOUBLE, value_double, slot_ptr);
        blk.br(&join_label);
    }
    ctx.current_block = canon_idx;
    {
        let blk = ctx.block();
        let canonical = canonicalize_raw_f64_numeric_store_value(blk, value_double);
        // GC_STORE_AUDIT(POINTER_FREE): the canonicalizer returns a raw number
        // (NaN for anything non-numeric), never a heap pointer.
        blk.store(DOUBLE, &canonical, slot_ptr);
        blk.br(&join_label);
    }
    ctx.current_block = join_idx;
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
        let rooted_operands: [&perry_hir::Expr; 2] = [object, value];
        let (rooted_values, rooted_group) =
            crate::lower_call::lower_operand_list_rooted(ctx, &rooted_operands)?;
        let recv_box = rooted_values[0].clone();
        let val_double = rooted_values[1].clone();
        let rooted_result = body(ctx, recv_box, val_double);
        rooted_group.release(ctx);
        return rooted_result;
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
    super::store_census::bump(ctx, super::store_census::BY_NAME_RUNTIME);
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
pub(crate) fn lower_put_value_property_set_by_name(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    value: &Expr,
    assignment_strict: bool,
) -> Result<String> {
    lower_static_key_store(ctx, object, property, value, assignment_strict)
}

fn lower_static_key_store(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    value: &Expr,
    assignment_strict: bool,
) -> Result<String> {
    super::store_census::bump(ctx, super::store_census::BY_NAME_PUT_VALUE);
    let value_is_scalar = expr_produces_non_pointer_bits_by_construction(ctx, value)
        || super::i32_fast_path::is_known_i32_range(ctx, value);
    // Object model step 2c: a constructor key's store in a run of the
    // body's family stores (`layout_read::family_run_plan`).
    let mut plan = if matches!(object, Expr::This) {
        super::layout_read::family_run_plan(ctx, property, value)
    } else {
        None
    };
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
        |ctx, vals, (val_double, val_bits)| {
            // `vals[0]` is the receiver RE-READ from its operand root after the
            // value's lowering, which is arbitrary user code: a collection that
            // moved it, or a shape change it made, is already visible here.
            let obj_box = vals[0].clone();
            let run = plan.take().and_then(|plan| {
                let bits = ctx.block().bitcast_double_to_i64(&obj_box);
                super::layout_read::family_run_arm(ctx, plan, &bits, property)
            });
            let key_idx = ctx.strings.intern(property);
            let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
            let obj_bits = ctx.block().bitcast_double_to_i64(&obj_box);
            emit_typed_feedback_property_set_observation(ctx, property, &obj_bits, |ctx| {
                // The key is an interned heap string, so its `StringHeader*` is
                // a mask, never an allocation.
                let key_box = ctx.block().load(DOUBLE, &key_handle_global);
                let key_bits = ctx.block().bitcast_double_to_i64(&key_box);
                ctx.block().and(I64, &key_bits, POINTER_MASK_I64)
            });
            // The store probe, else the complete `[[Set]]`. The assignment's
            // value: the RHS on a probe store, and otherwise the runtime's
            // re-read of it from its own root (a collection point).
            let result = super::put_value_store_ic::emit_static_store_ic(
                ctx,
                &obj_box,
                property,
                &val_double,
                &val_bits,
                // `undefined.x = 1` is a TypeError in BOTH modes. A nullish
                // base fails the hit path's receiver test, and the miss entry's
                // `[[Set]]` throws it (`js_put_value_set`: node's wording, and
                // before `Throw` is consulted), so no guard is emitted here.
                assignment_strict,
                value_is_scalar,
                matches!(object, Expr::This),
                run.as_ref(),
            );
            Ok(result)
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
    let _fallback = crate::fallback_diag::enter_expr(ctx, expr);
    match expr {
        Expr::PropertySet {
            object,
            property,
            value,
        } => {
            // A receiver the wpfacts lowering proves takes the elided read
            // and store below instead of the compound site.
            // #11791: an instance private field write is a shape compare and
            // a slot store.
            if let Some(site) =
                super::private_field_site::private_field_site(ctx, object, property, 1)
            {
                return super::private_field_site::lower_set(ctx, site, property, value);
            }
            if let Some((function, keys, contained)) =
                super::named_accessor::candidate(ctx, object, property, true)
            {
                let value_i32 = (super::is_known_i32_range(ctx, value)
                    || matches!(value.as_ref(), Expr::LocalGet(id) if ctx.i32_counter_slots.contains_key(id)))
                    && crate::type_analysis::expr_produces_canonical_raw_f64(ctx, value);
                return with_class_store_operands(ctx, object, value, |ctx, receiver, rhs| {
                    let value_i32 = value_i32 || ctx.block().widened_i32_source(&rhs).is_some();
                    crate::rooting::with_rooted_group(ctx, 1, |ctx, group| {
                        let saved =
                            group.adopt_emitted(ctx, crate::rooting::Repr::Boxed, &rhs, true);
                        super::named_accessor::emit(
                            ctx,
                            &receiver,
                            property,
                            &function,
                            &keys,
                            Some(&rhs),
                            assignment_strict,
                            contained,
                            value_i32,
                        )?;
                        Ok(group.reread_emitted(ctx, saved))
                    })
                });
            }
            // Object model step 1: adding an `Object.create(P)` family's key.
            if let Some(add) = crate::layout_desc::create_families::add_site(ctx, object, property)
            {
                return crate::layout_desc::create_families::lower_add(
                    ctx,
                    &add,
                    object,
                    value,
                    assignment_strict,
                );
            }
            // Object model step 1: adding a literal family's key is a store.
            if let Some(site) = super::family_slot::site(ctx, object, property, true) {
                return super::family_slot::lower_set(
                    ctx,
                    &site,
                    object,
                    property,
                    value,
                    assignment_strict,
                );
            }
            // Step 4b: a planned-bare store inside a region's F-body.
            if let Some(v) =
                crate::stmt::region_loop::try_lower_bare_put(ctx, expr, object, property, value)?
            {
                return Ok(v);
            }
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
                    if numeric_store {
                        emit_raw_f64_class_field_slot_store(ctx, value, &val_double, &slot);
                    } else {
                        ctx.block().store(DOUBLE, &val_double, &slot);
                    }
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
                    if !numeric_store && matches!(&**value, Expr::LocalGet(_)) {
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
                        let lowered_f64 = LoweredValue::f64(val_double.clone());
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
                        if numeric_store {
                            emit_raw_f64_class_field_slot_store(ctx, value, &val_double, &slot);
                        } else {
                            ctx.block().store(DOUBLE, &val_double, &slot);
                        }
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
                            let lowered_f64 = LoweredValue::f64(val_double.clone());
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
            // wpfacts slice 2: a receiver the whole-program facts prove
            // (prefix-stable classes, a definite data birth field at one
            // slot, non-null) stores into the slot with no guard.
            if let Some(w) = super::wpfacts_access::prove_store(
                ctx,
                object,
                property,
                super::wpfacts_access::Access::Set,
            ) {
                let class_name = w.proof.class_names[0].clone();
                let slot = w.proof.slot;
                let raw = w.proof.any_f64_lane;
                return emit_proven_class_field_store(
                    ctx,
                    object,
                    property,
                    value,
                    &class_name,
                    slot,
                    raw,
                    Some(&w),
                    assignment_strict,
                );
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
                                super::store_census::bump(ctx, super::store_census::CFIELD_SETTER);
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
                // #12483: a class-field store is the static-key store site
                // (`lower_put_value_property_set_by_name`): the receiver test,
                // the site's word against the receiver's ShapeId, the store,
                // the barrier. Only a receiver whose shape is statically
                // PROVEN (`Ptr<Shape>`) stores without that compare. The class
                // guard this replaces compared against the class's BIRTH
                // ShapeId, so an instance that left it (constructor-store
                // reservations, a `: any` field whose lane generalized on its
                // first store, constructor-added keys, a subclass the arms
                // could not name) missed it on every store and paid the
                // site-less by-name `js_put_value_set` behind it.
                //
                // A proven store stays strict-only, as it was: its cold arm
                // (`js_class_field_set_fallback`) rejects by throwing.
                if assignment_strict {
                    if let Some(field_index) =
                        crate::type_analysis::class_field_global_index(ctx, &class_name, property)
                    {
                        if let (true, Some(keys_global_name)) = (
                            ctx.class_ids.contains_key(&class_name),
                            ctx.class_keys_globals.get(&class_name).cloned(),
                        ) {
                            let requires_raw_f64 =
                                crate::expr::class_field_inline_guard::class_field_site_raw_f64(
                                    ctx,
                                    &class_name,
                                    property,
                                    field_index,
                                );
                            // A contained offset proof may carry completed
                            // ConstFn facts. Writing that boxed slot must
                            // deprecate/restamp through the checked funnel.
                            let ptr_shape_proven = ctx
                                .ptr_shape_store_fact(object.as_ref())
                                .is_some_and(|fact| fact.class_name == class_name)
                                && !(crate::expr::class_field_inline_guard::class_instances_carry_private_elements(
                                    ctx,
                                    &class_name,
                                ) && crate::expr::class_field_inline_guard::class_instances_grow_past_layout(
                                    ctx,
                                    &class_name,
                                ))
                                && (requires_raw_f64
                                    || !crate::codegen::slot_may_be_constfn(
                                        &keys_global_name,
                                        field_index,
                                    ));
                            if ptr_shape_proven {
                                return emit_proven_class_field_store(
                                    ctx,
                                    object,
                                    property,
                                    value,
                                    &class_name,
                                    field_index,
                                    requires_raw_f64,
                                    None,
                                    assignment_strict,
                                );
                            }
                        }
                    }
                }
                // A declared field of a class whose instances carry their keys
                // from birth: an own-data overwrite through the store site.
                if crate::type_analysis::class_field_global_index(ctx, &class_name, property)
                    .is_some()
                    && crate::expr::class_field_inline_guard::class_instances_keep_birth_shape(
                        ctx,
                        &class_name,
                    )
                {
                    return lower_static_key_store(ctx, object, property, value, assignment_strict);
                }
            }
            // #9459 / #9495 / #9525: the generic by-name tail is the
            // receiver-aware `[[Set]]` with the assignment's own `Throw` flag,
            // in BOTH modes. This includes `caller` / `arguments`: their
            // inherited poisoned accessor rejects a plain non-strict
            // function store with `false`, and only PutValue can decide from
            // `Throw` whether that rejection stays silent or becomes a
            // TypeError. The by-name setter below has no `Throw` parameter and
            // therefore remains only for strict-only specialised paths.
            return lower_put_value_property_set_by_name(
                ctx,
                object,
                property,
                value,
                assignment_strict,
            );
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

/// The guard-free class-field store of a receiver whose layout is proven:
/// a `Ptr<Shape>` local (`wpf == None`) or a wpfacts zero-guard binding
/// (`wpf`, whose kinds also decide the barrier and the string addref, and
/// whose contract witness runs after the store in a contract build).
#[allow(clippy::too_many_arguments)]
fn emit_proven_class_field_store(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    value: &Expr,
    class_name: &str,
    field_index: u32,
    requires_raw_f64: bool,
    wpf: Option<&super::wpfacts_access::StoreProof>,
    strict: bool,
) -> Result<String> {
    with_class_store_operands(ctx, object, value, |ctx, recv_box, val_double| {
        let key_idx = ctx.strings.intern(property);
        let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
        // CLS D3: the facts prove the receiver an object of the class set or
        // nullish. The one test that stays sends nullish to the complete
        // `[[Set]]`, which throws (after the value was evaluated, as the
        // specification orders it); a passing receiver stores into the slot.
        if wpf.is_some_and(|w| w.proof.nullable) {
            let obj_bits = ctx.block().bitcast_double_to_i64(&recv_box);
            let recv = super::receiver_range::emit_fused_receiver_test(ctx.block(), &obj_bits);
            let hit_idx = ctx.new_block("wpf.set.obj");
            let miss_idx = ctx.new_block("wpf.set.nullish");
            let hit_label = ctx.block_label(hit_idx);
            let miss_label = ctx.block_label(miss_idx);
            ctx.block()
                .cond_br(&recv.is_object_pointer, &hit_label, &miss_label);
            ctx.current_block = miss_idx;
            let blk = ctx.block();
            let key_box = blk.load(DOUBLE, &key_handle_global);
            let _ = blk.call(
                DOUBLE,
                "js_put_value_set",
                &[
                    (DOUBLE, &recv_box),
                    (DOUBLE, &key_box),
                    (DOUBLE, &val_double),
                    (DOUBLE, &recv_box),
                    (I32, if strict { "1" } else { "0" }),
                ],
            );
            blk.unreachable();
            ctx.current_block = hit_idx;
            super::wpfacts_access::note_tested(ctx, object);
        }
        let site_id = emit_typed_feedback_register_site(
            ctx,
            TypedFeedbackKind::PropertySet,
            property,
            TypedFeedbackContract::class_field_set(),
        );
        let field_idx_str = field_index.to_string();
        // Representation-selection Phase 3b: shape-proven
        // Ptr<Shape> receiver (collectors/ptr_shape.rs) — no
        // guard, no shape diamond. Raw-f64 slots keep the
        // inline plain-finite value check with a cold
        // `js_class_field_set_fallback` arm (a NaN/Inf/boxed
        // value must never be stored raw into a scalar-masked
        // slot — the runtime setter performs the layout
        // downgrade the GC scan relies on). Boxed slots store
        // inline with the existing generational write barrier
        // for possibly-pointer values. Phase 5a routes `this`
        // here too: the freeze-family module-wide kill
        // (collectors/proven_this.rs) is what makes a
        // guard-free STORE through a proven `this` sound.
        if wpf.is_none() {
            ctx.note_ptr_shape_consumed(object, "ptr_shape_set");
        }
        super::store_census::bump(ctx, super::store_census::CFIELD_SHAPE_PROVEN);
        let header_skip =
            crate::target_layout::object_header_size_bytes(ctx.target_triple).to_string();
        // wpfacts F2: a field that never holds a pointer-bearing kind needs
        // no barrier (the contract build checks every stored kind).
        let field_set_barrier_needed = !expr_produces_non_pointer_bits_by_construction(ctx, value)
            && wpf.is_none_or(|w| w.proof.may_hold_pointer());
        // Case 2 (§3.7): the proven classes disagree on the slot, so the
        // receiver's class id selects it.
        let case2 = wpf.filter(|w| !w.proof.agrees());
        let (obj_bits, obj_handle, field_ptr, val_bits) = {
            let case2_ptr =
                case2.map(|w| super::wpfacts_access::emit_proof_slot_ptr(ctx, &recv_box, &w.proof));
            let blk = ctx.block();
            let obj_bits = blk.bitcast_double_to_i64(&recv_box);
            let obj_handle = blk.and(I64, &obj_bits, POINTER_MASK_I64);
            let field_ptr = match case2_ptr {
                Some(p) => p,
                None => {
                    // Biased addressing (`wpfacts_access::emit_slot_ptr`).
                    let biased = blk.sub(
                        I64,
                        &obj_bits,
                        &(super::receiver_range::RECEIVER_BIAS as i64).to_string(),
                    );
                    super::receiver_range::emit_field_ptr(
                        blk,
                        &biased,
                        header_skip.parse::<i64>().unwrap_or(0) + i64::from(field_index) * 8,
                    )
                }
            };
            let val_bits = blk.bitcast_double_to_i64(&val_double);
            (obj_bits, obj_handle, field_ptr, val_bits)
        };
        if requires_raw_f64 {
            // A bounded canonical numeric producer cannot downgrade
            // an F64 lane. Bitwise results with a Number operand
            // are canonical finite doubles on both inline/helper arms.
            let plain = (super::is_known_i32_range(ctx, value)
                || matches!(value, Expr::LocalGet(id) if ctx.i32_counter_slots.contains_key(id)))
                && (crate::type_analysis::expr_produces_canonical_raw_f64(ctx, value)
                    || matches!(
                        value,
                        Expr::Binary {
                            op: perry_hir::BinaryOp::BitAnd
                                | perry_hir::BinaryOp::BitOr
                                | perry_hir::BinaryOp::BitXor
                                | perry_hir::BinaryOp::Shl
                                | perry_hir::BinaryOp::Shr,
                            ..
                        }
                    ));
            let plain = plain || ctx.block().widened_i32_source(&val_double).is_some();
            let store_idx = ctx.new_block("ptr_shape_set.raw_store");
            let cold_idx = ctx.new_block("ptr_shape_set.downgrade");
            let merge_idx = ctx.new_block("ptr_shape_set.merge");
            let store_label = ctx.block_label(store_idx);
            let cold_label = ctx.block_label(cold_idx);
            let merge_label = ctx.block_label(merge_idx);
            {
                let blk = ctx.block();
                if plain {
                    blk.br(&store_label);
                } else {
                    let finite =
                        crate::expr::class_field_inline_guard::emit_plain_finite_number_check(
                            blk, &val_bits,
                        );
                    blk.cond_br(&finite, &store_label, &cold_label);
                }
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
            let string_addref_needed = class_field_store_needs_string_addref(ctx, value)
                && wpf.is_none_or(|w| w.proof.may_hold_string());
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
                string_addref_needed,
                &obj_bits,
                &field_addr,
                field_set_barrier_needed,
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
            if wpf.is_some() {
                "class_field_set.wpfacts_t0"
            } else {
                "class_field_set.shape_proven_store"
            },
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
                match wpf {
                    Some(w) => format!("receiver_proof=wpfacts:{}", w.proof.binding),
                    None => "receiver_proof=ptr_shape_local".to_string(),
                },
                format!("field_layout_raw_f64={}", requires_raw_f64),
            ],
        );
        if let Some(w) = wpf {
            super::wpfacts_access::emit_witness(
                ctx,
                &recv_box,
                &val_double,
                property,
                &w.proof,
                super::wpfacts_access::WPF_STORE,
                w.site,
            );
        }
        Ok(val_double)
    })
}

#[cfg(test)]
#[path = "collecting_root_tests.rs"]
mod collecting_root_tests;
