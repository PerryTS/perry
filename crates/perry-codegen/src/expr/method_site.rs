//! The method-call site: `recv.m(args)` as a [[Get]] of `m` and a call with
//! `recv` as `this` (object model step 5, OBJECT_MODEL_2026-10-13.md §3.6,
//! §3.7). The site holds no state: no memo, no learned word, nothing primed.
//!
//! ```text
//!   t   = bits - RECEIVER_BIAS ; t <u SPAN                 else SLOW
//!   [static ConstFn lanes: [recv] == lane.word -> call @B(slot, recv, args)]
//!   v   = perry_probe_get(dir, t, key, hash)      (own, then the chain)
//!   v is a heap pointer, [v-8] & 0x80FF == CLOSURE,
//!   [v].capture_count has no `this` capture                 else SLOW
//!   [v.info.code == @B for a compile-time candidate body B -> call @B]
//!   CALL: r = js_closure_call{N}(v, recv, args...)
//!   SLOW: js_method_call_slow(feedback_site, recv, key, method_id, args)
//!         (the universal by-name dispatcher)
//! ```
//!
//! The probe answers only data properties, so the slow path's own [[Get]]
//! never repeats an observable read. A closure that reads `this` from a
//! capture (an object-literal method, #6475) is rebound by the dispatcher.
//!
//! #11910 (the split site): when the arguments can observe WHEN the method is
//! read ([`args_may_observe_lookup`]), ECMA-262 13.3.6.1 requires the read
//! before them, so the site splits into one runtime call on each side:
//!
//! ```text
//!   LOOKUP (before the args):
//!     v = js_method_lookup(dir, recv, key, hash, method_id)
//!         (the probe's function value, else the one [[Get]] that can run a
//!          getter/trap, or a by-name answer)
//!     v rooted across the arguments
//!   CALL (after the args):
//!     v a plain closure: the candidate bodies, else js_closure_call{N}(v, recv, args...)
//!     else: r = js_method_call_split(v, site, recv, method_id, args)
//!           (BY_NAME / BY_NAME_DIRECT -> by-id dispatch, else call v)
//! ```

use super::FnCtx;
use crate::types::{DOUBLE, I1, I16, I32, I64, PTR};

const EMPTY_SHAPE_DIR: &str = "@PERRY_EMPTY_SHAPE_DIR";

/// Is the One Path method site available for this call?
/// `PERRY_METHOD_SITE=0` at compile time keeps the old dispatcher (A/B).
pub(crate) fn method_site_enabled(ctx: &FnCtx<'_>, property: &str, argc: usize) -> bool {
    site_target_enabled(ctx)
        && !(property.is_empty()
            || property.starts_with('#')
            || property.starts_with("__perry_")
            || property.starts_with("@@")
            || property == "constructor"
            || argc > 16)
}

/// Is the keyed method site ([`emit_keyed_method_site`]) available for a
/// computed-key call `recv[key](argc args)`?
pub(crate) fn keyed_method_site_enabled(ctx: &FnCtx<'_>, argc: usize) -> bool {
    site_target_enabled(ctx) && argc <= 16
}

/// The method site's target and build conditions, independent of the key.
fn site_target_enabled(ctx: &FnCtx<'_>) -> bool {
    static OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *OFF.get_or_init(|| std::env::var("PERRY_METHOD_SITE").as_deref() == Ok("0")) {
        return false;
    }
    // 64-bit targets only: the site addresses 8-byte slots behind a 16-byte
    // header and compares an 8-byte receiver word.
    let triple = ctx.target_triple;
    if !(triple.starts_with("x86_64")
        || triple.starts_with("aarch64")
        || triple.starts_with("arm64"))
        || triple.contains("32")
    {
        return false;
    }
    // A typed-feedback (profiling) build records every method call in the
    // dispatcher; a site hit would skip that recording.
    !super::typed_feedback::typed_feedback_emission_enabled()
}

