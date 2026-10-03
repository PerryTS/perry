//! Step 4b regions: the guard (receiver test, shape compare, store admission, prime).

use super::*;

// ---------------------------------------------------------------- lowering

/// The receiver's value in the current block. A fresh lowering of the binding
/// reads its root now, so the result names the object's CURRENT address.
pub(super) fn lower_recv(ctx: &mut FnCtx<'_>, r: Recv) -> Result<String> {
    lower_expr(ctx, &r.expr())
}

/// `bits & POINTER_MASK` — the handle, derived transparently.
pub(super) fn handle_of(ctx: &mut FnCtx<'_>, recv_box: &str) -> String {
    let bits = ctx.block().bitcast_double_to_i64(recv_box);
    ctx.block().and(I64, &bits, crate::nanbox::POINTER_MASK_I64)
}

pub(super) fn field_i16(ctx: &mut FnCtx<'_>, handle: &str, offset: i64) -> String {
    let addr = ctx.block().add(I64, handle, &offset.to_string());
    let ptr = ctx.block().inttoptr(I64, &addr);
    ctx.block().load(I16, &ptr)
}

pub(super) fn field_i32(ctx: &mut FnCtx<'_>, handle: &str, offset: i64) -> String {
    let addr = ctx.block().add(I64, handle, &offset.to_string());
    let ptr = ctx.block().inttoptr(I64, &addr);
    ctx.block().load(I32, &ptr)
}

/// DESIGN §6.5a: F-A (receiver kind) and F-B (no Array-subclass numeric
/// proof), as an i1, for a receiver whose handle is proven an object.
pub(super) fn store_admission(ctx: &mut FnCtx<'_>, handle: &str, with_kind: bool) -> String {
    // Both the learned word and the static birth word name an Ordinary
    // ShapeId. An OrdinaryNumericProof sibling cannot match either word.
    if !with_kind {
        return "true".to_string();
    }
    let reserved = field_i16(ctx, handle, -6);
    let class_id = field_i32(ctx, handle, 0);
    let biased = ctx.block().add(I32, &class_id, "2");
    let has_class = ctx.block().icmp_ugt(I32, &biased, "2");
    let admit_bits = ctx.block().and(I16, &reserved, CLASSLESS_ADMIT_MASK_I16);
    let admitted = ctx.block().icmp_eq(I16, &admit_bits, CLASSLESS_ADMIT_I16);
    let classless = ctx.block().icmp_eq(I32, &class_id, "0");
    let plain = ctx.block().and(I1, &admitted, &classless);
    let kind_ok = ctx.block().or(I1, &has_class, &plain);
    kind_ok
}

/// Emit the full guard for one receiver from the CURRENT block. Returns the
/// word (EMPTY on every failing edge) and the pass flag, both valid in the
/// block the function leaves current.
/// Bump the bounded attempt counter and ask the runtime to pack and publish
/// this receiver's word for `sid`; returns what it published (or EMPTY).
pub(super) fn emit_prime_call(
    ctx: &mut FnCtx<'_>,
    rv: &Receiver,
    sites: &Sites,
    tries: &str,
    sid: &str,
) -> String {
    let next = ctx.block().add(I32, tries, "1");
    ctx.block().store(I32, &next, &sites.tries_g);
    let is_last = ctx.block().icmp_eq(I32, &next, PRIME_ATTEMPTS);
    let last = ctx.block().zext(I1, &is_last, I32);
    let mut key_bits: Vec<String> = Vec::with_capacity(MAX_KEYS);
    for i in 0..MAX_KEYS {
        if let Some(key) = rv.keys.get(i) {
            let idx = ctx.strings.intern(key);
            let g = format!("@{}", ctx.strings.entry(idx).handle_global);
            let boxed = ctx.block().load(DOUBLE, &g);
            key_bits.push(ctx.block().bitcast_double_to_i64(&boxed));
        } else {
            key_bits.push("0".to_string());
        }
    }
    let n = rv.keys.len().to_string();
    let word_g = sites.word_g.clone();
    ctx.block().call(
        I64,
        "js_region_loop_prime",
        &[
            (crate::types::PTR, &word_g),
            (I32, sid),
            (I32, &n),
            (I64, &key_bits[0]),
            (I64, &key_bits[1]),
            (I64, &key_bits[2]),
            (I64, &key_bits[3]),
            (I64, &key_bits[4]),
            (I32, &last),
            (I32, &rv.stored_mask.to_string()),
            (I32, &rv.boxed_mask.to_string()),
            (I32, &rv.r_mask.to_string()),
        ],
    )
}

