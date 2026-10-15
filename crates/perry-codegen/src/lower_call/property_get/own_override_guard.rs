//! An own property beats a builtin, even on a receiver whose KIND is proven
//! (#10943).
//!
//! ECMA-262 resolves `recv.m(a)` as `Get(recv, "m")` then `Call`, so an own
//! `m` wins. perry lowers a method call to a DIRECT native call whenever it
//! can prove the receiver's kind — and an own property that shadows the method
//! leaves that proof entirely intact: `const m = new Map(); m.get = () => 1`
//! is still provably a Map. The result is a silent, PLAUSIBLE wrong value: a
//! zero-argument `Map.prototype.get` returns `undefined`, `Date.getTime` a
//! real timestamp, `Array.push` a length. Nothing throws.
//!
//! #10476 fixed this for UNPROVEN receivers, whose runtime kind already picks
//! between the builtin and the universal dispatcher. **A proof of KIND was
//! being read as a proof of NO OWN OVERRIDE.**
//!
//! # Why the guard is here and not in an arm
//!
//! `try_lower_property_get_method_call` is an ORDERED CHAIN, and the only arm
//! with a runtime diamond (`builtin_kind_guard`) is near its end — behind
//! `number_string`, the array arm and `map_set`. Every proven receiver is
//! claimed upstream and never reaches it, so extending that diamond emits a
//! guard nothing executes (measured: symbol present, call count 0). The guard
//! has to sit above the whole chain.
//!
//! # What is hoisted, and what is not
//!
//! Only the RECEIVER. The condition needs its value before the branch, so an
//! arm that re-lowered the receiver afterwards would evaluate an effectful one
//! (`make().get(k)`) twice. It is therefore materialised once above the branch
//! and re-read in both arms (`rooting::with_materialized_receiver`).
//!
//! ARGUMENTS are not hoisted. Each arm lowers its own, and a diamond runs
//! exactly one arm, so every argument is still evaluated exactly once at
//! runtime; the cost is two emitted copies, which is code size and not
//! semantics. That is what keeps this change from having to convert every arm
//! in the chain to operand form.
//!
//! The receiver EXPRESSION is passed down unchanged rather than rewritten to a
//! synthetic local, because every arm's proof is keyed on it — `is_array_expr`,
//! `receiver_class_name`, `is_date_receiver`, the `Ptr<Shape>` facts. A
//! synthetic local would erase those proofs and silently un-specialise every
//! one of these calls.
//!
//! # The guard only chooses a branch
//!
//! It never resolves the property and calls it: an own slot can hold a builtin
//! thunk that dispatches by name again, and an earlier attempt at this fix did
//! exactly that and overflowed the stack. The other side of the diamond is the
//! universal dispatcher, which already finds an own or inherited user method.

use anyhow::Result;
use perry_hir::Expr;

use super::helpers::is_date_receiver;
use crate::expr::{lower_expr, FnCtx};
use crate::rooting;
use crate::type_analysis::{is_array_expr, is_declared_map_expr, is_map_expr, is_set_expr};
use crate::types::{DOUBLE, I1, I32, I64, I8};

/// Method names a specialised lowering can claim on one of the exotic kinds
/// below. Over-approximating is safe — a name the chain declines simply makes
/// both arms of the diamond reach the same dispatcher — but staying close
/// keeps the emitted diamonds where the bug is.
fn shadowable_builtin_name(property: &str) -> bool {
    // Date: every name the chain can lower to a direct Date builtin, from the
    // table that lowering itself uses. The hand-kept list below stopped at
    // `getTime`/`toISOString` and a few setters, so a proven Date's
    // `getUTCHours`, `setTime` or `toUTCString` reached the direct call with no
    // guard at all (#11493).
    super::builtin_kind_guard::is_direct_date_builtin_name(property)
        || matches!(
            property,
            // Map / Set
            "get" | "set" | "has" | "delete" | "add" | "clear" | "entries" | "keys" | "values"
            | "forEach"
        // Array. `push` is ABSENT, and needs no diamond: a proven array's
        // push is folded by HIR into `Expr::ArrayPush`, whose slow arms honour
        // an own `push` themselves (`expr/array_push_own.rs`, #11021). A
        // diamond here would cost the inline store +94 instructions per call
        // for a receiver the inline tier's admission mask already excludes.
            | "pop" | "shift" | "unshift" | "slice" | "splice" | "indexOf"
            | "lastIndexOf" | "includes" | "join" | "concat" | "reverse" | "sort" | "fill"
            | "find" | "findIndex" | "filter" | "map" | "some" | "every" | "reduce"
            | "flat" | "flatMap" | "at"
        // The rest of the names the receiver-kind guard lowers directly on an
        // UNPROVEN array (`is_array_method_on_values`), which now tests an own
        // property first (#11493). A proven array has to test it too, or the
        // same call answers differently depending on what the compiler proved.
            | "toReversed" | "toSorted" | "toSpliced" | "reduceRight" | "copyWithin"
        // Date names with no direct builtin in the table above
            | "toJSON" | "valueOf"
        // Number
            | "toFixed" | "toPrecision" | "toExponential"
        )
}