/// The computed-key method site: `recv[key](args)` whose arguments cannot
/// observe the method read. The key's atom probes the receiver's own layout,
/// then its prototypes' (`perry_probe_get_keyed`: data properties only, a
/// GC leaf); a function value that takes `this` as its parameter is called
/// with `recv` as `this`, exactly as the static-key site calls its probe's
/// answer. Anything else (a decline, a key that is no atom, an accessor, a
/// closure reading `this` from a capture) calls `slow` with the same
/// operands. `recv_box`, `key_box` and `lowered_args` are evaluated and
/// re-read from their roots.
pub(crate) fn emit_keyed_method_site(
    ctx: &mut FnCtx<'_>,
    recv_box: &str,
    key_box: &str,
    lowered_args: &[String],
    slow: impl FnOnce(&mut FnCtx<'_>) -> String,
) -> String {
    let slow_idx = ctx.new_block("ksite.slow");
    let merge_idx = ctx.new_block("ksite.merge");
    let slow_l = ctx.block_label(slow_idx);
    let merge_l = ctx.block_label(merge_idx);
    let dir = crate::expr::agent_ptr::emit_agent_ptr_or(
        ctx,
        crate::runtime_abi::AGENT_PTR_SHAPE_DIR,
        EMPTY_SHAPE_DIR,
    );
    let answered_bits = {
        let blk = ctx.block();
        let recv_bits = blk.bitcast_double_to_i64(recv_box);
        let key_bits = blk.bitcast_double_to_i64(key_box);
        let answered = blk.call(
            DOUBLE,
            "perry_probe_get_keyed",
            &[(PTR, &dir), (I64, &recv_bits), (I64, &key_bits)],
        );
        blk.bitcast_double_to_i64(&answered)
    };
    crate::expr::note_site("probe-keyed-method");
    let mut results: Vec<(String, String)> = Vec::new();
    emit_function_value_call(
        ctx,
        &answered_bits,
        recv_box,
        lowered_args,
        &[],
        &slow_l,
        &merge_l,
        &mut results,
    );
    ctx.current_block = slow_idx;
    let slow_value = slow(ctx);
    let slow_end = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_l);
    }
    results.push((slow_value, slow_end));
    ctx.current_block = merge_idx;
    let incoming: Vec<(&str, &str)> = results
        .iter()
        .map(|(v, l)| (v.as_str(), l.as_str()))
        .collect();
    ctx.block().phi(DOUBLE, &incoming)
}

/// Can evaluating `args` observe WHEN the method is read (#11910)? Only an
/// argument that runs code, writes state or reads state that code run by the
/// read (a getter, a Proxy trap) could write. Such a call takes the split site
/// ([`emit_method_site_lookup`]); every other call keeps the fused one.
///
/// Order-free: literals, `this`, a function reference, a closure or a literal
/// of order-free parts (creation runs no user code), a local no other
/// function can write: not boxed (a boxed local is captured and written by
/// some closure) and not a module global (any function may write it), and
/// operators over order-free parts that run no user code (`!`, `typeof`, `void`,
/// `===`, `&&`, `?:`, and arithmetic the type analysis proved primitive).
pub(crate) fn args_may_observe_lookup(ctx: &FnCtx<'_>, args: &[perry_hir::Expr]) -> bool {
    args.iter().any(|a| !arg_is_order_free(ctx, a))
}

fn arg_is_order_free(ctx: &FnCtx<'_>, e: &perry_hir::Expr) -> bool {
    use perry_hir::{CompareOp, Expr, UnaryOp};
    let private = |id: &perry_hir::types::LocalId| {
        !ctx.boxed_vars.contains(id) && !ctx.module_globals.contains_key(id)
    };
    match e {
        Expr::Undefined
        | Expr::Null
        | Expr::Bool(_)
        | Expr::Number(_)
        | Expr::Integer(_)
        | Expr::String(_)
        | Expr::This
        | Expr::FuncRef(_)
        | Expr::Closure { .. } => true,
        Expr::LocalGet(id) => private(id),
        // `i++` on a number local nothing else can see: a numeric add and a
        // store no getter can observe.
        Expr::Update { id, .. } => private(id) && crate::rooting::expr_is_inert_primitive(ctx, e),
        Expr::Array(items, _) => items.iter().all(|i| arg_is_order_free(ctx, i)),
        Expr::Object(props) => props.iter().all(|(_, v)| arg_is_order_free(ctx, v)),
        // ToBoolean, `typeof` and strict equality never run user code.
        Expr::TypeOf(operand)
        | Expr::Void(operand)
        | Expr::Unary {
            op: UnaryOp::Not,
            operand,
        } => arg_is_order_free(ctx, operand),
        Expr::Compare {
            op: CompareOp::Eq | CompareOp::Ne,
            left,
            right,
        }
        | Expr::Logical { left, right, .. } => {
            arg_is_order_free(ctx, left) && arg_is_order_free(ctx, right)
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            arg_is_order_free(ctx, condition)
                && arg_is_order_free(ctx, then_expr)
                && arg_is_order_free(ctx, else_expr)
        }
        // Every other operator converts its operands (ToPrimitive /
        // ToNumeric), which runs user code unless the type analysis proved
        // them primitives.
        Expr::Unary { operand, .. } => {
            crate::rooting::expr_is_inert_primitive(ctx, e) && arg_is_order_free(ctx, operand)
        }
        Expr::Binary { left, right, .. } | Expr::Compare { left, right, .. } => {
            crate::rooting::expr_is_inert_primitive(ctx, e)
                && arg_is_order_free(ctx, left)
                && arg_is_order_free(ctx, right)
        }
        _ => false,
    }
}

/// `GC_TYPE_OBJECT` as the `i8` an `obj_type` header byte holds.

/// Emit the site. `recv_box` and `lowered_args` are already evaluated and
/// re-read from their roots; `feedback_site` is the typed-feedback site id the
/// miss hands to the dispatcher; `method_id` the static dispatch id.

