//! The method-call site: `recv.m(args)` as ONE path — the property read's
//! receiver test and shape compare, one slot load (or the memoized inherited
//! closure), then a direct call of the method body with `recv` as `this`.
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
//!   s < 0 (inherited):  PERRY_PROTO_VALIDITY == site.gen     else MISS
//!                       h = site.closure ; f = site.func
//!   own:  v = load [recv + HDR + 8*s] ; v is a heap pointer  else MISS
//!   fn:   v = load [[recv + PROPS] + HDR + 8*s]  (bit 61: a function's
//!         own-property object; same checks as own)
//!         [v-8] & 0x80FF == CLOSURE ; [v+8] == site.func      else NEXT WAY
//!         h = handle(v) ; f = site.func
//!   CALL: this = recv ; r = f(h, args...) ; restore this
//!   MISS: js_method_site_miss(slot, feedback_site, recv, method_id, args)
//!   PRIMITIVE: js_typed_feedback_native_call_method_by_id(feedback_site, recv, method_id, args)
//! ```
//!
//! The miss primes the entry when its facts hold and otherwise performs the
//! universal by-name dispatch, so every receiver the memo cannot describe —
//! primitives, native handles, Proxy, accessors, bound functions — keeps
//! exactly the behaviour it had.

use super::FnCtx;
use crate::types::{DOUBLE, I1, I32, I64, PTR};

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
    if !(triple.starts_with("x86_64") || triple.starts_with("aarch64")) || triple.contains("32") {
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
pub(crate) fn emit_method_site(
    ctx: &mut FnCtx<'_>,
    recv_box: &str,
    lowered_args: &[String],
    feedback_site: &str,
    method_id: &str,
    args_ptr: &str,
    argc: &str,
) -> String {
    use crate::expr::receiver_range::{
        emit_field_ptr, emit_fused_receiver_test, emit_handle, RECEIVER_BIAS, RECEIVER_SPAN,
    };
    let abi_word = crate::runtime_abi::METHOD_SITE_WORD_OFFSET.to_string();
    let abi_slot = crate::runtime_abi::METHOD_SITE_SLOT_OFFSET.to_string();
    let abi_func = crate::runtime_abi::METHOD_SITE_FUNC_OFFSET.to_string();
    let abi_closure = crate::runtime_abi::METHOD_SITE_CLOSURE_OFFSET.to_string();
    let abi_gen = crate::runtime_abi::METHOD_SITE_GEN_OFFSET.to_string();
    let ways = crate::runtime_abi::METHOD_SITE_WAYS;
    let meta_offset = crate::target_layout::object_meta_slot_offset_bytes(ctx.target_triple) as i64;
    let spill_offset = crate::runtime_abi::OBJECT_META_SPILL_OFFSET.to_string();
    let array_header = crate::runtime_abi::ARRAY_HEADER_SIZE.to_string();
    let index_mask = crate::runtime_abi::METHOD_SITE_INDEX_MASK.to_string();
    let entry_size = crate::runtime_abi::METHOD_SITE_ENTRY_SIZE;
    let header = crate::target_layout::object_header_size_bytes(ctx.target_triple) as i64;
    // `ClosureHeader` (64-bit only here): the code pointer, and the GcHeader
    // (type, flags) half-word in front of the payload that makes a cell a
    // live function object: type `GC_TYPE_CLOSURE` and not a forwarded stub
    // (`closure::is_closure_ptr`'s kind term; there is no payload magic).
    let func_offset = crate::runtime_abi::CLOSURE_FUNC_PTR_OFFSET as i64;
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
    let value_idx = ctx.new_block("msite.value");
    let inh_idx = ctx.new_block("msite.inherited");
    let call_idx = ctx.new_block("msite.call");
    let miss_idx = ctx.new_block("msite.miss");
    let merge_idx = ctx.new_block("msite.merge");
    let deref_l = ctx.block_label(deref_idx);
    let kind_l = ctx.block_label(kind_idx);
    let own_l = ctx.block_label(own_idx);
    let own_fn_l = ctx.block_label(own_fn_idx);
    let value_l = ctx.block_label(value_idx);
    let inh_l = ctx.block_label(inh_idx);
    let call_l = ctx.block_label(call_idx);
    let miss_l = ctx.block_label(miss_idx);
    let merge_l = ctx.block_label(merge_idx);

    // Entry: the fused receiver test decides the RECEIVER KIND. A primitive
    // (a string, a number, a boolean, undefined, ...) is not the site's: it
    // takes the universal dispatcher directly, with its string and primitive
    // arms, exactly as without a site. A heap object takes the site: its memo
    // if the site has one, else the miss, which primes it.
    let ic = crate::expr::emit_inline_cache_slot(ctx, &cache_name);
    let prim_idx = ctx.new_block("msite.primitive");
    let object_idx = ctx.new_block("msite.object");
    let prim_l = ctx.block_label(prim_idx);
    let object_l = ctx.block_label(object_idx);
    let biased = {
        let blk = ctx.block();
        let bits = blk.bitcast_double_to_i64(recv_box);
        let fused = emit_fused_receiver_test(blk, &bits);
        blk.cond_br(&fused.is_object_pointer, &object_l, &prim_l);
        fused.biased
    };
    ctx.current_block = object_idx;
    ctx.block().cond_br(&ic.present, &deref_l, &miss_l);

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
    // kind: an own inline slot (top two bits clear), else inherited (bit 63)
    // or an own spill slot (bit 62).
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
    let bag_idx = ctx.new_block("msite.fn_bag");
    let bag2_idx = ctx.new_block("msite.fn_bag_load");
    let spill_idx = ctx.new_block("msite.spill");
    let spill2_idx = ctx.new_block("msite.spill_buf");
    let spill3_idx = ctx.new_block("msite.spill_check");
    let spill4_idx = ctx.new_block("msite.spill_load");
    let other_l = ctx.block_label(other_idx);
    let other2_l = ctx.block_label(other2_idx);
    let other3_l = ctx.block_label(other3_idx);
    let bag_l = ctx.block_label(bag_idx);
    let bag2_l = ctx.block_label(bag2_idx);
    let spill_l = ctx.block_label(spill_idx);
    let spill2_l = ctx.block_label(spill2_idx);
    let spill3_l = ctx.block_label(spill3_idx);
    let spill4_l = ctx.block_label(spill4_idx);
    let slot = {
        let blk = ctx.block();
        let sp = blk.gep(crate::types::I8, &entry, &[(I64, &abi_slot)]);
        let s = blk.load(I64, &sp);
        let top = blk.lshr(I64, &s, "61");
        let tagged = blk.icmp_ne(I64, &top, "0");
        blk.cond_br(&tagged, &other_l, &own_l);
        s
    };
    // other: inherited (bit 63), own spill (bit 62) or function bag (bit 61);
    // any other kind bit is not one this site knows, and misses.
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
        blk.cond_br(&is_bag, &bag_l, &miss_l);
    }
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
    // own inline: load the slot.
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
            ],
        );
        let u = blk.sub(I64, &v, &(RECEIVER_BIAS as i64).to_string());
        let heap = blk.icmp_ult(I64, &u, &(RECEIVER_SPAN as i64).to_string());
        blk.cond_br(&heap, &own_fn_l, &miss_l);
        u
    };
    ctx.current_block = own_fn_idx;
    let (own_handle, own_func, own_end) = {
        let blk = ctx.block();
        let kp = emit_field_ptr(blk, &own_ub, kind_offset);
        let kind = blk.load(crate::types::I16, &kp);
        let kind = blk.and(crate::types::I16, &kind, &kind_mask);
        let is_closure = blk.icmp_eq(crate::types::I16, &kind, &closure_kind);
        let fpp = emit_field_ptr(blk, &own_ub, func_offset);
        let fp = blk.load(I64, &fpp);
        let mf_p = blk.gep(crate::types::I8, &entry, &[(I64, &abi_func)]);
        let mf = blk.load(I64, &mf_p);
        let same = blk.icmp_eq(I64, &fp, &mf);
        let hit = blk.and(I1, &is_closure, &same);
        let h = emit_handle(blk, &own_ub);
        let end = blk.label.clone();
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
        (h, mf, end)
    };
    // inherited: the memoized closure, valid while the validity word holds.
    ctx.current_block = inh_idx;
    let (inh_handle, inh_func, inh_end) = {
        let blk = ctx.block();
        let g = blk.load(I64, "@PERRY_PROTO_VALIDITY");
        let mg_p = blk.gep(crate::types::I8, &entry, &[(I64, &abi_gen)]);
        let mg = blk.load(I64, &mg_p);
        let valid = blk.icmp_eq(I64, &g, &mg);
        let hp = blk.gep(crate::types::I8, &entry, &[(I64, &abi_closure)]);
        let h = blk.load(I64, &hp);
        let fp_p = blk.gep(crate::types::I8, &entry, &[(I64, &abi_func)]);
        let f = blk.load(I64, &fp_p);
        let end = blk.label.clone();
        blk.cond_br(&valid, &call_l, &miss_l);
        (h, f, end)
    };
    // call: bind `this`, call the body directly, restore.
    ctx.current_block = call_idx;
    let handle = ctx
        .block()
        .phi(I64, &[(&own_handle, &own_end), (&inh_handle, &inh_end)]);
    let func = ctx
        .block()
        .phi(I64, &[(&own_func, &own_end), (&inh_func, &inh_end)]);
    let fptr = ctx.block().inttoptr(I64, &func);
    let cell = crate::rooting::implicit_this_cell_ptr(ctx);
    let saved = match &cell {
        Some(cell) => crate::rooting::implicit_this_save_at(ctx, cell, recv_box),
        None => crate::rooting::implicit_this_save(ctx, recv_box),
    };
    let mut call_args: Vec<(crate::types::LlvmType, &str)> =
        Vec::with_capacity(lowered_args.len() + 1);
    call_args.push((I64, &handle));
    call_args.extend(lowered_args.iter().map(|a| (DOUBLE, a.as_str())));
    // Pad with `undefined` up to the arity the prime admits, so a body that
    // declares a few more parameters than this call passes is entered
    // directly (`dispatch_with_arity` pads the same way). A body declaring
    // fewer ignores the extra registers.
    let undefined = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
    let pad = crate::runtime_abi::method_site_padded_argc(lowered_args.len()) - lowered_args.len();
    for _ in 0..pad {
        call_args.push((DOUBLE, undefined.as_str()));
    }
    let hit_value = ctx.block().call_indirect(DOUBLE, &fptr, &call_args);
    match &cell {
        Some(cell) => crate::rooting::implicit_this_restore_at(ctx, cell, saved),
        None => crate::rooting::implicit_this_restore(ctx, saved),
    }
    let hit_end = ctx.block().label.clone();
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
    ctx.block().phi(
        DOUBLE,
        &[
            (&hit_value, &hit_end),
            (&miss_value, &miss_end),
            (&prim_value, &prim_end),
        ],
    )
}