/// Is the receiver's KIND proven to be one whose builtins are lowered
/// directly? Those are exactly the receivers #10476's diamond never sees.
///
/// A string receiver is deliberately absent: a primitive carries no own
/// properties, so its builtin cannot be shadowed on the receiver itself.
fn kind_is_proven_exotic(ctx: &FnCtx<'_>, object: &Expr) -> bool {
    is_map_expr(ctx, object)
        || is_set_expr(ctx, object)
        || is_array_expr(ctx, object)
        || is_date_receiver(ctx, object)
        // A DECLARED collection is specialised too, and by a condition that is
        // the negation of the proven one (`!is_native_map && is_declared_map_expr`
        // in `map_set.rs`), so asking only about proven kinds missed it
        // entirely: `class Holder { m = new Map() }` read through a captured
        // `h.m` emitted NO guard and ran `js_declared_map_get`. That helper
        // brand-checks the receiver and then calls the native method, which an
        // own property must still beat.
        //
        // `ReadonlySet` is deliberately NOT here. `js_readonly_set_has`
        // brand-checks and otherwise preserves JavaScript dispatch, which
        // already reaches an own method — so a diamond would only add the
        // generic tower to the common native case, which
        // `readonly_collection_tests` exists to forbid.
        || is_declared_map_expr(ctx, object)
}

/// Does this receiver need materialising, or is re-lowering it already both
/// free and a fresh read of a rooted location?
///
/// A `LocalGet`/`This` receiver lives in a slot the collector rewrites, and
/// lowering it again emits a LOAD from that slot — which is exactly the
/// re-read the rooting invariant demands after an allocating operand ("a root
/// buys a rewritten LOCATION; the consuming call only observes the rewrite if
/// it reads that location again", #7114/#9523). Materialising it into a second
/// rooted slot would be safe but pointless: it adds a hop, and it moves the
/// re-read off the slot the receiver actually lives in.
///
/// Everything else — a call result, a property or element read — cannot be
/// evaluated twice, so it is materialised once and every consumer re-reads
/// THAT slot.
fn receiver_needs_materializing(object: &Expr) -> bool {
    !matches!(object, Expr::LocalGet(_) | Expr::This)
}

/// Does this call need the own-override diamond?
pub(super) fn guards(ctx: &FnCtx<'_>, object: &Expr, property: &str) -> bool {
    shadowable_builtin_name(property) && kind_is_proven_exotic(ctx, object)
}

/// Branch to `own_label` when `recv` may own a property named `property` that
/// shadows the builtin, else to `builtin_label`.
///
/// The runtime answers in one relaxed load for the overwhelmingly common
/// receiver (`object/own_override.rs`), and never answers "no" for anything it
/// cannot prove: a wrong "no" is a silent wrong value, a wrong "yes" is only
/// slower.
/// An array owns a named property only in its named layout object
/// (`ArrayHeader.named`, §6a): a null `named` is the absence proof. Plus
/// `GC_ARRAY_CUSTOM_PROTO` (0x40 in `GcHeader::_reserved`, #11915): an array
/// with its own `[[Prototype]]` does not read `%Array.prototype%`, and the
/// predicate answers for it; and `GC_FLAG_FORWARDED` (0x80 in `gc_flags`): a
/// pre-growth alias's `named` is not the live head's. As one i32 at -8
/// (`obj_type`, `gc_flags`, `_reserved`): `0x80 << 8 | 0x40 << 16`.
const ARRAY_NOT_DEFAULT_MASK_I32: &str = "4227072";
/// `ArrayHeader.named`'s byte offset.
const ARRAY_NAMED_OFFSET: &str = "8";
/// `HANDLE_MASK_48` — the address bits of a NaN-boxed pointer.
const HANDLE_MASK_48: &str = "281474976710655";