/// The site's key: its atom's bits (loaded from the string pool's handle
/// global), the atom's compile-time hash, and the agent's directory.
pub(crate) fn emit_site_key(ctx: &mut FnCtx<'_>, property: &str) -> (String, String, String) {
    let key_idx = ctx.strings.intern(property);
    let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
    let dir = crate::expr::agent_ptr::emit_agent_ptr_or(
        ctx,
        crate::runtime_abi::AGENT_PTR_SHAPE_DIR,
        EMPTY_SHAPE_DIR,
    );
    let key_box = ctx.block().load(DOUBLE, &key_handle_global);
    let key_bits = ctx.block().bitcast_double_to_i64(&key_box);
    let hash = (crate::layout_desc::atom_hash(property) as i64).to_string();
    (dir, key_bits, hash)
}

/// Call the function value `value_bits` (`i64`) with `recv_box` as `this`
/// when it is a live closure that takes `this` as its parameter: a
/// compile-time candidate body (a static lane's body) directly, any other
/// body through `js_closure_call{N}`. Anything else branches to `decline_l`.
/// Pushes each call's (result, end label); every call block branches to
/// `merge_l`.
#[allow(clippy::too_many_arguments)]
fn emit_function_value_call(
    ctx: &mut FnCtx<'_>,
    value_bits: &str,
    recv_box: &str,
    lowered_args: &[String],
    lanes: &[crate::codegen::static_constfn::StaticMethodLane],
    decline_l: &str,
    merge_l: &str,
    results: &mut Vec<(String, String)>,
) {
    use crate::expr::receiver_range::{emit_field_ptr, emit_handle, RECEIVER_BIAS, RECEIVER_SPAN};
    let kind_offset = -(crate::runtime_abi::GC_HEADER_SIZE as i64);
    let kind_mask = (0xFFu16 | (u16::from(crate::runtime_abi::GC_FLAG_FORWARDED) << 8)).to_string();
    let closure_kind = crate::runtime_abi::GC_TYPE_CLOSURE.to_string();
    let info_offset = crate::runtime_abi::CLOSURE_INFO_OFFSET as i64;
    let this_flag = (crate::runtime_abi::CLOSURE_CAPTURES_THIS_FLAG as i32).to_string();
    let kind_idx = ctx.new_block("msite.kind");
    let call_idx = ctx.new_block("msite.call");
    let kind_l = ctx.block_label(kind_idx);
    let call_l = ctx.block_label(call_idx);
    let u = {
        let blk = ctx.block();
        let u = blk.sub(I64, value_bits, &(RECEIVER_BIAS as i64).to_string());
        let heap = blk.icmp_ult(I64, &u, &(RECEIVER_SPAN as i64).to_string());
        blk.cond_br(&heap, &kind_l, decline_l);
        u
    };
    ctx.current_block = kind_idx;
    {
        let blk = ctx.block();
        let kp = emit_field_ptr(blk, &u, kind_offset);
        let kind = blk.load(I16, &kp);
        let kind = blk.and(I16, &kind, &kind_mask);
        let is_closure = blk.icmp_eq(I16, &kind, &closure_kind);
        let cp = emit_field_ptr(blk, &u, 0);
        let count = blk.load(I32, &cp);
        let this_bit = blk.and(I32, &count, &this_flag);
        let plain_this = blk.icmp_eq(I32, &this_bit, "0");
        let ok = blk.and(I1, &is_closure, &plain_this);
        blk.cond_br(&ok, &call_l, decline_l);
    }
    ctx.current_block = call_idx;
    let handle = emit_handle(ctx.block(), &u);
    let recv_bits = ctx.block().bitcast_double_to_i64(recv_box);
    let undefined = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
    let mut seen: Vec<&str> = Vec::new();
    if !lanes.is_empty() {
        let code = {
            let blk = ctx.block();
            let ip = emit_field_ptr(blk, &u, info_offset);
            let info = blk.load(PTR, &ip);
            blk.load(I64, &info)
        };
        for lane in lanes {
            if seen.contains(&lane.body.as_str()) {
                continue;
            }
            seen.push(&lane.body);
            // An inherited candidate can belong to another module. Naming
            // its entry as a pointer needs the same lazy declare as a call.
            ctx.pending_declares.push((
                lane.body.clone(),
                DOUBLE,
                crate::expr::body_call::js_body_param_types(lane.arity),
            ));
            let direct_idx = ctx.new_block("msite.candidate");
            let next_idx = ctx.new_block("msite.candidate_next");
            let direct_l = ctx.block_label(direct_idx);
            let next_l = ctx.block_label(next_idx);
            {
                let blk = ctx.block();
                let body_addr = blk.ptrtoint(&format!("@{}", lane.body), I64);
                let same = blk.icmp_eq(I64, &code, &body_addr);
                blk.cond_br(&same, &direct_l, &next_l);
            }
            ctx.current_block = direct_idx;
            let mut args: Vec<String> = lowered_args.iter().take(lane.arity).cloned().collect();
            args.resize(lane.arity, undefined.clone());
            let blk = ctx.block();
            let r = crate::expr::body_call::emit_js_body_call(
                blk,
                crate::expr::body_call::JsBody::Symbol(&lane.body),
                &handle,
                &recv_bits,
                &args,
            );
            let end = blk.label.clone();
            if !blk.is_terminated() {
                blk.br(merge_l);
            }
            results.push((r, end));
            ctx.current_block = next_idx;
        }
    }
    let r =
        crate::lower_call::emit_closure_handle_call_this(ctx, &handle, &recv_bits, lowered_args);
    let end = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(merge_l);
    }
    results.push((r, end));
}

