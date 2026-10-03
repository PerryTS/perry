//! The method-call site: `recv.m(args)` as ONE path — the property read's
//! receiver test and shape compare, a slot load (from the receiver or a
//! shape-guarded direct holder), then a direct call with `recv` as `this`.
//!
//! The site's memo is a runtime `MethodSite`
//! (`perry-runtime/src/object/method_site.rs`, which states what an entry
//! claims and why it cannot go stale). Emitted:
//!
//! ```text
//!   t   = bits - RECEIVER_BIAS ; t <u SPAN                 else PRIMITIVE
//!         site != null                                       else MISS
//!   w   = load [recv]           ; (class_id | ShapeId)
//!         w == site.word                                     else MISS
//!   s   = site.slot
//!   s < 0 (inherited):  [site.holder] == site.holder_word     else MISS
//!                       v = load [site.holder + HDR + 8*i]
//!   own:  v = load [recv + HDR + 8*s] ; v is a heap pointer  else MISS
//!   fn:   v = load [[recv + PROPS] + HDR + 8*s]  (bit 61: a function's
//!         own-property object; same checks as own)
//!         [v-8] & 0x80FF == CLOSURE ; [v+8] == site.info      else NEXT WAY
//!         h = handle(v) ; f = site.code
//!   CALL: r = f(h, recv, args...)    (the receiver is the `this` parameter)
//!   MISS: js_method_site_miss(slot, feedback_site, recv, method_id, args)
//!   PRIMITIVE: js_typed_feedback_native_call_method_by_id(feedback_site, recv, method_id, args)
//! ```
//!
//! Step 5C (ConstFn lanes): a completed shape whose lane at the key's slot
//! names body B proves "this receiver's slot holds a closure of B". So
//!
//! * a STATIC lane (the receiver's candidate class has a completed shape F,
//!   known at compile time) is tested before the memo:
//!   `[recv] == (cid | F<<32)` -> `call @B(slot closure, recv, args)`, a
//!   direct call; the slot value is only the callee environment, unchecked;
//! * a learned ConstFn entry (own, or inherited through a holder whose shape
//!   carries the lane) is decoded first and calls `site.code` with no kind or
//!   info check of the slot value.
//!
//! The miss primes the entry when its facts hold and otherwise performs the
//! universal by-name dispatch, so every receiver the memo cannot describe —
//! primitives, native handles, Proxy, accessors, bound functions — keeps
//! exactly the behaviour it had.

use super::FnCtx;
use crate::types::{DOUBLE, I1, I32, I64, I8, PTR};

/// Is the One Path method site available for this call?
/// `PERRY_METHOD_SITE=0` at compile time keeps the old dispatcher (A/B).
pub(crate) fn method_site_enabled(ctx: &FnCtx<'_>, property: &str, argc: usize) -> bool {
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
    if super::typed_feedback::typed_feedback_emission_enabled() {
        return false;
    }
    !(property.is_empty()
        || property.starts_with('#')
        || property.starts_with("__perry_")
        || property.starts_with("@@")
        || property == "constructor"
        || argc > 16)
}