/// The guard's first half: the receiver's site globals and one load of its
/// word. Returns the word.
pub(super) fn emit_guard_word(ctx: &mut FnCtx<'_>, rv: &mut Receiver) -> (Sites, String) {
    let sites: Sites = region_guard::state_globals(ctx);
    rv.sites = Some((sites.word_g.clone(), sites.tries_g.clone()));
    note(ctx, Route::RloopGuard);
    let word = ctx.block().load_atomic_monotonic(I64, &sites.word_g, 8);
    (sites, word)
}

/// A single-receiver BODY region's guard, emitted as control flow straight
/// into its F copies: the all-inline word's match branches to `inline_l`, the
/// spill word's to `spill_l`, anything else to `fail_l`. Run every iteration,
/// so no state is merged and re-tested. A prime publishes for the NEXT
/// iteration; this one runs G (`word`, loaded before F-body was lowered, is
/// the one F-body decodes, and the prime's result is not it).
pub(super) fn emit_body_guard_direct(
    ctx: &mut FnCtx<'_>,
    rv: &Receiver,
    sites: &Sites,
    word: &str,
    entry_tests: &[u32],
    inline_l: &str,
    spill_l: &str,
    fail_l: &str,
) -> Result<()> {
    let live = ctx.block().icmp_ne(I64, word, RETIRED_WORD);
    let open = ctx.new_block("rloop.guard.open");
    let chk = ctx.new_block("rloop.guard.chk");
    let flip = ctx.new_block("rloop.guard.flip");
    let miss = ctx.new_block("rloop.guard.miss");
    let prime = ctx.new_block("rloop.guard.prime");
    let open_l = ctx.block_label(open);
    let chk_l = ctx.block_label(chk);
    let flip_l = ctx.block_label(flip);
    let miss_l = ctx.block_label(miss);
    let prime_l = ctx.block_label(prime);
    ctx.block().cond_br(&live, &open_l, fail_l);

    ctx.current_block = open;
    note(ctx, Route::RloopOpen);
    let recv_box = lower_recv(ctx, rv.recv)?;
    let bits = ctx.block().bitcast_double_to_i64(&recv_box);
    let test = crate::expr::receiver_range::emit_fused_receiver_test(ctx.block(), &bits);
    ctx.block().cond_br(&test.is_object_pointer, &chk_l, fail_l);

    ctx.current_block = chk;
    let handle = crate::expr::receiver_range::emit_handle(ctx.block(), &test.biased);
    let expected = ctx.block().trunc(I64, word, I32);
    let sid = field_i32(ctx, &handle, 4);
    let mut eq = ctx.block().icmp_eq(I32, &sid, &expected);
    if !entry_tests.is_empty() {
        let number_ok = emit_number_entry_tests(ctx, entry_tests)?;
        eq = ctx.block().and(I1, &eq, &number_ok);
    }
    let admit = if rv.has_store {
        Some(store_admission(ctx, &handle, true))
    } else {
        None
    };
    let to =
        |ctx: &mut FnCtx<'_>, cond: &str, target: &str, other: &str, admit: &Option<String>| {
            match admit {
                Some(a) => {
                    let adm = ctx.new_block("rloop.guard.admit");
                    let adm_l = ctx.block_label(adm);
                    ctx.block().cond_br(cond, &adm_l, other);
                    ctx.current_block = adm;
                    ctx.block().cond_br(a, target, fail_l);
                }
                None => ctx.block().cond_br(cond, target, other),
            }
        };
    // A word carrying VALUE_TEST_BIT enters F only once the object's R
    // slots hold Numbers; the matched ShapeId makes the word's slots its own.
    let (inline_to, spill_to) = if may_value_test(rv) {
        let mut targets = Vec::with_capacity(2);
        for target in [inline_l, spill_l] {
            let vt = ctx.new_block("rloop.guard.value");
            let test = ctx.new_block("rloop.guard.value.test");
            let vt_l = ctx.block_label(vt);
            let test_l = ctx.block_label(test);
            let saved = ctx.current_block;
            ctx.current_block = vt;
            let need = ctx.block().icmp_slt(I64, word, "0");
            ctx.block().cond_br(&need, &test_l, target);
            ctx.current_block = test;
            let ok = value_tests_on(ctx, rv, word, &handle);
            ctx.block().cond_br(&ok, target, fail_l);
            ctx.current_block = saved;
            targets.push(vt_l);
        }
        let spill_to = targets.pop().expect("two targets");
        (targets.pop().expect("two targets"), spill_to)
    } else {
        (inline_l.to_string(), spill_l.to_string())
    };
    to(ctx, &eq, &inline_to, &flip_l, &admit);

    ctx.current_block = flip;
    let exp_f = ctx.block().xor(I32, &expected, FLIP_I32);
    let eq_f = ctx.block().icmp_eq(I32, &sid, &exp_f);
    to(ctx, &eq_f, &spill_to, &miss_l, &admit);

    ctx.current_block = miss;
    let tries = ctx.block().load(I32, &sites.tries_g);
    let may = ctx.block().icmp_ult(I32, &tries, PRIME_ATTEMPTS);
    ctx.block().cond_br(&may, &prime_l, fail_l);

    ctx.current_block = prime;
    let _ = emit_prime_call(ctx, rv, sites, &tries, &sid);
    ctx.block().br(fail_l);
    Ok(())
}