/// The guard's receiver test: `may this receiver own a named property of this
/// name?`, answered INLINE for the case that is almost always the answer.
///
/// Calling `js_receiver_may_own_named_method` to learn "no" cost, measured
/// against upstream/main with five interleaved rounds: **+38.000 instructions
/// on every proven-Map builtin call**, and, before the array tier had its
/// absence proof, +4,745 on every array one. The predicate's first act is one
/// of two cheap proofs, so the guard performs that proof itself and calls only
/// when it fails:
///
/// * a proven ARRAY answers from its own `GcHeader::_reserved` — the bit the
///   predicate tests first;
/// * every other proven kind answers from `PERRY_OWN_NAMED_PROP_INSTALLED`,
///   which is the predicate's own early return (`nothing anywhere has ever put
///   a named property on a non-ordinary cell`) read with one monotonic load.
///
/// This is a LIFT, not a new proof: both tests are the predicate's own first
/// lines, so nothing is proven here that the runtime did not already prove,
/// and no new install site has to be armed for it to be sound.
///
/// `Pristine` (#11915) costs the non-array guard nothing: every modification
/// of a builtin prototype arms `PERRY_OWN_NAMED_PROP_INSTALLED`, and the
/// predicate answers "may own" for a receiver whose prototype is not pristine
/// (`builtin_proto::receiver_pristine`), so a modified prototype takes the
/// same own arm — the generic path, which performs the real `Get` and call.
/// An ARRAY answers from its own header, so `pristine` (`Some(Array)` for a
/// site that proves it here) adds the one byte test to the branch the guard
/// already has; its `GC_ARRAY_CUSTOM_PROTO` bit joins the header mask, and
/// the predicate answers for it too.
pub(crate) fn emit_own_override_branch(
    ctx: &mut FnCtx<'_>,
    property: &str,
    recv: &str,
    receiver_is_array: bool,
    pristine: Option<crate::builtin_proto::BuiltinProto>,
    own_label: &str,
    builtin_label: &str,
) {
    let ask_idx = ctx.new_block("ownoverride.ask");
    let ask_label = ctx.block_label(ask_idx);
    {
        if receiver_is_array {
            if let Some(family) = pristine {
                // One compare and an unlikely branch to the ask arm, whose
                // predicate answers for a modified prototype.
                let patched = crate::builtin_proto::emit_patched_test(ctx, family);
                let header_idx = ctx.new_block("ownoverride.header");
                let header_label = ctx.block_label(header_idx);
                ctx.block().cond_br(&patched, &ask_label, &header_label);
                ctx.current_block = header_idx;
            }
            let blk = ctx.block();
            let bits = blk.bitcast_double_to_i64(recv);
            let handle = blk.and(I64, &bits, HANDLE_MASK_48);
            let obj_ptr = blk.inttoptr(I64, &handle);
            // GcHeader precedes the array: obj_type/gc_flags/_reserved @-8.
            let hdr_ptr = blk.gep(I8, &obj_ptr, &[(I64, "-8")]);
            let hdr = blk.load(I32, &hdr_ptr);
            let odd = blk.and(I32, &hdr, ARRAY_NOT_DEFAULT_MASK_I32);
            let not_default = blk.icmp_ne(I32, &odd, "0");
            let named_ptr = blk.gep(I8, &obj_ptr, &[(I64, ARRAY_NAMED_OFFSET)]);
            let named = blk.load(I64, &named_ptr);
            let has_named = blk.icmp_ne(I64, &named, "0");
            let maybe_owns = blk.or(I1, &not_default, &has_named);
            blk.cond_br(&maybe_owns, &ask_label, builtin_label);
        } else {
            let blk = ctx.block();
            let installed = blk.load_atomic_monotonic(I32, "@PERRY_OWN_NAMED_PROP_INSTALLED", 4);
            let any_installed = blk.icmp_ne(I32, &installed, "0");
            blk.cond_br(&any_installed, &ask_label, builtin_label);
        }
    }

    ctx.current_block = ask_idx;
    // The predicate takes the interned key value (not a dispatch id). The
    // string-pool handle is already a rooted NaN-boxed string; passing it
    // avoids rebuilding the same property key from raw bytes in the runtime.
    let idx = ctx.strings.intern(property);
    let key_handle = format!("@{}", ctx.strings.entry(idx).handle_global);
    let blk = ctx.block();
    let key = blk.load(DOUBLE, &key_handle);
    let maybe = blk.call(
        I32,
        "js_receiver_may_own_named_method",
        &[(DOUBLE, recv), (DOUBLE, &key)],
    );
    let may_own = blk.icmp_ne(I32, &maybe, "0");
    blk.cond_br(&may_own, own_label, builtin_label);
}