/// Emit the site. `recv_box` and `lowered_args` are already evaluated and
/// re-read from their roots; `feedback_site` is the typed-feedback site id the
/// slow call hands to the dispatcher; `method_id` the static dispatch id.
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_method_site(
    ctx: &mut FnCtx<'_>,
    property: &str,
    recv_box: &str,
    lowered_args: &[String],
    feedback_site: &str,
    method_id: &str,
    lanes: &[crate::codegen::static_constfn::StaticMethodLane],
) -> String {
    use crate::expr::receiver_range::{
        emit_field_ptr, emit_fused_receiver_test, emit_handle, RECEIVER_BIAS,
    };
    let probe_idx = ctx.new_block("msite.probe");
    let slow_idx = ctx.new_block("msite.slow");
    let merge_idx = ctx.new_block("msite.merge");
    let probe_l = ctx.block_label(probe_idx);
    let slow_l = ctx.block_label(slow_idx);
    let merge_l = ctx.block_label(merge_idx);
    let mut results: Vec<(String, String)> = Vec::new();

    // Only a lane the receiver may itself carry is compared; every lane's
    // body is a candidate of the probe's answer below.
    let compared: Vec<&crate::codegen::static_constfn::StaticMethodLane> =
        lanes.iter().filter(|lane| lane.own).collect();
    let lanes_idx = (!compared.is_empty()).then(|| ctx.new_block("msite.static"));
    let lanes_l = match lanes_idx {
        Some(idx) => ctx.block_label(idx),
        None => probe_l.clone(),
    };
    // A create family's object: the method through its chain's carried
    // layouts (`create_families::HolderRead`), else the lanes and the probe.
    let holder = ctx.create_families.method_holder.take();
    let holder_idx = holder.as_ref().map(|_| ctx.new_block("msite.holder"));
    let first_l = match holder_idx {
        Some(idx) => ctx.block_label(idx),
        None => lanes_l.clone(),
    };
    // A primitive, a native handle: the slow call. So is a heap receiver
    // other than an ordinary object (an array, a function, a collection)
    // when the key names a builtin method: its direct entry
    // (`emit_slow_call`) proves the receiver's kind, that it has no own key
    // of the name and that its family's prototype is unmodified, and
    // otherwise takes the universal dispatcher, so the probe's prototype
    // walk would only find the same builtin.
    let builtin = crate::builtin_proto::builtin_method(property, lowered_args.len()).is_some();
    let kind_idx = builtin.then(|| ctx.new_block("msite.kind"));
    let on_object = match kind_idx {
        Some(idx) => ctx.block_label(idx),
        None => first_l.clone(),
    };
    let biased = {
        let blk = ctx.block();
        let bits = blk.bitcast_double_to_i64(recv_box);
        let fused = emit_fused_receiver_test(blk, &bits);
        blk.cond_br(&fused.is_object_pointer, &on_object, &slow_l);
        fused.biased
    };
    // A WeakMap/WeakSet is an ordinary cell: its birth layout word names it
    // (Z2), and with its family's prototype pristine the call is the runtime
    // operation itself; any other ordinary object takes the probe.
    let weak = crate::builtin_proto::weak_method_entry(property, lowered_args.len());
    let weak_idx = weak.map(|_| ctx.new_block("msite.weak"));
    if let Some(idx) = kind_idx {
        ctx.current_block = idx;
        let on_ordinary = match weak_idx {
            Some(w) => ctx.block_label(w),
            None => first_l.clone(),
        };
        let blk = ctx.block();
        // GcHeader precedes the cell: obj_type @-8 (i8).
        let tp = emit_field_ptr(blk, &biased, -8);
        let ty = blk.load(crate::types::I8, &tp);
        let ordinary = crate::target_layout::emit_gc_type_is_object(blk, crate::types::I8, &ty);
        blk.cond_br(&ordinary, &on_ordinary, &slow_l);
    }
    if let (Some(idx), Some((entry, births))) = (weak_idx, weak) {
        ctx.current_block = idx;
        let word = {
            let blk = ctx.block();
            let wp = emit_field_ptr(blk, &biased, 4);
            blk.load(I32, &wp)
        };
        for (i, (birth, family)) in births.iter().enumerate() {
            let born_idx = ctx.new_block("msite.weak_born");
            let call_idx = ctx.new_block("msite.weak_call");
            let born_l = ctx.block_label(born_idx);
            let call_l = ctx.block_label(call_idx);
            let next = (i + 1 < births.len()).then(|| ctx.new_block("msite.weak"));
            let next_l = match next {
                Some(n) => ctx.block_label(n),
                None => first_l.clone(),
            };
            {
                let blk = ctx.block();
                let eq = blk.icmp_eq(I32, &word, &(*birth as i32).to_string());
                blk.cond_br(&eq, &born_l, &next_l);
            }
            ctx.current_block = born_idx;
            let patched = crate::builtin_proto::emit_patched_test(ctx, *family);
            ctx.block().cond_br(&patched, &slow_l, &call_l);
            ctx.current_block = call_idx;
            let mut operands: Vec<(crate::types::LlvmType, &str)> = vec![(DOUBLE, recv_box)];
            operands.extend(lowered_args.iter().map(|a| (DOUBLE, a.as_str())));
            let blk = ctx.block();
            let r = blk.call(DOUBLE, entry, &operands);
            let end = blk.label.clone();
            blk.br(&merge_l);
            results.push((r, end));
            if let Some(n) = next {
                ctx.current_block = n;
            }
        }
    }
    if let (Some(idx), Some(read)) = (holder_idx, holder.as_ref()) {
        ctx.current_block = idx;
        let value =
            crate::layout_desc::create_families::emit_holder_load(ctx, read, &biased, &lanes_l);
        emit_function_value_call(
            ctx,
            &value,
            recv_box,
            lowered_args,
            lanes,
            &slow_l,
            &merge_l,
            &mut results,
        );
    }
    // Static ConstFn lanes: the receiver's completed shape names the body, so
    // the hit is a direct call. The slot value is the callee environment only
    // (factory objects share a body, not captures); the shape proves it is a
    // closure of that body.
    if let Some(idx) = lanes_idx {
        ctx.current_block = idx;
        let w = {
            let blk = ctx.block();
            let wp = emit_field_ptr(blk, &biased, 0);
            blk.load(I64, &wp)
        };
        let undefined = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
        for (i, lane) in compared.iter().enumerate() {
            let hit_idx = ctx.new_block("msite.static_hit");
            let hit_l = ctx.block_label(hit_idx);
            let next_l = if i + 1 < compared.len() {
                let n = ctx.new_block("msite.static");
                (Some(n), ctx.block_label(n))
            } else {
                (None, probe_l.clone())
            };
            {
                let blk = ctx.block();
                let eq = blk.icmp_eq(I64, &w, &lane.word.to_string());
                blk.cond_br(&eq, &hit_l, &next_l.1);
            }
            ctx.current_block = hit_idx;
            let mut call_args: Vec<String> =
                lowered_args.iter().take(lane.arity).cloned().collect();
            call_args.resize(lane.arity, undefined.clone());
            let header =
                crate::target_layout::object_slot_base_bytes(ctx.target_triple, lane.form()) as i64;
            let blk = ctx.block();
            let vp = emit_field_ptr(blk, &biased, header + 8 * i64::from(lane.slot));
            let v = blk.load(I64, &vp);
            let ub = blk.sub(I64, &v, &(RECEIVER_BIAS as i64).to_string());
            let env = emit_handle(blk, &ub);
            let recv_bits = blk.bitcast_double_to_i64(recv_box);
            let r = crate::expr::body_call::emit_js_body_call(
                blk,
                crate::expr::body_call::JsBody::Symbol(&lane.body),
                &env,
                &recv_bits,
                &call_args,
            );
            let end = blk.label.clone();
            if !blk.is_terminated() {
                blk.br(&merge_l);
            }
            results.push((r, end));
            if let Some(n) = next_l.0 {
                ctx.current_block = n;
            }
        }
    }

    // The probe: the receiver's own layout, then its prototypes'.
    ctx.current_block = probe_idx;
    let (dir, key_bits, hash) = emit_site_key(ctx, property);
    let family_words = crate::layout_desc::methods::family_words(&ctx.program_layouts, property);
    if !family_words.is_empty() {
        let family_call = ctx.new_block("msite.family_call");
        let ordinary_probe = ctx.new_block("msite.ordinary_probe");
        let family_l = ctx.block_label(family_call);
        let ordinary_l = ctx.block_label(ordinary_probe);
        let family_probe = ctx.new_block("msite.family_probe");
        let family_probe_l = ctx.block_label(family_probe);
        let wp = emit_field_ptr(ctx.block(), &biased, 4);
        let word = ctx.block().load(I32, &wp);
        // Only rows whose emitted index contains this name can answer. The
        // runtime still checks the live receiver, holders, and call ABI.
        let mut dispatch = format!("switch i32 {word}, label %{ordinary_l} [");
        for word in family_words {
            dispatch.push_str(&format!(" i32 {word}, label %{family_probe_l}"));
        }
        dispatch.push_str(" ]");
        ctx.block().emit_raw(dispatch);
        ctx.block().mark_terminated();
        ctx.current_block = family_probe;
        let callee = ctx.block().call(
            I64,
            "perry_probe_family_method",
            &[
                (PTR, &dir),
                (I64, &biased),
                (I64, &key_bits),
                (I64, &hash),
                (I32, &lowered_args.len().to_string()),
            ],
        );
        let present = ctx.block().icmp_ne(I64, &callee, "0");
        ctx.block().cond_br(&present, &family_l, &ordinary_l);
        ctx.current_block = family_call;
        let recv_bits = ctx.block().bitcast_double_to_i64(recv_box);
        let ptr = ctx.block().inttoptr(I64, &callee);
        let ip = ctx.block().gep(
            crate::types::I8,
            &ptr,
            &[(I64, &crate::runtime_abi::CLOSURE_INFO_OFFSET.to_string())],
        );
        let info = ctx.block().load(PTR, &ip);
        let code = ctx.block().load(PTR, &info);
        let r = crate::expr::body_call::emit_js_body_call(
            ctx.block(),
            crate::expr::body_call::JsBody::Pointer(&code),
            &callee,
            &recv_bits,
            lowered_args,
        );
        let end = ctx.block().label.clone();
        ctx.block().br(&merge_l);
        results.push((r, end));
        ctx.current_block = ordinary_probe;
    }
    crate::expr::note_site("probe-method");
    let answered = ctx.block().call(
        DOUBLE,
        "perry_probe_get",
        &[(PTR, &dir), (I64, &biased), (I64, &key_bits), (I64, &hash)],
    );
    let answered_bits = ctx.block().bitcast_double_to_i64(&answered);
    emit_function_value_call(
        ctx,
        &answered_bits,
        recv_box,
        lowered_args,
        lanes,
        &slow_l,
        &merge_l,
        &mut results,
    );

    // slow: a builtin receiver's direct entry when the key names a builtin
    // method (§3.6), else the universal dispatcher.
    ctx.current_block = slow_idx;
    let slow_value = emit_slow_call(
        ctx,
        property,
        recv_box,
        lowered_args,
        feedback_site,
        method_id,
    );
    let slow_end = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_l);
    }
    results.push((slow_value, slow_end));

    ctx.current_block = merge_idx;
    let incoming: Vec<(&str, &str)> = results
        .iter()
        .map(|(v, l)| (v.as_str(), l.as_str()))
        .collect();
    ctx.block().phi(DOUBLE, &incoming)
}