/// The region word the static supplier names for this receiver (DESIGN
/// §4.1): the driver's static id of the receiver's class birth shape and its
/// keys' slots, packed exactly as the runtime packs an all-inline word
/// (`id | slot_i << (32 + SLOT_BITS * i)`). `None` when the compiler names no
/// class for the receiver, the driver assigned the class no static id, or a
/// key is not an inline slot of that shape.
///
/// Class provenance chooses the supplier when available; otherwise a class
/// hint suffices. Neither licenses a slot access: the guard compares the live
/// ShapeId against the supplier's id, and a different shape selects G.
fn static_region_word(ctx: &FnCtx<'_>, rv: &Receiver) -> Option<(u64, u32, u32, bool)> {
    // Use containment's exact class to select the supplier when available.
    // This consumes only class provenance: the compared ShapeId below remains
    // the sole authority for slot locations and Number representation.
    let proven_class = ctx.ptr_shape_region_class(&rv.recv.expr());
    let class_name = proven_class.clone().or_else(|| {
        crate::type_analysis::receiver_class_name(ctx, &rv.recv.expr()).or_else(|| {
            match rv.recv {
                Recv::Local(id) => match ctx.local_type_hint(&id)? {
                    perry_hir::types::Type::Named(name) if ctx.classes.contains_key(name) => {
                        Some(name.clone())
                    }
                    // A closed object type: the literal class its literals allocate.
                    ty => {
                        crate::stmt::element_shape_loop::anon_shape_class_for_object_type(ctx, ty)
                    }
                },
                Recv::This => None,
            }
        })
    })?;
    let keys_global = ctx.class_keys_globals.get(&class_name)?;
    let (id, slots, f64_lanes) =
        crate::codegen::static_region_slots(keys_global, &rv.keys, rv.boxed_mask)?;
    // The runtime's rule (`region_loop_pack`), decided here for the static
    // birth shape: a requested Number read on an identity F64 lane is free,
    // and one on an `Any` lane is served by a value test on the object
    // unless a bare store may write that key a non-Number, or the plan does
    // not allow value tests (`vt_mask`: a guard re-run every iteration).
    let tested = rv.r_mask & rv.vt_mask & !f64_lanes & !rv.boxed_mask;
    let mut word = u64::from(id);
    for (i, slot) in slots.iter().enumerate() {
        word |= u64::from(*slot) << (32 + SLOT_BITS * i as u32);
    }
    if tested != 0 {
        word |= VALUE_TEST_BIT;
    }
    Some((word, f64_lanes | tested, tested, proven_class.is_some()))
}