/// Emit the site. `recv_box` and `lowered_args` are already evaluated and
/// re-read from their roots; `feedback_site` is the typed-feedback site id the
/// miss hands to the dispatcher; `method_id` the static dispatch id.
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_method_site(
    ctx: &mut FnCtx<'_>,
    recv_box: &str,
    lowered_args: &[String],
    feedback_site: &str,
    method_id: &str,
    args_ptr: &str,
    argc: &str,
    lanes: &[crate::codegen::static_constfn::StaticMethodLane],
) -> String {
    use crate::expr::receiver_range::{
        emit_field_ptr, emit_fused_receiver_test, emit_handle, RECEIVER_BIAS, RECEIVER_SPAN,
    };
    let abi_word = crate::runtime_abi::METHOD_SITE_WORD_OFFSET.to_string();
    let abi_slot = crate::runtime_abi::METHOD_SITE_SLOT_OFFSET.to_string();
    // An entry memoizes a body as its `JsFunctionInfo` (the identity a
    // closure's info word is compared with) and its code address (the call
    // target), so a hit costs no load through the info.
    let abi_info = crate::runtime_abi::METHOD_SITE_INFO_OFFSET.to_string();
    let abi_code = crate::runtime_abi::METHOD_SITE_CODE_OFFSET.to_string();
    let abi_closure = crate::runtime_abi::METHOD_SITE_CLOSURE_OFFSET.to_string();
    let abi_gen = crate::runtime_abi::METHOD_SITE_GEN_OFFSET.to_string();
    let ways = crate::runtime_abi::METHOD_SITE_WAYS;
    let meta_offset = crate::target_layout::object_meta_slot_offset_bytes(ctx.target_triple) as i64;
    let spill_offset = crate::runtime_abi::OBJECT_META_SPILL_OFFSET.to_string();
    let array_header = crate::runtime_abi::ARRAY_HEADER_SIZE.to_string();
    let index_mask = crate::runtime_abi::METHOD_SITE_INDEX_MASK.to_string();
    let entry_size = crate::runtime_abi::METHOD_SITE_ENTRY_SIZE;
    let header = crate::target_layout::object_header_size_bytes(ctx.target_triple) as i64;
    // `ClosureHeader` (64-bit only here): the info pointer, and the GcHeader
    // (type, flags) half-word in front of the payload that makes a cell a
    // live function object: type `GC_TYPE_CLOSURE` and not a forwarded stub
    // (`closure::is_closure_ptr`'s kind term; there is no payload magic).
    let info_offset = crate::runtime_abi::CLOSURE_INFO_OFFSET as i64;
    let props_offset = crate::runtime_abi::CLOSURE_PROPS_OFFSET as i64;
    let kind_offset = -(crate::runtime_abi::GC_HEADER_SIZE as i64);
    let kind_mask = (0xFFu16 | (u16::from(crate::runtime_abi::GC_FLAG_FORWARDED) << 8)).to_string();
    let closure_kind = crate::runtime_abi::GC_TYPE_CLOSURE.to_string();

    let site_no = ctx.ic_site_counter;
    ctx.ic_site_counter += 1;
    let cache_name = crate::expr::inline_cache_global_name(ctx, site_no);
    ctx.ic_globals.push(cache_name.clone());

    let deref_idx = ctx.new_block("msite.deref");
    let kind_idx = ctx.new_block("msite.kind");
    let own_idx = ctx.new_block("msite.own");
    let own_fn_idx = ctx.new_block("msite.own_fn");
    let constfn_idx = ctx.new_block("msite.constfn");
    let value_idx = ctx.new_block("msite.value");
    let inh_idx = ctx.new_block("msite.inherited");
    let inh_value_idx = ctx.new_block("msite.inherited_value");
    let call_idx = ctx.new_block("msite.call");
    let miss_idx = ctx.new_block("msite.miss");
    let merge_idx = ctx.new_block("msite.merge");
    let deref_l = ctx.block_label(deref_idx);
    let kind_l = ctx.block_label(kind_idx);
    let own_l = ctx.block_label(own_idx);
    let own_fn_l = ctx.block_label(own_fn_idx);
    let constfn_l = ctx.block_label(constfn_idx);
    let value_l = ctx.block_label(value_idx);
    let inh_l = ctx.block_label(inh_idx);
    let inh_value_l = ctx.block_label(inh_value_idx);
    let call_l = ctx.block_label(call_idx);
    let miss_l = ctx.block_label(miss_idx);
    let merge_l = ctx.block_label(merge_idx);

    // Entry: the fused receiver test decides the RECEIVER KIND. A primitive
    // (a string, a number, a boolean, undefined, ...) is not the site's: it
    // takes the universal dispatcher directly, with its string and primitive
    // arms, exactly as without a site. A heap object takes the site: its memo
    // if the site has one, else the miss, which primes it.
    // The runtime publishes this process-global slot with an AtomicPtr CAS.
    // A worker may enter the site just as the primary agent first publishes
    // it, before the sticky worker gate below is loaded. Pair the load with
    // that publication even though the worker will then take the miss path.
    let slot_ref = format!("@{cache_name}");
    let cache = ctx.block().load_atomic_acquire(PTR, &slot_ref, 8);
    let present = ctx.block().icmp_ne(PTR, &cache, "null");
    let ic = crate::expr::InlineCacheSlot {
        slot_ref,
        cache,
        present,
    };
    let prim_idx = ctx.new_block("msite.primitive");
    let object_idx = ctx.new_block("msite.object");
    let prim_l = ctx.block_label(prim_idx);
    let object_l = ctx.block_label(object_idx);
    // Only a lane the receiver may itself carry is compared; every lane's
    // body is a candidate of the learned ConstFn hit below.
    let compared: Vec<&crate::codegen::static_constfn::StaticMethodLane> =
        lanes.iter().filter(|lane| lane.own).collect();
    let lanes_idx = (!compared.is_empty()).then(|| ctx.new_block("msite.static"));
    let first_l = match lanes_idx {
        Some(idx) => ctx.block_label(idx),
        None => object_l.clone(),
    };
    let biased = {
        let blk = ctx.block();
        let bits = blk.bitcast_double_to_i64(recv_box);
        let fused = emit_fused_receiver_test(blk, &bits);
        blk.cond_br(&fused.is_object_pointer, &first_l, &prim_l);
        fused.biased
    };
    // Static ConstFn lanes: the receiver's completed shape names the body, so
    // the hit is a direct call. The slot value is the callee environment only
    // (factory objects share a body, not captures); the shape proves it is a
    // closure of that body, so it is neither checked nor called through.
    // Static ids and their ConstFn records exist in every agent, so no worker
    // gate is needed; a receiver that is not on a lane falls to the memo.
    let mut lane_hits: Vec<(String, String)> = Vec::new();
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
                (None, object_l.clone())
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
            lane_hits.push((r, end));
            if let Some(n) = next_l.0 {
                ctx.current_block = n;
            }
        }
    }
    ctx.current_block = object_idx;
    // Site records are process-global, and inherited holders belong to the
    // primary heap. A worker's first startup publishes this sticky gate
    // before executing user code; afterward every agent takes the generic
    // path. No worker reads a primary holder or races a primary site update.
    let workers = ctx
        .block()
        .load_atomic_seq_cst(I8, "@PERRY_METHOD_SITE_WORKERS_PRESENT", 1);
    let no_workers = ctx.block().icmp_eq(I8, &workers, "0");
    let site_enabled = ctx.block().and(I1, &no_workers, &ic.present);
    ctx.block().cond_br(&site_enabled, &deref_l, &miss_l);

    // deref: the receiver word against each entry's word, in order.
    ctx.current_block = deref_idx;
    let w = {
        let blk = ctx.block();
        let wp = emit_field_ptr(blk, &biased, 0);
        blk.load(I64, &wp)
    };
    let mut found: Vec<(String, String)> = Vec::new();
    // The label where each way after the first starts its word compare: an
    // own entry whose code pointer does not match resumes there, so objects
    // of ONE shape holding different method bodies (a strategy field, a
    // literal the compiler unrolled) get one entry per body.
    let mut way_labels: Vec<String> = Vec::new();
    for way in 0..ways {
        let entry = if way == 0 {
            ic.cache.clone()
        } else {
            ctx.block().gep(
                crate::types::I8,
                &ic.cache,
                &[(I64, &(way * entry_size).to_string())],
            )
        };
        let next_l = if way + 1 < ways {
            let idx = ctx.new_block("msite.way");
            (Some(idx), ctx.block_label(idx))
        } else {
            (None, miss_l.clone())
        };
        let blk = ctx.block();
        let mw_p = blk.gep(crate::types::I8, &entry, &[(I64, &abi_word)]);
        let mw = blk.load(I64, &mw_p);
        let eq = blk.icmp_eq(I64, &w, &mw);
        found.push((entry, blk.label.clone()));
        blk.cond_br(&eq, &kind_l, &next_l.1);
        if let Some(idx) = next_l.0 {
            way_labels.push(next_l.1.clone());
            ctx.current_block = idx;
        }
    }
    // kind: an own inline slot when bits 59..63 are clear; otherwise route
    // inherited, spill, function-bag and ConstFn tags explicitly.
    ctx.current_block = kind_idx;
    let entry = {
        let incoming: Vec<(&str, &str)> = found
            .iter()
            .map(|(e, l)| (e.as_str(), l.as_str()))
            .collect();
        ctx.block().phi(PTR, &incoming)
    };
    let other_idx = ctx.new_block("msite.other");
    let other2_idx = ctx.new_block("msite.other2");
    let other3_idx = ctx.new_block("msite.other3");
    let other4_idx = ctx.new_block("msite.other4");
    let bag_idx = ctx.new_block("msite.fn_bag");
    let bag2_idx = ctx.new_block("msite.fn_bag_load");
    let spill_idx = ctx.new_block("msite.spill");
    let spill2_idx = ctx.new_block("msite.spill_buf");
    let spill3_idx = ctx.new_block("msite.spill_check");
    let spill4_idx = ctx.new_block("msite.spill_load");
    let other_l = ctx.block_label(other_idx);
    let other2_l = ctx.block_label(other2_idx);
    let other3_l = ctx.block_label(other3_idx);
    let other4_l = ctx.block_label(other4_idx);
    let bag_l = ctx.block_label(bag_idx);
    let bag2_l = ctx.block_label(bag2_idx);
    let spill_l = ctx.block_label(spill_idx);
    let spill2_l = ctx.block_label(spill2_idx);
    let spill3_l = ctx.block_label(spill3_idx);
    let spill4_l = ctx.block_label(spill4_idx);
    let cf_kind_idx = ctx.new_block("msite.kind_constfn");
    let icf_kind_idx = ctx.new_block("msite.kind_inherited_constfn");
    let icf_idx = ctx.new_block("msite.inherited_constfn");
    let icf_hit_idx = ctx.new_block("msite.inherited_constfn_hit");
    let cf_kind_l = ctx.block_label(cf_kind_idx);
    let icf_kind_l = ctx.block_label(icf_kind_idx);
    let icf_l = ctx.block_label(icf_idx);
    let icf_hit_l = ctx.block_label(icf_hit_idx);
    let cf_call_idx = ctx.new_block("msite.call_constfn");
    let cf_call_l = ctx.block_label(cf_call_idx);
    let (slot, top) = {
        let blk = ctx.block();
        let sp = blk.gep(crate::types::I8, &entry, &[(I64, &abi_slot)]);
        let s = blk.load(I64, &sp);
        let top = blk.lshr(I64, &s, "59");
        let tagged = blk.icmp_ne(I64, &top, "0");
        blk.cond_br(&tagged, &cf_kind_l, &own_l);
        (s, top)
    };
    // The ConstFn kinds are decoded before the older tags: own ConstFn is
    // exactly bit 59, inherited ConstFn exactly bits 63 and 59.
    ctx.current_block = cf_kind_idx;
    {
        let blk = ctx.block();
        let is_constfn = blk.icmp_eq(I64, &top, "1");
        blk.cond_br(&is_constfn, &constfn_l, &icf_kind_l);
    }
    ctx.current_block = icf_kind_idx;
    {
        let blk = ctx.block();
        let inherited_constfn = (crate::runtime_abi::METHOD_SITE_INHERITED
            | crate::runtime_abi::METHOD_SITE_CONSTFN)
            >> 59;
        let is_icf = blk.icmp_eq(I64, &top, &inherited_constfn.to_string());
        blk.cond_br(&is_icf, &icf_l, &other_l);
    }
    // other: inherited (bit 63), own spill (bit 62) or function bag (bit 61).
    // Bit 60 is reserved; unknown tags miss.
    ctx.current_block = other_idx;
    {
        let blk = ctx.block();
        let inherited = blk.icmp_slt(I64, &slot, "0");
        blk.cond_br(&inherited, &inh_l, &other2_l);
    }
    ctx.current_block = other2_idx;
    let index = {
        let blk = ctx.block();
        let spill_bit = blk.lshr(I64, &slot, "62");
        let is_spill = blk.icmp_ne(I64, &spill_bit, "0");
        let index = blk.and(I64, &slot, &index_mask);
        blk.cond_br(&is_spill, &spill_l, &other3_l);
        index
    };
    ctx.current_block = other3_idx;
    {
        let blk = ctx.block();
        let bag_bit = blk.lshr(I64, &slot, "61");
        let is_bag = blk.icmp_ne(I64, &bag_bit, "0");
        blk.cond_br(&is_bag, &bag_l, &other4_l);
    }
    // Every remaining tag (bit 60 is reserved) misses: an unknown entry must
    // never become an unchecked call.
    ctx.current_block = other4_idx;
    ctx.block().br(&miss_l);
    // function bag: the receiver's own-property object, then its inline slot.
    // The keyed Function ShapeId the word matched is canonical per that
    // object's key list, so the object exists; the null test is a guard.
    ctx.current_block = bag_idx;
    let bag = {
        let blk = ctx.block();
        let pp = emit_field_ptr(blk, &biased, props_offset);
        let bag = blk.load(I64, &pp);
        let has = blk.icmp_ne(I64, &bag, "0");
        blk.cond_br(&has, &bag2_l, &miss_l);
        bag
    };
    ctx.current_block = bag2_idx;
    let (bag_v, bag_end) = {
        let blk = ctx.block();
        let bptr = blk.inttoptr(I64, &bag);
        let base = blk.gep(crate::types::I8, &bptr, &[(I64, &header.to_string())]);
        let vp = blk.gep(I64, &base, &[(I64, &index)]);
        let v = blk.load(I64, &vp);
        let end = blk.label.clone();
        blk.br(&value_l);
        (v, end)
    };
    // own inline: load the slot (an untagged entry, the slot index itself).
    ctx.current_block = own_idx;
    let (inline_v, inline_end) = {
        let blk = ctx.block();
        let base = emit_field_ptr(blk, &biased, header);
        let vp = blk.gep(I64, &base, &[(I64, &slot)]);
        let v = blk.load(I64, &vp);
        let end = blk.label.clone();
        blk.br(&value_l);
        (v, end)
    };
    // ConstFn: the shape compare proves the current slot is a closure of this
    // body's info. Load that closure as the callee environment (its captures);
    // no heap-kind or info load occurs on this hit path.
    ctx.current_block = constfn_idx;
    let (constfn_handle, constfn_func, constfn_end) = {
        let blk = ctx.block();
        let base = emit_field_ptr(blk, &biased, header);
        let own_index = blk.and(I64, &slot, &index_mask);
        let vp = blk.gep(I64, &base, &[(I64, &own_index)]);
        let v = blk.load(I64, &vp);
        let ub = blk.sub(I64, &v, &(RECEIVER_BIAS as i64).to_string());
        let h = emit_handle(blk, &ub);
        let fp = blk.gep(crate::types::I8, &entry, &[(I64, &abi_code)]);
        let f = blk.load(I64, &fp);
        let end = blk.label.clone();
        blk.br(&cf_call_l);
        (h, f, end)
    };
    // Inherited ConstFn: the receiver word pins the direct holder, the
    // holder's word pins ITS shape, and that shape's lane names the body the
    // entry's code is. The holder slot is loaded as the callee environment.
    ctx.current_block = icf_idx;
    let icf_holder = {
        let blk = ctx.block();
        let hp = blk.gep(crate::types::I8, &entry, &[(I64, &abi_closure)]);
        let holder = blk.load(I64, &hp);
        let holder_ptr = blk.inttoptr(I64, &holder);
        let word = blk.load(I64, &holder_ptr);
        let wp = blk.gep(crate::types::I8, &entry, &[(I64, &abi_gen)]);
        let saved = blk.load(I64, &wp);
        let valid = blk.icmp_eq(I64, &word, &saved);
        blk.cond_br(&valid, &icf_hit_l, &miss_l);
        holder
    };
    ctx.current_block = icf_hit_idx;
    let (icf_handle, icf_func, icf_end) = {
        let blk = ctx.block();
        let holder_ptr = blk.inttoptr(I64, &icf_holder);
        let base = blk.gep(crate::types::I8, &holder_ptr, &[(I64, &header.to_string())]);
        let idx = blk.and(I64, &slot, &index_mask);
        let vp = blk.gep(I64, &base, &[(I64, &idx)]);
        let v = blk.load(I64, &vp);
        let ub = blk.sub(I64, &v, &(RECEIVER_BIAS as i64).to_string());
        let h = emit_handle(blk, &ub);
        let fp = blk.gep(crate::types::I8, &entry, &[(I64, &abi_code)]);
        let f = blk.load(I64, &fp);
        let end = blk.label.clone();
        blk.br(&cf_call_l);
        (h, f, end)
    };
    // own spill: meta -> spill buffer -> element, bounds-checked.
    ctx.current_block = spill_idx;
    let meta = {
        let blk = ctx.block();
        let mp = emit_field_ptr(blk, &biased, meta_offset);
        let meta = blk.load(I64, &mp);
        let has = blk.icmp_ne(I64, &meta, "0");
        blk.cond_br(&has, &spill2_l, &miss_l);
        meta
    };
    ctx.current_block = spill2_idx;
    let buf = {
        let blk = ctx.block();
        let mptr = blk.inttoptr(I64, &meta);
        let bp = blk.gep(crate::types::I8, &mptr, &[(I64, &spill_offset)]);
        let buf = blk.load(I64, &bp);
        let has = blk.icmp_ne(I64, &buf, "0");
        blk.cond_br(&has, &spill3_l, &miss_l);
        buf
    };
    ctx.current_block = spill3_idx;
    {
        let blk = ctx.block();
        let bptr = blk.inttoptr(I64, &buf);
        let len = blk.load(I32, &bptr);
        let len64 = blk.zext(I32, &len, I64);
        let in_range = blk.icmp_ult(I64, &index, &len64);
        blk.cond_br(&in_range, &spill4_l, &miss_l);
    }
    ctx.current_block = spill4_idx;
    let (spill_v, spill_end) = {
        let blk = ctx.block();
        let bptr = blk.inttoptr(I64, &buf);
        let elems = blk.gep(crate::types::I8, &bptr, &[(I64, &array_header)]);
        let ep = blk.gep(I64, &elems, &[(I64, &index)]);
        let v = blk.load(I64, &ep);
        let end = blk.label.clone();
        blk.br(&value_l);
        (v, end)
    };
    // The receiver's shape pins the direct holder. Its word pins the slot;
    // the slot value itself is loaded on every hit, just like an own method.
    ctx.current_block = inh_idx;
    let holder = {
        let blk = ctx.block();
        let hp = blk.gep(crate::types::I8, &entry, &[(I64, &abi_closure)]);
        let holder = blk.load(I64, &hp);
        let holder_ptr = blk.inttoptr(I64, &holder);
        let word = blk.load(I64, &holder_ptr);
        let wp = blk.gep(crate::types::I8, &entry, &[(I64, &abi_gen)]);
        let saved = blk.load(I64, &wp);
        let valid = blk.icmp_eq(I64, &word, &saved);
        blk.cond_br(&valid, &inh_value_l, &miss_l);
        holder
    };
    ctx.current_block = inh_value_idx;
    let (inh_v, inh_end) = {
        let blk = ctx.block();
        let holder_ptr = blk.inttoptr(I64, &holder);
        let base = blk.gep(crate::types::I8, &holder_ptr, &[(I64, &header.to_string())]);
        let idx = blk.and(I64, &slot, &index_mask);
        let vp = blk.gep(I64, &base, &[(I64, &idx)]);
        let v = blk.load(I64, &vp);
        let end = blk.label.clone();
        blk.br(&value_l);
        (v, end)
    };
    // value: it must hold a closure running the memoized body.
    ctx.current_block = value_idx;
    let own_ub = {
        let blk = ctx.block();
        let v = blk.phi(
            I64,
            &[
                (&inline_v, &inline_end),
                (&spill_v, &spill_end),
                (&bag_v, &bag_end),
                (&inh_v, &inh_end),
            ],
        );
        let u = blk.sub(I64, &v, &(RECEIVER_BIAS as i64).to_string());
        let heap = blk.icmp_ult(I64, &u, &(RECEIVER_SPAN as i64).to_string());
        blk.cond_br(&heap, &own_fn_l, &miss_l);
        u
    };
    ctx.current_block = own_fn_idx;
    // The plain call block's only predecessor is this own-function check.
    let (own_handle, own_func) = {
        let blk = ctx.block();
        let kp = emit_field_ptr(blk, &own_ub, kind_offset);
        let kind = blk.load(crate::types::I16, &kp);
        let kind = blk.and(crate::types::I16, &kind, &kind_mask);
        let is_closure = blk.icmp_eq(crate::types::I16, &kind, &closure_kind);
        let ip = emit_field_ptr(blk, &own_ub, info_offset);
        let info = blk.load(I64, &ip);
        let mi_p = blk.gep(crate::types::I8, &entry, &[(I64, &abi_info)]);
        let mi = blk.load(I64, &mi_p);
        let same = blk.icmp_eq(I64, &info, &mi);
        let mf_p = blk.gep(crate::types::I8, &entry, &[(I64, &abi_code)]);
        let mf = blk.load(I64, &mf_p);
        let hit = blk.and(I1, &is_closure, &same);
        let h = emit_handle(blk, &own_ub);
        let retry_idx = ctx.new_block("msite.next_way");
        let retry_l = ctx.block_label(retry_idx);
        ctx.block().cond_br(&hit, &call_l, &retry_l);
        // next way: resume the word compares after the entry that matched.
        ctx.current_block = retry_idx;
        for (i, next) in way_labels.iter().enumerate() {
            let blk = ctx.block();
            // Way i's entry, re-derived here (a gep in way i's own compare
            // block would not dominate this one).
            let way_entry = if i == 0 {
                ic.cache.clone()
            } else {
                blk.gep(
                    crate::types::I8,
                    &ic.cache,
                    &[(I64, &(i * entry_size).to_string())],
                )
            };
            let was = blk.icmp_eq(PTR, &entry, &way_entry);
            let else_l = if i + 1 < way_labels.len() {
                let idx = ctx.new_block("msite.next_way");
                (Some(idx), ctx.block_label(idx))
            } else {
                (None, miss_l.clone())
            };
            ctx.block().cond_br(&was, next, &else_l.1);
            if let Some(idx) = else_l.0 {
                ctx.current_block = idx;
            }
        }
        if way_labels.is_empty() {
            ctx.block().br(&miss_l);
        }
        (h, mf)
    };
    // call: the body directly, with the receiver as its `this` parameter.
    ctx.current_block = call_idx;
    let handle = own_handle;
    let fptr = ctx.block().inttoptr(I64, &own_func);
    let mut call_args: Vec<String> = lowered_args.to_vec();
    // Pad with `undefined` up to the arity the prime admits, so a body that
    // declares a few more parameters than this call passes is entered
    // directly (`dispatch_with_arity` pads the same way). A body declaring
    // fewer ignores the extra registers.
    let undefined = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
    let pad = crate::runtime_abi::method_site_padded_argc(lowered_args.len()) - lowered_args.len();
    call_args.extend(std::iter::repeat_n(undefined, pad));
    // The body gets the receiver as its `this` parameter.
    let recv_bits = ctx.block().bitcast_double_to_i64(recv_box);
    let hit_value = crate::expr::body_call::emit_js_body_call(
        ctx.block(),
        crate::expr::body_call::JsBody::Pointer(&fptr),
        &handle,
        &recv_bits,
        &call_args,
    );
    let hit_end = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_l);
    }

    // ConstFn call: the prime admits a ConstFn entry only for a body that
    // declares at most this call's argument count, so the call passes exactly
    // the call's arguments (no `undefined` padding).
    ctx.current_block = cf_call_idx;
    let cf_handle = ctx.block().phi(
        I64,
        &[(&constfn_handle, &constfn_end), (&icf_handle, &icf_end)],
    );
    let cf_func = ctx
        .block()
        .phi(I64, &[(&constfn_func, &constfn_end), (&icf_func, &icf_end)]);
    let cf_recv_bits = ctx.block().bitcast_double_to_i64(recv_box);
    // The entry's code is a body the compared shape names. When it is one of
    // this site's compile-time candidate bodies (the static lanes' bodies; a
    // `this` in a literal method also reaches inheriting receivers), call that
    // body DIRECTLY, so the call can be inlined; otherwise call the entry's
    // code. The ConstFn prime admits only bodies declaring at most `argc`, so
    // a candidate's own arity never needs more arguments than the call has.
    let mut cf_results: Vec<(String, String)> = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for lane in lanes {
        if seen.contains(&lane.body.as_str()) || lane.arity > lowered_args.len() {
            continue;
        }
        seen.push(&lane.body);
        let direct_idx = ctx.new_block("msite.constfn_direct");
        let next_idx = ctx.new_block("msite.constfn_body");
        let direct_l = ctx.block_label(direct_idx);
        let next_l = ctx.block_label(next_idx);
        {
            let blk = ctx.block();
            let body_addr = blk.ptrtoint(&format!("@{}", lane.body), I64);
            let same = blk.icmp_eq(I64, &cf_func, &body_addr);
            blk.cond_br(&same, &direct_l, &next_l);
        }
        ctx.current_block = direct_idx;
        let blk = ctx.block();
        let args: Vec<String> = lowered_args.iter().take(lane.arity).cloned().collect();
        let r = crate::expr::body_call::emit_js_body_call(
            blk,
            crate::expr::body_call::JsBody::Symbol(&lane.body),
            &cf_handle,
            &cf_recv_bits,
            &args,
        );
        let end = blk.label.clone();
        if !blk.is_terminated() {
            blk.br(&merge_l);
        }
        cf_results.push((r, end));
        ctx.current_block = next_idx;
    }
    let cf_fptr = ctx.block().inttoptr(I64, &cf_func);
    let cf_value = crate::expr::body_call::emit_js_body_call(
        ctx.block(),
        crate::expr::body_call::JsBody::Pointer(&cf_fptr),
        &cf_handle,
        &cf_recv_bits,
        lowered_args,
    );
    let cf_end = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_l);
    }

    // miss (heap object): prime + the universal dispatcher.
    ctx.current_block = miss_idx;
    let miss_value = ctx.block().call(
        DOUBLE,
        "js_method_site_miss",
        &[
            (PTR, &ic.slot_ref),
            (I64, feedback_site),
            (DOUBLE, recv_box),
            (I64, method_id),
            (PTR, args_ptr),
            (I64, argc),
        ],
    );
    let miss_end = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_l);
    }

    // primitive: the universal dispatcher, as without a site.
    ctx.current_block = prim_idx;
    let prim_value = ctx.block().call(
        DOUBLE,
        "js_typed_feedback_native_call_method_by_id",
        &[
            (I64, feedback_site),
            (DOUBLE, recv_box),
            (I64, method_id),
            (PTR, args_ptr),
            (I64, argc),
        ],
    );
    let prim_end = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_l);
    }

    ctx.current_block = merge_idx;
    let mut incoming: Vec<(&str, &str)> = vec![
        (&hit_value, &hit_end),
        (&cf_value, &cf_end),
        (&miss_value, &miss_end),
        (&prim_value, &prim_end),
    ];
    incoming.extend(lane_hits.iter().map(|(v, l)| (v.as_str(), l.as_str())));
    incoming.extend(cf_results.iter().map(|(v, l)| (v.as_str(), l.as_str())));
    ctx.block().phi(DOUBLE, &incoming)
}