/// The site's call once the probe declined: `js_bm_<property><argc>` (a
/// builtin receiver's direct call, `builtin_proto::BUILTIN_METHODS`) when the
/// key and argument count name a builtin method, which itself takes the
/// universal dispatcher for any other receiver; else the dispatcher
/// (`js_method_call_slow`) with the arguments in a buffer.
fn emit_slow_call(
    ctx: &mut FnCtx<'_>,
    property: &str,
    recv_box: &str,
    lowered_args: &[String],
    feedback_site: &str,
    method_id: &str,
) -> String {
    let slow_key = {
        let key_idx = ctx.strings.intern(property);
        let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
        let key_box = ctx.block().load(DOUBLE, &key_handle_global);
        ctx.block().bitcast_double_to_i64(&key_box)
    };
    if let Some(entry) = crate::builtin_proto::builtin_method(property, lowered_args.len()) {
        let mut operands: Vec<(crate::types::LlvmType, &str)> = vec![
            (I64, feedback_site),
            (DOUBLE, recv_box),
            (I64, &slow_key),
            (I64, method_id),
        ];
        operands.extend(lowered_args.iter().map(|a| (DOUBLE, a.as_str())));
        return ctx.block().call(DOUBLE, &entry, &operands);
    }
    let (args_ptr, argc) = crate::lower_call::emit_method_args_buffer(ctx, lowered_args);
    ctx.block().call(
        DOUBLE,
        "js_method_call_slow",
        &[
            (I64, feedback_site),
            (DOUBLE, recv_box),
            (I64, &slow_key),
            (I64, method_id),
            (PTR, &args_ptr),
            (I64, &argc),
        ],
    )
}