/// Bit 63 of a region word, `REGION_LOOP_WORD_VALUE_TEST`: a Number read
/// (R) of this word sits on a lane that does not guarantee a Number for every
/// carrier, so the guard tests the R slots' values on the object.
pub(super) const VALUE_TEST_BIT: u64 = 1 << 63;

/// The R slots' value test: each slot `rv.vt_mask` names (decoded from
/// `word`) of the object `handle` holds a raw canonical Number now — the
/// strict test the number entry tests use (no tag, no INT32 box, no mirror
/// of the tag band). Only valid where `word` matched the object's ShapeId.
fn value_tests_on(ctx: &mut FnCtx<'_>, rv: &Receiver, word: &str, handle: &str) -> String {
    let mut ok = "true".to_string();
    for i in 0..rv.keys.len() {
        if (rv.r_mask & rv.vt_mask) & (1 << i) == 0 {
            continue;
        }
        let shift = (32 + SLOT_BITS * i as u32).to_string();
        let s = ctx.block().lshr(I64, word, &shift);
        let slot = ctx.block().and(I64, &s, "63");
        let p = super::bare::slot_ptr(ctx, handle, &slot);
        let v = ctx.block().load(DOUBLE, &p);
        let number = crate::stmt::loops::emit_js_value_is_number(ctx, &v);
        ok = ctx.block().and(I1, &ok, &number);
    }
    ok
}

/// Does `rv` have an R slot its word may ask the guard to value-test?
pub(super) fn may_value_test(rv: &Receiver) -> bool {
    rv.r_mask & rv.vt_mask != 0
}

/// A loop region's re-check compare for one receiver: the object's ShapeId
/// `sid` against the expected id `exp`, plus the R slots' value test when the
/// word asks for one (JS may have written those slots since the guard).
///
/// The re-check sits on the hot path of every iteration that may run JS, so
/// a LEARNED word without [`VALUE_TEST_BIT`] must cost exactly the plain
/// compare. Its bit is known only at run time, so the compare is against a
/// loop-invariant id that a word WITH the bit replaces by `EMPTY` (which no
/// object carries): such a word fails the re-check and the loop continues in
/// G (today's code), never in an F whose R facts were not re-proven. A static
/// word knows its bit when compiled: one with the bit re-tests the values,
/// one without pays nothing.
pub(super) fn emit_recheck_eq(
    ctx: &mut FnCtx<'_>,
    rv: &Receiver,
    sid: &str,
    exp: &str,
) -> Result<String> {
    if !may_value_test(rv) {
        return Ok(ctx.block().icmp_eq(I32, sid, exp));
    }
    if rv.expected_shape.is_some() {
        let eq = ctx.block().icmp_eq(I32, sid, exp);
        let word = rv.word.clone();
        return emit_value_tests(ctx, rv, &word, &eq);
    }
    let need = ctx.block().icmp_slt(I64, &rv.word, "0");
    let exp_fast = ctx.block().select(I1, &need, I32, "-1", exp);
    Ok(ctx.block().icmp_eq(I32, sid, &exp_fast))
}

/// `pass` and, when it holds and `word` carries [`VALUE_TEST_BIT`], the R
/// slots' value test on the receiver (re-derived from its binding: `pass`
/// proves it is an object whose ShapeId `word` names). A word without the bit
/// costs one sign test; a receiver with no testable R costs nothing.
pub(super) fn emit_value_tests(
    ctx: &mut FnCtx<'_>,
    rv: &Receiver,
    word: &str,
    pass: &str,
) -> Result<String> {
    if !may_value_test(rv) {
        return Ok(pass.to_string());
    }
    // Branch on `pass` first, then on the word's sign bit: a failing compare
    // keeps its one branch, and a word without the bit pays one sign test.
    let chk = ctx.new_block("rloop.guard.value.need");
    let vt = ctx.new_block("rloop.guard.value");
    let join = ctx.new_block("rloop.guard.value.join");
    let chk_l = ctx.block_label(chk);
    let vt_l = ctx.block_label(vt);
    let join_l = ctx.block_label(join);
    let from = ctx.block().label.clone();
    ctx.block().cond_br(pass, &chk_l, &join_l);
    ctx.current_block = chk;
    let need = ctx.block().icmp_slt(I64, word, "0");
    ctx.block().cond_br(&need, &vt_l, &join_l);
    ctx.current_block = vt;
    let recv_box = lower_recv(ctx, rv.recv)?;
    let handle = handle_of(ctx, &recv_box);
    let ok = value_tests_on(ctx, rv, word, &handle);
    let vt_end = ctx.block().label.clone();
    ctx.block().br(&join_l);
    ctx.current_block = join;
    Ok(ctx.block().phi(
        I1,
        &[
            ("false", from.as_str()),
            ("true", chk_l.as_str()),
            (&ok, vt_end.as_str()),
        ],
    ))
}

