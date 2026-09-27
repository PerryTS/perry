//! The method-call site: `recv.m(args)` as ONE path — the property read's
//! receiver test and shape compare, one slot load (or the memoized inherited
//! closure), then a direct call of the method body with `recv` as `this`.
//!
//! The site's memo is a runtime `MethodSite`
//! (`perry-runtime/src/object/method_site.rs`, which states what an entry
//! claims and why it cannot go stale). Emitted:
//!
//! ```text
//!   t   = bits - RECEIVER_BIAS ; t <u SPAN && site != null   else MISS
//!   w   = load [recv]           ; (class_id | ShapeId)
//!         w == site.word                                     else MISS
//!   s   = site.slot
//!   s < 0 (inherited):  PERRY_PROTO_VALIDITY == site.gen     else MISS
//!                       h = site.closure ; f = site.func
//!   own:  v = load [recv + HDR + 8*s] ; v is a heap pointer  else MISS
//!         [v+12] == CLOSURE_MAGIC ; [v] == site.func          else NEXT WAY
//!         h = handle(v) ; f = site.func
//!   CALL: this = recv ; r = f(h, args...) ; restore this
//!   MISS: js_method_site_miss(slot, feedback_site, recv, method_id, args)
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
    // A String.prototype method name keeps the dispatcher: its receiver is
    // most often a primitive string, which the site cannot describe (it would
    // only add a miss call in front of the dispatcher's string arm), and the
    // tag-guarded String lowering's non-string arm and the invalid-arity
    // fallback must stay the plain universal dispatch.
    if crate::lower_string_method::is_known_string_method_name(property) {
        return false;
    }
    // Object.prototype own methods (hasOwnProperty, valueOf, ...)
    // resolve to builtins the site never memoizes, so it would only add a miss
    // call in front of the dispatcher.
    if crate::lower_call::property_get::is_inherited_object_prototype_method(property) {
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
    // `ClosureHeader` (64-bit only here): the code pointer and the type tag
    // that identifies a closure (`closure::is_closure_ptr`'s selective term).
    let func_offset = crate::runtime_abi::CLOSURE_FUNC_PTR_OFFSET as i64;
    let tag_offset = crate::runtime_abi::CLOSURE_TYPE_TAG_OFFSET as i64;
    debug_assert_eq!(
        tag_offset as u64,
        crate::target_layout::closure_type_tag_offset_bytes(ctx.target_triple)
    );
    let closure_magic = crate::runtime_abi::CLOSURE_MAGIC.to_string();

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

    // Entry: the fused receiver test, folded with "the site has a memo".
    let ic = crate::expr::emit_inline_cache_slot(ctx, &cache_name);
    let (biased, ok) = {
        let blk = ctx.block();
        let bits = blk.bitcast_double_to_i64(recv_box);
        let fused = emit_fused_receiver_test(blk, &bits);
        let ok = blk.and(I1, &fused.is_object_pointer, &ic.present);
        (fused.biased, ok)
    };
    ctx.block().cond_br(&ok, &deref_l, &miss_l);

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
    let spill_idx = ctx.new_block("msite.spill");
    let spill2_idx = ctx.new_block("msite.spill_buf");
    let spill3_idx = ctx.new_block("msite.spill_check");
    let spill4_idx = ctx.new_block("msite.spill_load");
    let other_l = ctx.block_label(other_idx);
    let other2_l = ctx.block_label(other2_idx);
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
    // other: inherited (bit 63) or own spill (bit 62); any other kind bit is
    // not one this site knows, and misses.
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
        blk.cond_br(&is_spill, &spill_l, &miss_l);
        index
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
        let v = blk.phi(I64, &[(&inline_v, &inline_end), (&spill_v, &spill_end)]);
        let u = blk.sub(I64, &v, &(RECEIVER_BIAS as i64).to_string());
        let heap = blk.icmp_ult(I64, &u, &(RECEIVER_SPAN as i64).to_string());
        blk.cond_br(&heap, &own_fn_l, &miss_l);
        u
    };
    ctx.current_block = own_fn_idx;
    let (own_handle, own_func, own_end) = {
        let blk = ctx.block();
        let tp = emit_field_ptr(blk, &own_ub, tag_offset);
        let tag = blk.load(I32, &tp);
        let is_closure = blk.icmp_eq(I32, &tag, &closure_magic);
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

    // miss: prime + the universal dispatcher.
    ctx.current_block = miss_idx;
    let (args_ptr, argc) = if lowered_args.is_empty() {
        ("null".to_string(), "0".to_string())
    } else {
        let n = lowered_args.len();
        let buf = ctx.func.alloca_entry_array(DOUBLE, n);
        let blk = ctx.block();
        for (i, value) in lowered_args.iter().enumerate() {
            let p = blk.gep(DOUBLE, &buf, &[(I64, &i.to_string())]);
            blk.store(DOUBLE, value, &p);
        }
        (buf, n.to_string())
    };
    let miss_value = ctx.block().call(
        DOUBLE,
        "js_method_site_miss",
        &[
            (PTR, &ic.slot_ref),
            (I64, feedback_site),
            (DOUBLE, recv_box),
            (I64, method_id),
            (PTR, &args_ptr),
            (I64, &argc),
        ],
    );
    let miss_end = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_l);
    }

    ctx.current_block = merge_idx;
    ctx.block()
        .phi(DOUBLE, &[(&hit_value, &hit_end), (&miss_value, &miss_end)])
}