/// What a split site's lookup half read (#11910), consumed by
/// [`emit_method_site_call`] after the arguments.
pub(crate) struct SiteLookup {
    /// `double`: the method value the read produced, or a by-name answer. A
    /// GC value: the caller roots it across the arguments.
    pub(crate) value: String,
    pub(crate) lanes: Vec<crate::codegen::static_constfn::StaticMethodLane>,
    /// The site's key: a by-name answer for a builtin receiver calls its
    /// direct entry (`builtin_proto::builtin_method`).
    pub(crate) property: String,
}

/// The site's lookup half, emitted BEFORE a call's arguments when they can
/// run code (#11910: ECMA-262 13.3.6.1 reads `o.m` before the arguments).
/// One call: `js_method_lookup` answers the probe's function value, else
/// performs the spec read, which runs a getter, a Proxy trap or a nullish
/// receiver's TypeError now, or answers "by name" for a receiver whose read
/// nothing can observe. The caller roots [`SiteLookup::value`] across the
/// arguments and then calls [`emit_method_site_call`].
pub(crate) fn emit_method_site_lookup(
    ctx: &mut FnCtx<'_>,
    property: &str,
    recv_box: &str,
    method_id: &str,
    lanes: &[crate::codegen::static_constfn::StaticMethodLane],
) -> SiteLookup {
    let (dir, key_bits, hash) = emit_site_key(ctx, property);
    // A key that names a builtin method: a builtin receiver whose family's
    // pristine prototype answers it reads nothing observable
    // (`js_bm_lookup`).
    // An ordinary object (not a weak collection on its birth layout, Z2)
    // is no builtin receiver: its lookup is `js_method_lookup` at once.
    let families = crate::builtin_proto::builtin_method_families(property);
    let lookup = |ctx: &mut FnCtx<'_>| {
        ctx.block().call(
            DOUBLE,
            "js_method_lookup",
            &[
                (PTR, &dir),
                (DOUBLE, recv_box),
                (I64, &key_bits),
                (I64, &hash),
                (I64, method_id),
            ],
        )
    };
    let value = if families != 0 {
        use crate::expr::receiver_range::{emit_field_ptr, emit_fused_receiver_test};
        let kind_idx = ctx.new_block("msite.lookup_kind");
        let builtin_idx = ctx.new_block("msite.lookup_builtin");
        let plain_idx = ctx.new_block("msite.lookup_plain");
        let join_idx = ctx.new_block("msite.lookup_join");
        let kind_l = ctx.block_label(kind_idx);
        let builtin_l = ctx.block_label(builtin_idx);
        let plain_l = ctx.block_label(plain_idx);
        let join_l = ctx.block_label(join_idx);
        let biased = {
            let blk = ctx.block();
            let bits = blk.bitcast_double_to_i64(recv_box);
            let fused = emit_fused_receiver_test(blk, &bits);
            blk.cond_br(&fused.is_object_pointer, &kind_l, &builtin_l);
            fused.biased
        };
        ctx.current_block = kind_idx;
        {
            let blk = ctx.block();
            let tp = emit_field_ptr(blk, &biased, -8);
            let ty = blk.load(crate::types::I8, &tp);
            let ordinary = crate::target_layout::emit_gc_type_is_object(blk, crate::types::I8, &ty);
            let plain = if crate::builtin_proto::weak_families(families) {
                let wp = emit_field_ptr(blk, &biased, 4);
                let word = blk.load(I32, &wp);
                let base = (crate::runtime_abi::WEAKMAP_BIRTH_LAYOUT_WORD as i32).to_string();
                let off = blk.sub(I32, &word, &base);
                let born = blk.icmp_ult(I32, &off, "2");
                let not_born = blk.xor(I1, &born, "true");
                blk.and(I1, &ordinary, &not_born)
            } else {
                ordinary
            };
            blk.cond_br(&plain, &plain_l, &builtin_l);
        }
        ctx.current_block = plain_idx;
        let plain_v = lookup(ctx);
        let plain_end = ctx.block().label.clone();
        ctx.block().br(&join_l);
        ctx.current_block = builtin_idx;
        let builtin_v = ctx.block().call(
            DOUBLE,
            "js_bm_lookup",
            &[
                (PTR, &dir),
                (DOUBLE, recv_box),
                (I64, &key_bits),
                (I64, &hash),
                (I64, method_id),
                (I64, &families.to_string()),
            ],
        );
        let builtin_end = ctx.block().label.clone();
        ctx.block().br(&join_l);
        ctx.current_block = join_idx;
        ctx.block().phi(
            DOUBLE,
            &[
                (plain_v.as_str(), plain_end.as_str()),
                (builtin_v.as_str(), builtin_end.as_str()),
            ],
        )
    } else {
        lookup(ctx)
    };
    SiteLookup {
        value,
        lanes: lanes.to_vec(),
        property: property.to_string(),
    }
}