/// A guard whose receiver the compiler names (DESIGN §4.1, static-exclusive):
/// test the receiver and compare its ShapeId against the static id as an
/// immediate (plus the store admission when the region stores). The region
/// word is the constant `word`, so the slots fold to displacements; there is
/// no word load and no prime. Any miss selects the generic copy.
fn emit_static_guard(
    ctx: &mut FnCtx<'_>,
    rv: &mut Receiver,
    word: u64,
) -> Result<(String, String, String)> {
    note(ctx, Route::RloopGuard);
    let chk = ctx.new_block("rloop.guard.static");
    let hit = ctx.new_block("rloop.guard.static.hit");
    let join = ctx.new_block("rloop.guard.join");
    let chk_l = ctx.block_label(chk);
    let hit_l = ctx.block_label(hit);
    let join_l = ctx.block_label(join);
    let recv_box = lower_recv(ctx, rv.recv)?;
    let bits = ctx.block().bitcast_double_to_i64(&recv_box);
    let test = crate::expr::receiver_range::emit_fused_receiver_test(ctx.block(), &bits);
    let entry_l = ctx.block().label.clone();
    ctx.block()
        .cond_br(&test.is_object_pointer, &chk_l, &join_l);

    ctx.current_block = chk;
    let handle = crate::expr::receiver_range::emit_handle(ctx.block(), &test.biased);
    let sid = field_i32(ctx, &handle, 4);
    let expected = (word as u32).to_string();
    // Region slots remain identical. Refuse a raw write to any CF lane;
    // other completed shapes retain the same numeric/boxed slot facts.
    let written_slots: Vec<u32> = if rv.has_store {
        (0..rv.keys.len())
            .map(|i| ((word >> (32 + SLOT_BITS * i as u32)) & ((1 << SLOT_BITS) - 1)) as u32)
            .collect()
    } else {
        Vec::new()
    };
    let eq =
        crate::typed_shape::emit_compatible_shape_eq(ctx.block(), &sid, &expected, &written_slots);
    let mut miss_edges = vec![entry_l];
    if rv.has_store {
        let admit = store_admission(ctx, &handle, true);
        let adm = ctx.new_block("rloop.guard.static.admit");
        let adm_l = ctx.block_label(adm);
        miss_edges.push(ctx.block().label.clone());
        ctx.block().cond_br(&eq, &adm_l, &join_l);
        ctx.current_block = adm;
        miss_edges.push(adm_l);
        ctx.block().cond_br(&admit, &hit_l, &join_l);
    } else {
        miss_edges.push(ctx.block().label.clone());
        ctx.block().cond_br(&eq, &hit_l, &join_l);
    }
    ctx.current_block = hit;
    note(ctx, Route::RloopStatic);
    let hit_end = ctx.block().label.clone();
    ctx.block().br(&join_l);

    ctx.current_block = join;
    let mut edges: Vec<(&str, &str)> = miss_edges.iter().map(|l| ("false", l.as_str())).collect();
    edges.push(("true", hit_end.as_str()));
    let pass = ctx.block().phi(I1, &edges);
    // Re-entry must compare the accepted header, which may be a compatible
    // completed ConstFn shape. Keep the packed slot word constant so field
    // displacements still fold; the extra value is a pointer-free ShapeId.
    let mut shape_edges: Vec<(&str, &str)> = miss_edges.iter().map(|l| ("0", l.as_str())).collect();
    shape_edges.push((sid.as_str(), hit_end.as_str()));
    rv.expected_shape = Some(ctx.block().phi(I32, &shape_edges));
    Ok(((word as i64).to_string(), pass, "false".to_string()))
}