/// The `builtin_proto` family of a receiver whose kind
/// [`kind_is_proven_exotic`] proved.
pub(crate) fn proven_kind_family(
    ctx: &FnCtx<'_>,
    object: &Expr,
    receiver_is_array: bool,
) -> crate::builtin_proto::BuiltinProto {
    use crate::builtin_proto::BuiltinProto;
    if receiver_is_array || is_array_expr(ctx, object) {
        BuiltinProto::Array
    } else if is_map_expr(ctx, object) || is_declared_map_expr(ctx, object) {
        BuiltinProto::Map
    } else if is_set_expr(ctx, object) {
        BuiltinProto::Set
    } else if is_date_receiver(ctx, object) {
        BuiltinProto::Date
    } else {
        // Unreachable for a proven exotic kind; `Object.prototype` withdraws
        // every family, so its byte is the conservative answer.
        BuiltinProto::Object
    }
}

/// The universal dispatcher, which finds an own or inherited user method. It
/// is the same call `builtin_kind_guard`'s generic block already makes.
fn emit_dispatcher(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    args: &[Expr],
    call_byte_offset: u32,
) -> Result<String> {
    let mut operands: Vec<&Expr> = Vec::with_capacity(args.len() + 1);
    operands.push(object);
    operands.extend(args.iter());
    // #11915: the shared generic path (see `builtin_proto::emit_generic_call`).
    rooting::with_operands_rooted(ctx, &operands, |ctx, values| {
        let (recv, arg_vals) = values.split_first().expect("the receiver is operand 0");
        crate::expr::calls::emit_call_location_at(ctx, call_byte_offset);
        Ok(crate::builtin_proto::emit_generic_call(
            ctx,
            crate::builtin_proto::GenericRead::Receiver,
            recv,
            property,
            arg_vals,
        ))
    })
}

/// Lower `object.property(args…)` under one own-override diamond.
pub(super) fn lower(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    object: &Expr,
    property: &str,
    args: &[Expr],
    call_byte_offset: u32,
) -> Result<String> {
    let receiver = lower_expr(ctx, object)?;
    let key = object as *const Expr as usize;
    // Which cheap proof the guard can perform inline. A proven ARRAY answers
    // from its own header bit; every other proven kind answers from the global
    // install flag. The two are not interchangeable -- arrays do not arm the
    // global one -- so this asks the same predicate `kind_is_proven_exotic`
    // used to admit the receiver in the first place.
    let receiver_is_array = is_array_expr(ctx, object);
    let emit = |ctx: &mut FnCtx<'_>| -> Result<String> {
        let own_idx = ctx.new_block("ownoverride.own");
        let builtin_idx = ctx.new_block("ownoverride.builtin");
        let merge_idx = ctx.new_block("ownoverride.merge");
        let own_label = ctx.block_label(own_idx);
        let builtin_label = ctx.block_label(builtin_idx);
        let merge_label = ctx.block_label(merge_idx);

        // Materialised or not, the receiver is READ HERE: from its slot when it
        // was materialised, and by re-lowering when it lives in one already
        // (a local). Expecting a materialisation that the gate deliberately
        // skipped is what made `array_pop` and `entry_block_alloca` panic.
        let recv = match rooting::materialized_receiver_reread(ctx, key) {
            Some(value) => value,
            None => lower_expr(ctx, object)?,
        };
        emit_own_override_branch(
            ctx,
            property,
            &recv,
            receiver_is_array,
            Some(proven_kind_family(ctx, object, receiver_is_array)),
            &own_label,
            &builtin_label,
        );

        // The own arm: the universal dispatcher. Its operands are lowered
        // HERE, inside the arm, so they are evaluated once by whichever arm
        // runs — and the receiver operand is served by the materialisation.
        ctx.current_block = own_idx;
        let own_value = emit_dispatcher(ctx, object, property, args, call_byte_offset)?;
        let own_end = ctx.block().label.clone();
        ctx.block().br(&merge_label);

        // The builtin arm: the ordinary chain, unchanged. A chain that
        // declines (an over-approximated name) reaches the same dispatcher, so
        // the diamond is redundant rather than wrong.
        ctx.current_block = builtin_idx;
        let family = proven_kind_family(ctx, object, receiver_is_array);
        crate::builtin_proto::emit_contract_check(ctx.block(), &[family]);
        let builtin_value = match super::lower_method_call_chain(
            ctx,
            callee,
            object,
            property,
            args,
            call_byte_offset,
        )? {
            Some(value) => value,
            None => emit_dispatcher(ctx, object, property, args, call_byte_offset)?,
        };
        let builtin_end = ctx.block().label.clone();
        ctx.block().br(&merge_label);

        ctx.current_block = merge_idx;
        Ok(ctx.block().phi(
            DOUBLE,
            &[
                (own_value.as_str(), own_end.as_str()),
                (builtin_value.as_str(), builtin_end.as_str()),
            ],
        ))
    };
    if receiver_needs_materializing(object) {
        rooting::with_materialized_receiver(ctx, key, &receiver, emit)
    } else {
        emit(ctx)
    }
}