/// [`emit_method_site_lookup`] behind a guard: when `skip` (an `i1`) holds,
/// no lookup runs, and the caller answers from `skip` itself before
/// the call half. The interface tower's implementor arms read nothing and
/// skip it.
pub(crate) fn emit_method_site_lookup_unless(
    ctx: &mut FnCtx<'_>,
    property: &str,
    recv_box: &str,
    method_id: &str,
    skip: &str,
) -> SiteLookup {
    let lookup_idx = ctx.new_block("msite.lookup");
    let join_idx = ctx.new_block("msite.lookup_join");
    let lookup_l = ctx.block_label(lookup_idx);
    let join_l = ctx.block_label(join_idx);
    let skip_end = ctx.block().label.clone();
    ctx.block().cond_br(skip, &join_l, &lookup_l);
    ctx.current_block = lookup_idx;
    let looked = emit_method_site_lookup(ctx, property, recv_box, method_id, &[]);
    let looked_end = ctx.block().label.clone();
    ctx.block().br(&join_l);
    ctx.current_block = join_idx;
    let undefined = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
    let value = ctx.block().phi(
        DOUBLE,
        &[(&looked.value, &looked_end), (&undefined, &skip_end)],
    );
    SiteLookup { value, ..looked }
}

/// Is the method the lookup read nullish (an optional call short-circuits)?
/// A hit holds a closure and a read value is tested; a by-name answer read
/// nothing, so `by_name_guard` (the source guard, whose re-read nothing can
/// observe) answers. Returns an `i1`.
pub(crate) fn emit_lookup_nullish(
    ctx: &mut FnCtx<'_>,
    value: &str,
    by_name_guard: impl FnOnce(&mut FnCtx<'_>) -> anyhow::Result<String>,
) -> anyhow::Result<String> {
    let named_idx = ctx.new_block("msite.opt_named");
    let read_idx = ctx.new_block("msite.opt_read");
    let join_idx = ctx.new_block("msite.opt_join");
    let named_l = ctx.block_label(named_idx);
    let read_l = ctx.block_label(read_idx);
    let join_l = ctx.block_label(join_idx);
    let bits = {
        let blk = ctx.block();
        let bits = blk.bitcast_double_to_i64(value);
        let named = emit_by_name_test(blk, &bits);
        blk.cond_br(&named, &named_l, &read_l);
        bits
    };
    ctx.current_block = named_idx;
    let g = by_name_guard(ctx)?;
    let named_end = ctx.block().label.clone();
    ctx.block().br(&join_l);
    ctx.current_block = read_idx;
    let r = {
        let blk = ctx.block();
        let undef = blk.icmp_eq(I64, &bits, &crate::nanbox::TAG_UNDEFINED.to_string());
        let null = blk.icmp_eq(I64, &bits, &crate::nanbox::TAG_NULL.to_string());
        let r = blk.or(I1, &undef, &null);
        blk.br(&join_l);
        r
    };
    let read_end = ctx.block_label(read_idx);
    ctx.current_block = join_idx;
    Ok(ctx.block().phi(I1, &[(&g, &named_end), (&r, &read_end)]))
}

/// `bits | 2 == BY_NAME_DIRECT`: either by-name answer (no JS value, and no
/// hit's closure, has either pattern).
fn emit_by_name_test(blk: &mut crate::block::LlBlock, bits: &str) -> String {
    let either = blk.or(I64, bits, "2");
    blk.icmp_eq(
        I64,
        &either,
        &crate::runtime_abi::METHOD_SITE_BY_NAME_DIRECT.to_string(),
    )
}

/// The call half of a split site: `value` is [`SiteLookup::value`] re-read
/// from its root below the arguments, `recv_box` likewise. A plain function
/// value is called with the receiver as `this` (a compile-time candidate body
/// by name, so it can be inlined); anything else is one runtime call,
/// `js_method_call_split`, which dispatches by name or calls the read value.
/// `args_buffer` emits the dispatcher's argument buffer, on that path only.
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_method_site_call(
    ctx: &mut FnCtx<'_>,
    lookup: &SiteLookup,
    recv_box: &str,
    value: &str,
    lowered_args: &[String],
    feedback_site: &str,
    method_id: &str,
    args_buffer: impl FnOnce(&mut FnCtx<'_>) -> (String, String),
) -> String {
    let named_idx = ctx.new_block("msite.split_named");
    let merge_idx = ctx.new_block("msite.split_merge");
    let named_l = ctx.block_label(named_idx);
    let merge_l = ctx.block_label(merge_idx);
    let mut results: Vec<(String, String)> = Vec::new();
    let value_bits = ctx.block().bitcast_double_to_i64(value);
    emit_function_value_call(
        ctx,
        &value_bits,
        recv_box,
        lowered_args,
        &lookup.lanes,
        &named_l,
        &merge_l,
        &mut results,
    );
    // Not a plain function value: one runtime call dispatches by name or
    // calls the read value. A by-name answer for a key that names a builtin
    // method calls the builtin receiver's direct entry.
    ctx.current_block = named_idx;
    if crate::builtin_proto::builtin_method(&lookup.property, lowered_args.len()).is_some() {
        let entry_idx = ctx.new_block("msite.split_builtin");
        let value_idx = ctx.new_block("msite.split_value");
        let entry_l = ctx.block_label(entry_idx);
        let value_l = ctx.block_label(value_idx);
        let named = emit_by_name_test(ctx.block(), &value_bits);
        ctx.block().cond_br(&named, &entry_l, &value_l);
        ctx.current_block = entry_idx;
        let r = emit_slow_call(
            ctx,
            &lookup.property,
            recv_box,
            lowered_args,
            feedback_site,
            method_id,
        );
        results.push((r, ctx.block().label.clone()));
        ctx.block().br(&merge_l);
        ctx.current_block = value_idx;
    }
    let (args_ptr, argc) = args_buffer(ctx);
    let r = ctx.block().call(
        DOUBLE,
        "js_method_call_split",
        &[
            (DOUBLE, value),
            (I64, feedback_site),
            (DOUBLE, recv_box),
            (I64, method_id),
            (PTR, &args_ptr),
            (I64, &argc),
        ],
    );
    results.push((r, ctx.block().label.clone()));
    ctx.block().br(&merge_l);
    ctx.current_block = merge_idx;
    let incoming: Vec<(&str, &str)> = results
        .iter()
        .map(|(v, l)| (v.as_str(), l.as_str()))
        .collect();
    ctx.block().phi(DOUBLE, &incoming)
}