/// Whether `rv`'s guard takes the static supplier (DESIGN §4.1). Every guard
/// construction must ask this first: the static supplier is exclusive, so a
/// guard that names a class must never fall back to a learned word.
pub(super) fn has_static_supplier(ctx: &FnCtx<'_>, rv: &Receiver) -> bool {
    static_region_word(ctx, rv).is_some()
}

pub(super) fn emit_guard(
    ctx: &mut FnCtx<'_>,
    rv: &mut Receiver,
) -> Result<(String, String, String)> {
    // A receiver whose class the compiler names takes its guard's ShapeId
    // from the driver's static id (DESIGN §4.1): no loaded supplier.
    if let Some((w, r_mask, tested, uses_ptr_shape_class)) = static_region_word(ctx, rv) {
        rv.r_mask = r_mask;
        rv.vt_mask = tested;
        rv.uses_ptr_shape_class = uses_ptr_shape_class;
        return emit_static_guard(ctx, rv, w);
    }
    // A learned word may carry VALUE_TEST_BIT for any R key: its guard must
    // honour the bit whatever the plan allowed a static word.
    rv.vt_mask = rv.r_mask;
    // A retired region (every bounded prime refused) is decided by the word
    // alone: one load and one compare, before the receiver is even tested.
    let (sites, word) = emit_guard_word(ctx, rv);
    let live = ctx.block().icmp_ne(I64, &word, RETIRED_WORD);
    let open = ctx.new_block("rloop.guard.open");
    let chk = ctx.new_block("rloop.guard.chk");
    let flip = ctx.new_block("rloop.guard.flip");
    let ok_i = ctx.new_block("rloop.guard.inline");
    let ok_s = ctx.new_block("rloop.guard.spill");
    let miss = ctx.new_block("rloop.guard.miss");
    let prime = ctx.new_block("rloop.guard.prime");
    let join = ctx.new_block("rloop.guard.join");
    let open_l = ctx.block_label(open);
    let chk_l = ctx.block_label(chk);
    let flip_l = ctx.block_label(flip);
    let ok_i_l = ctx.block_label(ok_i);
    let ok_s_l = ctx.block_label(ok_s);
    let miss_l = ctx.block_label(miss);
    let prime_l = ctx.block_label(prime);
    let join_l = ctx.block_label(join);
    let retired_l = ctx.block().label.clone();
    ctx.block().cond_br(&live, &open_l, &join_l);

    ctx.current_block = open;
    note(ctx, Route::RloopOpen);
    let recv_box = lower_recv(ctx, rv.recv)?;
    let bits = ctx.block().bitcast_double_to_i64(&recv_box);
    let test = crate::expr::receiver_range::emit_fused_receiver_test(ctx.block(), &bits);
    let entry_l = ctx.block().label.clone();
    ctx.block()
        .cond_br(&test.is_object_pointer, &chk_l, &join_l);

    ctx.current_block = chk;
    let handle = crate::expr::receiver_range::emit_handle(ctx.block(), &test.biased);
    let expected = ctx.block().trunc(I64, &word, I32);
    let sid = field_i32(ctx, &handle, 4);
    let eq = ctx.block().icmp_eq(I32, &sid, &expected);
    let admit = if rv.has_store {
        Some(store_admission(ctx, &handle, true))
    } else {
        None
    };
    // Every edge into the join carries a CONSTANT state (0 refused, 1 the
    // all-inline word, 2 the spill word) except the cold prime's, so the
    // per-iteration decision a body region makes on it threads straight from
    // the compare to its F copy.
    let chk_end = ctx.block().label.clone();
    let mut refused_edges: Vec<String> = Vec::new();
    match &admit {
        Some(a) => {
            let adm = ctx.new_block("rloop.guard.admit");
            let adm_l = ctx.block_label(adm);
            ctx.block().cond_br(&eq, &adm_l, &flip_l);
            ctx.current_block = adm;
            ctx.block().cond_br(a, &ok_i_l, &join_l);
            refused_edges.push(adm_l);
        }
        None => ctx.block().cond_br(&eq, &ok_i_l, &flip_l),
    }
    let _ = chk_end;

    // Not the all-inline word: is it this shape's SPILL word?
    ctx.current_block = flip;
    let exp_f = ctx.block().xor(I32, &expected, FLIP_I32);
    let eq_f = ctx.block().icmp_eq(I32, &sid, &exp_f);
    match &admit {
        Some(a) => {
            let adm = ctx.new_block("rloop.guard.admit");
            let adm_l = ctx.block_label(adm);
            ctx.block().cond_br(&eq_f, &adm_l, &miss_l);
            ctx.current_block = adm;
            ctx.block().cond_br(a, &ok_s_l, &join_l);
            refused_edges.push(adm_l);
        }
        None => ctx.block().cond_br(&eq_f, &ok_s_l, &miss_l),
    }
    ctx.current_block = ok_i;
    ctx.block().br(&join_l);
    ctx.current_block = ok_s;
    ctx.block().br(&join_l);

    // The shape did not match: prime (bounded for the process), then compare
    // again against what the runtime published.
    ctx.current_block = miss;
    let tries = ctx.block().load(I32, &sites.tries_g);
    let may = ctx.block().icmp_ult(I32, &tries, PRIME_ATTEMPTS);
    ctx.block().cond_br(&may, &prime_l, &join_l);

    ctx.current_block = prime;
    let primed = emit_prime_call(ctx, rv, &sites, &tries, &sid);
    // The prime is not a collection point for the receiver's facts: it reads
    // shapes and key bytes only. The object may still have moved in theory
    // (unclassified callee), so the handle is re-derived for the admission.
    let recv_box2 = lower_recv(ctx, rv.recv)?;
    let handle2 = handle_of(ctx, &recv_box2);
    let sid2 = field_i32(ctx, &handle2, 4);
    let exp2 = ctx.block().trunc(I64, &primed, I32);
    let eq2i = ctx.block().icmp_eq(I32, &sid2, &exp2);
    let exp2f = ctx.block().xor(I32, &exp2, FLIP_I32);
    let spill2 = ctx.block().icmp_eq(I32, &sid2, &exp2f);
    let eq2 = ctx.block().or(I1, &eq2i, &spill2);
    let admit2 = if rv.has_store {
        store_admission(ctx, &handle2, true)
    } else {
        "true".to_string()
    };
    let pass2 = ctx.block().and(I1, &eq2, &admit2);
    let two_or_one = ctx.block().select(I1, &spill2, I8, "2", "1");
    let state2 = ctx.block().select(I1, &pass2, I8, &two_or_one, "0");
    let prime_end = ctx.block().label.clone();
    ctx.block().br(&join_l);

    ctx.current_block = join;
    let mut words: Vec<(String, String)> = vec![
        (EMPTY_WORD.to_string(), retired_l.clone()),
        (EMPTY_WORD.to_string(), entry_l.clone()),
        (word.clone(), ok_i_l.clone()),
        (word.clone(), ok_s_l.clone()),
        (EMPTY_WORD.to_string(), miss_l.clone()),
        (primed.clone(), prime_end.clone()),
    ];
    let mut states: Vec<(String, String)> = vec![
        ("0".to_string(), retired_l.clone()),
        ("0".to_string(), entry_l.clone()),
        ("1".to_string(), ok_i_l.clone()),
        ("2".to_string(), ok_s_l.clone()),
        ("0".to_string(), miss_l.clone()),
        (state2.clone(), prime_end.clone()),
    ];
    for e in &refused_edges {
        words.push((EMPTY_WORD.to_string(), e.clone()));
        states.push(("0".to_string(), e.clone()));
    }
    let w: Vec<(&str, &str)> = words
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    let word_out = ctx.block().phi(I64, &w);
    let st: Vec<(&str, &str)> = states
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    let state = ctx.block().phi(I8, &st);
    let pass_out = ctx.block().icmp_ne(I8, &state, "0");
    let spill_out = ctx.block().icmp_eq(I8, &state, "2");
    Ok((word_out, pass_out, spill_out))
}

pub(super) fn decode_slots(ctx: &mut FnCtx<'_>, rv: &mut Receiver, word: &str) {
    rv.word = word.to_string();
    rv.slots = (0..rv.keys.len())
        .map(|i| {
            let shift = (32 + SLOT_BITS * i as u32).to_string();
            let s = ctx.block().lshr(I64, word, &shift);
            ctx.block().and(I64, &s, "63")
        })
        .collect();
}
