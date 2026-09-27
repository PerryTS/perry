//! The two slow exits of the emitted generic property-get tower.
//!
//! # Why this exists
//!
//! `lower_generic_property_get` used to emit ~37 basic blocks and 6-7 runtime
//! call sites per untyped `obj.prop`: an SSO arm, an INT32 class-ref arm, a
//! nullish-throw arm, a non-object-receiver arm, an overflow-slot load, a
//! deleted-slot miss, two Array-subclass named-prefix ladders, and the
//! miss+prime. Every one of those calls is a statepoint, so both the emitted
//! `.text` and `.perry_gcmap` scale with call sites x live GC values — on
//! @babel/parser that tower was 29% of all emitted IR over 6,487 sites.
//!
//! The inline *hit* is worth its bytes; the arms around it are not. These two
//! entries are what the site branches to instead of each of them, reproducing
//! the arms they replaced in the same ORDER and with the same cache-priming
//! decisions. The emitted site keeps, byte for byte, the receiver-tag test, the
//! small-handle test, the packed header kind/descriptor test, the packed-MRU
//! compare, the overflow-bit test, the inline field load with its hole check,
//! and the polymorphic ways.
//!
//! # Why TWO exits and not one
//!
//! The first cut had one entry taking the unmasked NaN-box, reached from every
//! guard failure including the receiver-tag test. It cost **+4.00 instructions
//! on every property-read HIT**, measured on a monomorphic 10M-read loop
//! (1,231,521,261 -> 1,271,522,669 retired instructions, +3.2%), and the
//! disassembly says exactly why: with the tag test and the small-handle test
//! failing to the SAME block, LLVM's SimplifyCFG folds them into one flat
//! predicate —
//!
//! ```text
//!   cmp …; sete %dl; cmp …; setae %dil; test %dil,%dl; je   (6 instructions)
//! ```
//!
//! — where the branchy chain was `cmp; jne; cmp; ja` (4). That is #7883's
//! finding re-introduced from the other side: there codegen flattened the
//! guards, here the optimiser does it because the shared exit made the two
//! branches congruent. Giving the tag test its own callee restores the chain
//! (and, as a bonus, lets the unmasked `obj_bits` die in the entry block
//! instead of staying live across the whole hit path for a call it might make).
//!
//! The split is along the one line that matters: [`js_object_get_field_ic_nonptr`]
//! serves receivers that are NOT heap pointers (and needs the tag, not the
//! cache), [`js_object_get_field_ic_slow`] serves the ones that are (and needs
//! the cache, not the tag). Two call sites per site instead of six.
//!
//! # The arms, in the order the emitted tower took them
//!
//! 1. Receiver-tag routing ([`js_object_get_field_ic_nonptr`]), exactly as
//!    `js_object_get_field_ic` does it (SSO -> the SSO-aware by-name helper;
//!    INT32 class ref -> the feedback-wrapped by-name helper; nullish ->
//!    `TypeError`; any other non-pointer -> the by-name helper, which still
//!    resolves typed shapes such as Date `.constructor`).
//! 2. A packed-MRU token HIT that the emitted hit path declined:
//!    * the slot carries `IC_SLOT_OVERFLOW_BIT` — the field lives in the spill
//!      buffer, so the inline `obj + header + slot*8` arithmetic must not run
//!      on it. This is `js_object_get_field_ic_overflow_load`'s body, including
//!      its fallback to the miss handler **without** the packed republication
//!      (that helper called the three-argument `js_object_get_field_ic_miss`,
//!      and re-publishing the packed word here would change which arm the next
//!      read of the same site takes);
//!    * otherwise the inline load found `TAG_HOLE` — a field deleted since
//!      priming — and takes the ordinary miss, with the packed word, exactly
//!      as the emitted `pic.hit.deleted` -> `pic.miss.call` edge did.
//! 3. The Array-subclass named-prefix proof (cache word 2 against `ObjectMeta`
//!    word 6, then the cached slot). The emitted tower had this twice, once for
//!    a descriptor-free receiver whose exact ShapeId missed and once for a
//!    descriptor-bearing one that could never take the raw-load PIC at all;
//!    both reduce to the same three loads on a `GC_TYPE_OBJECT` receiver with a
//!    resolved cache, so they are one arm here.
//! 4. Everything else: `get_field_ic_miss_impl`, which owns the priming policy
//!    (`prime_get`, the megamorphic countdown, the `PERRY_IC_DIAG` rows).
//!
//! # What is deliberately NOT here
//!
//! The polymorphic ways stay inline. They are read-only compares against a
//! cache the site already resolved, they serve the receiver rotation that is
//! the whole point of #7753, and moving them behind a call is a separate
//! measurement.
//!
//! Typed feedback is unchanged: the OBSERVE call still sits inline in
//! `pget.recv_ok` and the guard-pass / guard-fail / fallback-call records still
//! sit on the emitted edges, all under the same compile-time gate. The one
//! feedback call that lives here is the class-ref arm's, which was a
//! feedback-wrapped helper on that edge before and still is.
//!
//! # Rooting
//!
//! Every call these functions make is in TAIL position: the tag-routing arms,
//! the overflow fallback, and `get_field_ic_miss_impl` are all `return`s, and
//! the two probes above them (`overflow_get`, the named-prefix loads) neither
//! allocate nor enter user code. There is therefore no window in which a raw
//! `obj`/`key` argument is named after a collection point, which is the
//! condition a `RuntimeHandleScope` exists to cover (cf. `js_put_value_set_ic_miss`,
//! which re-reads its key to prime AFTER the allocating `js_put_value_set`).
//! Adding a scope here would root nothing and would put a TLS push/pop on every
//! megamorphic read. If an arm ever grows work after a call, it needs the scope
//! at that moment.

use crate::object::{ObjectHeader, PicCacheSlot};
use std::sync::atomic::{AtomicU64, Ordering};

/// The exit for a receiver that is NOT a heap pointer: the emitted site's
/// `(tag & 0xFFFD) == 0x7FFD` test just failed.
///
/// It takes no cache: none of these arms can prime one, which is exactly why
/// splitting them off costs nothing and buys the emitted guard chain back (see
/// the module header).
///
/// * `obj_bits` — the receiver's full, UNMASKED NaN-box bits.
/// * `key` — the interned property-name `StringHeader`, already masked.
/// * `site_id` — the typed-feedback site id, used only by the class-ref arm.
///
/// `extern "C-unwind"`: the nullish arm throws (and the by-name arms can run a
/// getter that throws). Under `panic=unwind` — every dev-profile / test build of
/// the runtime — a plain `extern "C"` frame carries an abort-on-unwind guard,
/// so a caught `o.foo` on `undefined` aborted the process with "panic in a
/// function that cannot unwind" instead of reaching the `catch` (#11560; the
/// three `issue_5247_property_read_source_location` tests). Release builds
/// use `panic=abort` and plant no guard, which is why only debug runtimes saw
/// it.
#[no_mangle]
pub extern "C-unwind" fn js_object_get_field_ic_nonptr(
    obj_bits: i64,
    key: *const crate::StringHeader,
    site_id: u64,
) -> f64 {
    let bits = obj_bits as u64;
    let tag = bits >> 48;
    let obj_unmasked = bits as usize as *const ObjectHeader;

    // Heap STRING receiver. Only a `.length` site still tests the two
    // pointer-ish tags together (`(tag & 0xFFFD) == 0x7FFD`) and keeps its
    // inline string arm; every other key now emits the EXACT POINTER test, so
    // a string receiver arrives here instead of being unmasked, admitted by
    // the tag test, and rejected by the GC-kind guard four loads later. The
    // answer is the same one the object exit produced — the by-name helper —
    // but the pointer must be MASKED first: that helper normalizes only the
    // 0x7FFD tag, so handing it a 0x7FFF-tagged box would be a wild pointer.
    if tag == crate::value::STRING_TAG >> 48 {
        let masked = (bits & 0x0000_FFFF_FFFF_FFFF) as usize as *const ObjectHeader;
        return super::js_object_get_field_by_name_f64(masked, key);
    }
    // SSO receiver (SHORT_STRING_TAG): the SSO-aware by-name helper reads
    // `.length` from the NaN-box payload and answers undefined otherwise.
    // A `.length` site keeps serving this inline and never gets here.
    if tag == 0x7FF9 {
        return super::js_object_get_field_by_name_f64(obj_unmasked, key);
    }
    // INT32-tagged class ref: static field / dynamic IIFE-set property /
    // synthetic `constructor`. Passes the UNMASKED bits so the runtime can
    // detect the tag, exactly as the emitted `pget.recv_class_ref` did.
    if tag == 0x7FFE {
        return crate::typed_feedback::js_typed_feedback_object_get_field_by_name_f64(
            site_id,
            obj_unmasked,
            key,
        );
    }
    // `undefined`/`null` throw a node-shaped TypeError (#462). The emitted
    // arm passed the property's static bytes and ended in `unreachable`;
    // this reads the same bytes off the interned key and the helper is
    // `-> !`, so the two are the same divergence with the same message.
    if bits == crate::value::TAG_UNDEFINED || bits == crate::value::TAG_NULL {
        let is_null = u32::from(bits == crate::value::TAG_NULL);
        let (ptr, len) = unsafe {
            match super::super::has_own_helpers::str_from_string_header(key) {
                Some(s) => (s.as_ptr(), s.len()),
                None => (std::ptr::null(), 0),
            }
        };
        crate::error::js_throw_type_error_property_access(is_null, ptr, len);
    }
    // Every other tag: no auto-boxing, but the by-name helper still recognizes
    // typed shapes (Date `.constructor` via DATE_REGISTRY). A POINTER/STRING
    // receiver never reaches this entry from emitted code; if one ever did,
    // this arm is the correct — merely non-priming — answer for it too, so
    // there is no unverified mode behind the split.
    super::js_object_get_field_by_name_f64(obj_unmasked, key)
}

/// The spill-buffer read, out of line.
///
/// Kept in its own `#[cold] #[inline(never)]` function for a code-shape reason
/// that is worth 12 instructions on EVERY slow call: inlined, its
/// `overflow_get` call forces `js_object_get_field_ic_slow` to save
/// callee-saved registers across it, which gives that function a real frame
/// (`push rbp/r15/r14/rbx` + the matching pops) and stops the miss handler from
/// being a sibling call. Out of line, the entry's every call is in tail
/// position and it needs no frame at all.
///
/// Semantics are `js_object_get_field_ic_overflow_load`'s, unchanged: read
/// through `overflow_get`, and on a tombstoned slot fall back to the
/// THREE-argument miss entry — i.e. with a null `packed`, because
/// republishing the packed word here would re-decide which arm the next read
/// of this site takes.
///
/// # Safety
/// Same contract as the caller: `obj` is a live `GC_TYPE_OBJECT` above the
/// handle band whose shape stamp matched the packed word, and `slot` is that
/// word's high half with `IC_SLOT_OVERFLOW_BIT` set.
#[cold]
#[inline(never)]
unsafe fn overflow_arm(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
    cache_slot: *mut PicCacheSlot,
    index: u32,
) -> f64 {
    // The emitted `pic.spill.hit` serves a matched spill entry inline (S5), so
    // this arm is reached only from a runtime caller of the slow entry; it
    // reads exactly what that block loads, stored `undefined` included.
    if let Some(v) = crate::object::spill_get_present(obj as usize, index as usize) {
        return f64::from_bits(v);
    }
    super::ic_miss::get_field_ic_miss_impl(obj, key, cache_slot, std::ptr::null())
}

/// The exit for a receiver that IS a heap pointer — every failing guard on the
/// emitted site's object path lands here.
///
/// * `obj_handle` — the receiver with the NaN-box tag already masked off. The
///   caller has established the POINTER/STRING tag; this entry re-establishes
///   everything below it, because a guard failure says nothing about WHICH
///   guard failed.
/// * `key` — the interned property-name `StringHeader`, already masked.
/// * `cache_slot` — the site's [`PicCacheSlot`] (`@perry_ic_N`), possibly still
///   null: `pic_slot_peek` answers null and the miss handler resolves it when
///   it actually primes.
/// * `packed` — the site's compact MRU word (`@perry_ic_N_packed_get`).
///
/// `extern "C-unwind"` for the same reason as [`js_object_get_field_ic_nonptr`]:
/// the miss handler can run a throwing getter.
#[no_mangle]
pub extern "C-unwind" fn js_object_get_field_ic_slow(
    obj_handle: i64,
    key: *const crate::StringHeader,
    cache_slot: *mut PicCacheSlot,
    packed: *const AtomicU64,
) -> f64 {
    // --- 0. a MEGAMORPHIC site's slot guess, confirmed by the receiver ------
    //
    // A site whose way state is latched negative sees more shapes than any
    // per-site entry can name, and every read that misses its compact word
    // lands here. The site may still hold one thing: a slot GUESS (the compact
    // word's high half — the slot the receiver's shape answered last, step 2b),
    // which the RECEIVER'S own shape confirms or refutes
    // (`shapes::confirm_slot_guess`: the position bound says key position
    // `guess` is inline slot `guess`, and the key there IS this key, one
    // pointer compare). Everything the guess cannot answer continues in
    // `ic_slow_body` unchanged. Asked first, and only at a latched site, so a
    // site that can still be primed is primed exactly as before. `length` is
    // excluded as in step 2b: an Array-subclass receiver serves it from its
    // elements store. Kept in this frameless entry, with the body out of line,
    // so the confirmed read pays no prologue for the arms below.
    if let Some(value) = unsafe { megamorphic_slot_guess(obj_handle, key, cache_slot, packed) } {
        return value;
    }
    ic_slow_body(obj_handle, key, cache_slot, packed)
}

/// Step 0 of [`js_object_get_field_ic_slow`]: the latched site's slot guess.
///
/// # Safety
/// The entry's contract: a POINTER receiver handle, this site's cache slot
/// and packed word.
#[inline(always)]
unsafe fn megamorphic_slot_guess(
    obj_handle: i64,
    key: *const crate::StringHeader,
    cache_slot: *mut PicCacheSlot,
    packed: *const AtomicU64,
) -> Option<f64> {
    let obj = obj_handle as usize as *const ObjectHeader;
    if packed.is_null()
        || key.is_null()
        || !crate::value::addr_class::is_above_handle_band(obj as usize)
    {
        return None;
    }
    let cache = crate::object::pic_slot_peek(cache_slot);
    if cache.is_null()
        || (*cache)[crate::object::field_get_set::ic_miss::PIC_WAY_STATE] >= 0
        || key_is_length(key)
    {
        return None;
    }
    let guess = ((*packed).load(Ordering::Relaxed) >> 32) as usize;
    let value = crate::object::shapes::confirm_slot_guess(obj, key, guess)?;
    #[cfg(test)]
    crate::object::shapes::SHAPE_ANSWERED_READS.fetch_add(1, Ordering::Relaxed);
    Some(value)
}

/// Everything after step 0 of [`js_object_get_field_ic_slow`].
#[inline(never)]
fn ic_slow_body(
    obj_handle: i64,
    key: *const crate::StringHeader,
    cache_slot: *mut PicCacheSlot,
    packed: *const AtomicU64,
) -> f64 {
    let obj = obj_handle as usize as *const ObjectHeader;
    let addr = obj as usize;
    // Below the handle band the emitted code never dereferenced, and neither
    // does this: the miss handler routes registry ids through
    // HANDLE_PROPERTY_DISPATCH.
    if crate::value::addr_class::is_above_handle_band(addr) {
        // SAFETY: the caller's emitted `(tag & 0xFFFD) == 0x7FFD` test
        // established a POINTER/STRING NaN-box and the address is above the
        // handle band — the same licence under which the emitted tower loaded
        // this header word inline.
        unsafe {
            let header = &*((addr - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader);
            if header.obj_type == crate::gc::GC_TYPE_OBJECT {
                let plain = header._reserved & crate::gc::OBJ_FLAG_HAS_DESCRIPTORS == 0;
                // --- 2. the MRU token hit the emitted hit path declined -----
                if plain && !packed.is_null() {
                    // The emitted guard is `icmp eq i32 %pcid, trunc(%packed)`
                    // against the RAW header word, with no range check, and
                    // this is that test: a nonzero packed word always carries a
                    // valid ShapeId (`prime_get` publishes nothing else), and a
                    // receiver whose word is an ordinary class id can never
                    // equal one, so `object_shape_stamp`'s range check would
                    // only re-derive what the equality already proves (#809's
                    // keyless receiver fails the equality, not the range test).
                    let word = (*packed).load(Ordering::Relaxed);
                    if let Some((stamp, index, is_spill)) = super::ic_miss::packed_get_decode(word)
                    {
                        if (*obj).parent_class_id == stamp {
                            if is_spill {
                                return overflow_arm(obj, key, cache_slot, index);
                            }
                            // An inline slot that reached this entry on a token hit
                            // was a `TAG_HOLE` — the field was deleted since
                            // priming. The emitted `pic.hit.deleted` edge took the
                            // ordinary miss WITH the packed word.
                            return super::ic_miss::get_field_ic_miss_impl(
                                obj, key, cache_slot, packed,
                            );
                        }
                    }
                }
                // --- 2b. a MEGAMORPHIC site: the receiver's shape answers ---
                //
                // A site whose way state is latched negative will not be primed
                // again, so the miss handler below would re-derive the receiver
                // class, probe the inherited-read cache, try to prime and scan
                // by name — ~700 instructions per read, measured on a 40-shape
                // `o.kind` site (node: ~51). The receiver's own shape already
                // knows the answer: an ordinary own data key's inline slot is its
                // position in the shape's canonical key list. Anything the shape
                // cannot answer by position (dictionary, generation > 0,
                // tombstones, spill, inherited, descriptors) falls through
                // unchanged.
                // `length` is excluded (UTF-16 length word first, so the byte
                // compare runs only for 6-unit keys): an Array-subclass receiver serves it
                // from its elements store, not from a key position.
                if plain && !key.is_null() && !key_is_length(key) {
                    let cache = crate::object::pic_slot_peek(cache_slot);
                    if !cache.is_null()
                        && (*cache)[crate::object::field_get_set::ic_miss::PIC_WAY_STATE] < 0
                    {
                        if let Some(rec) =
                            crate::object::shapes::shape_record_by_id((*obj).parent_class_id)
                        {
                            // The site's last primed slot is the guess (the
                            // compact word's high half; a spill entry's
                            // flipped id carries no inline slot to guess).
                            let word = if packed.is_null() {
                                u64::MAX
                            } else {
                                (*packed).load(Ordering::Relaxed)
                            };
                            let hint = (word >> 32) as usize;
                            if let Some(slot) = rec.inline_slot_of_key(key, hint) {
                                #[cfg(test)]
                                crate::object::shapes::SHAPE_ANSWERED_READS
                                    .fetch_add(1, Ordering::Relaxed);
                                // Keep the answer as the site's next slot GUESS
                                // (owner-approved form: a guess the receiver's
                                // shape confirms). Only while the word's low
                                // half is unmatchable (`PACKED_GET_EMPTY`'s
                                // 0xFFFF_FFFF): the inline ShapeId compare can
                                // never equal it, and `packed_get_decode` reads
                                // it as no entry.
                                if slot != hint && !packed.is_null() && word as u32 == u32::MAX {
                                    (*packed).store(
                                        ((slot as u64) << 32) | u64::from(u32::MAX),
                                        Ordering::Relaxed,
                                    );
                                }
                                let field = (obj as *const u8)
                                    .add(std::mem::size_of::<ObjectHeader>() + slot * 8)
                                    as *const f64;
                                return *field;
                            }
                            // S5: a SPILL-located own data key is answered the
                            // same way — its position in the shape's key list
                            // is its index in the receiver's spill buffer. The
                            // site keeps no guess for it (the word's high half
                            // is an inline-slot guess).
                            if let Some(pos) = rec.spill_position_of_key(key) {
                                if let Some(bits) = crate::object::spill_get_present(addr, pos) {
                                    #[cfg(test)]
                                    crate::object::shapes::SHAPE_ANSWERED_SPILL_READS
                                        .fetch_add(1, Ordering::Relaxed);
                                    crate::hot_diag::recv_route_note_runtime(
                                        crate::hot_diag::RT_ROUTE_MEGA_SPILL,
                                    );
                                    return f64::from_bits(bits);
                                }
                            }
                        }
                    }
                }
                // (There is no third arm. An object-backed Array subclass used
                // to be served here by a class-wide "named-prefix" token held
                // in cache word 2 and matched against the receiver's
                // ObjectMeta, across ShapeIds the site had never been primed
                // on. That token was site state not derived from one shape, and
                // S6 removed it: a site now holds only `(ShapeId, slot)` pairs,
                // and an Array-subclass receiver whose ShapeId the site has not
                // seen takes the miss handler below like any other receiver.
                // The token survives on the OBJECT as a prime-time proof only —
                // see `get_field_ic_miss_impl`.)
            }
        }
    }

    // --- 4. everything else: the miss handler owns the priming policy -------
    super::ic_miss::get_field_ic_miss_impl(obj, key, cache_slot, packed)
}

/// `key` spells `length` — six bytes, compared directly (no UTF-8 validation).
#[inline]
unsafe fn key_is_length(key: *const crate::StringHeader) -> bool {
    (*key).byte_len == 6
        && std::slice::from_raw_parts(crate::string::string_data(key), 6) == b"length"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{PicCache, PIC_CACHE_WORDS};

    /// The compact word an emitted site is born holding.
    const PACKED_GET_EMPTY_WORD: u64 = 0xFFFF_FFFF;

    fn key_of(bytes: &[u8]) -> *const crate::StringHeader {
        crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
    }

    /// What the emitted site passes: the NaN box with its tag masked off. The
    /// tag itself was already tested inline, which is why this entry never
    /// sees it.
    fn handle(obj: *mut ObjectHeader) -> i64 {
        (obj as u64 & 0x0000_FFFF_FFFF_FFFF) as i64
    }

    /// Build one receiver per distinct shape: every object gets `pos`, `end`,
    /// `kind` (so `kind` sits at slot 2 in all of them) and then ONE distinct
    /// extra key, which forks the shape. Returns (receivers, kind key).
    fn megamorphic_receivers<'s>(
        scope: &'s crate::gc::RuntimeHandleScope,
        n: usize,
    ) -> Vec<crate::gc::RuntimeHandle<'s>> {
        let mut out = Vec::new();
        for i in 0..n {
            let obj = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 8));
            for (k, v) in [
                (&b"pos"[..], 1.0),
                (&b"end"[..], 2.0),
                (&b"kind"[..], 100.0 + i as f64),
            ] {
                let key = scope.root_string_ptr(key_of(k));
                obj.with_mut_ptr(|o| {
                    key.with_const_ptr(|kp| crate::object::js_object_set_field_by_name(o, kp, v))
                });
            }
            let extra = format!("x{i}");
            let key = scope.root_string_ptr(key_of(extra.as_bytes()));
            obj.with_mut_ptr(|o| {
                key.with_const_ptr(|kp| crate::object::js_object_set_field_by_name(o, kp, 7.0))
            });
            out.push(obj);
        }
        out
    }

    /// S3: once a site has latched megamorphic, a read is answered by the
    /// RECEIVER'S SHAPE (its key list), for every one of 48 shapes — and the
    /// answer is the receiver's own value, not the value of whichever shape
    /// last primed the site.
    #[test]
    fn a_latched_megamorphic_site_is_answered_by_the_receivers_shape() {
        let _lock = crate::gc::global_side_table_test_lock();
        let scope = crate::gc::RuntimeHandleScope::new();
        let objs = megamorphic_receivers(&scope, 48);
        let kind = scope.root_string_ptr(key_of(b"kind"));
        let mut cache: PicCache = [0; PIC_CACHE_WORDS];
        let mut slot: PicCacheSlot = &mut cache;
        let packed = AtomicU64::new(0);
        let read = |o: &crate::gc::RuntimeHandle<'_>, slot: &mut PicCacheSlot| {
            o.with_mut_ptr(|p: *mut ObjectHeader| {
                kind.with_const_ptr(|k| js_object_get_field_ic_slow(handle(p), k, slot, &packed))
            })
        };
        // Drive the site until it latches.
        for round in 0..4 {
            for (i, o) in objs.iter().enumerate() {
                assert_eq!(
                    read(o, &mut slot),
                    100.0 + i as f64,
                    "round {round} receiver {i}"
                );
            }
        }
        assert!(
            cache[crate::object::field_get_set::ic_miss::PIC_WAY_STATE] < 0,
            "48 shapes must latch the site megamorphic: state {}",
            cache[crate::object::field_get_set::ic_miss::PIC_WAY_STATE]
        );
        let before =
            crate::object::shapes::SHAPE_ANSWERED_READS.load(std::sync::atomic::Ordering::Relaxed);
        for (i, o) in objs.iter().enumerate() {
            assert_eq!(
                read(o, &mut slot),
                100.0 + i as f64,
                "latched read, receiver {i}"
            );
        }
        let answered = crate::object::shapes::SHAPE_ANSWERED_READS
            .load(std::sync::atomic::Ordering::Relaxed)
            - before;
        // The one receiver whose shape the compact word still names is served
        // by the word itself (inline, in emitted code; step 2 here). Every
        // other latched read is answered by its receiver's shape.
        assert!(
            answered >= 47,
            "every latched read the word cannot serve must be answered by the shape: {answered}"
        );
        // A WRONG slot guess (the compact word's high half) must not change the
        // answer: the shape confirms or refutes the guess.
        packed.store(5u64 << 32, std::sync::atomic::Ordering::Relaxed);
        for (i, o) in objs.iter().enumerate() {
            assert_eq!(
                read(o, &mut slot),
                100.0 + i as f64,
                "wrong guess, receiver {i}"
            );
        }
    }

    /// S3 declines what a key POSITION cannot answer: a key the shape does not
    /// have (inherited/absent) still reaches the full miss handler.
    #[test]
    fn a_latched_site_still_answers_an_absent_key_through_the_miss_handler() {
        let _lock = crate::gc::global_side_table_test_lock();
        let scope = crate::gc::RuntimeHandleScope::new();
        let objs = megamorphic_receivers(&scope, 48);
        let absent = scope.root_string_ptr(key_of(b"notthere"));
        let mut cache: PicCache = [0; PIC_CACHE_WORDS];
        let mut slot: PicCacheSlot = &mut cache;
        let packed = AtomicU64::new(0);
        let kind = scope.root_string_ptr(key_of(b"kind"));
        for _ in 0..4 {
            for o in &objs {
                o.with_mut_ptr(|p: *mut ObjectHeader| {
                    kind.with_const_ptr(|k| {
                        js_object_get_field_ic_slow(handle(p), k, &mut slot, &packed)
                    })
                });
            }
        }
        for o in &objs {
            let v = o.with_mut_ptr(|p: *mut ObjectHeader| {
                absent.with_const_ptr(|k| {
                    js_object_get_field_ic_slow(handle(p), k, &mut slot, &packed)
                })
            });
            assert_eq!(
                v.to_bits(),
                crate::value::TAG_UNDEFINED,
                "an absent key reads undefined"
            );
        }
    }

    /// `megamorphic_receivers` with a chosen text for the read key (slot 2),
    /// each receiver growing that key through its OWN freshly allocated string
    /// — never an atom — so which string a shape's list ends up holding is
    /// decided by the list writer, not by the test.
    fn megamorphic_receivers_keyed<'s>(
        scope: &'s crate::gc::RuntimeHandleScope,
        n: usize,
        read_key: &[u8],
        extra_prefix: &str,
    ) -> Vec<crate::gc::RuntimeHandle<'s>> {
        let mut out = Vec::new();
        for i in 0..n {
            let obj = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 8));
            for (k, v) in [
                (&b"pos"[..], 1.0),
                (&b"end"[..], 2.0),
                (read_key, 100.0 + i as f64),
            ] {
                let key = scope.root_string_ptr(key_of(k));
                obj.with_mut_ptr(|o| {
                    key.with_const_ptr(|kp| crate::object::js_object_set_field_by_name(o, kp, v))
                });
            }
            let extra = format!("{extra_prefix}{i}");
            let key = scope.root_string_ptr(key_of(extra.as_bytes()));
            obj.with_mut_ptr(|o| {
                key.with_const_ptr(|kp| crate::object::js_object_set_field_by_name(o, kp, 7.0))
            });
            out.push(obj);
        }
        out
    }

    fn atom_of(text: &[u8]) -> *mut crate::StringHeader {
        let hash = crate::object::key_bytes_hash(text.as_ptr(), text.len());
        crate::string::js_string_pool_atom(text.as_ptr(), text.len() as u32, hash, 0)
    }

    fn answered() -> u64 {
        crate::object::shapes::SHAPE_ANSWERED_READS.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Read `key` at one latched site over `objs` until it latches, then once
    /// more; every read must return `expect(i)`. Returns the reads the
    /// receivers' SHAPES answered on the latched pass.
    fn latched_pass(
        objs: &[crate::gc::RuntimeHandle<'_>],
        key: &crate::gc::RuntimeHandle<'_>,
        expect: impl Fn(usize) -> u64,
    ) -> u64 {
        let mut cache: PicCache = [0; PIC_CACHE_WORDS];
        let mut slot: PicCacheSlot = &mut cache;
        let packed = AtomicU64::new(0);
        let read = |o: &crate::gc::RuntimeHandle<'_>, slot: &mut PicCacheSlot| {
            o.with_mut_ptr(|p: *mut ObjectHeader| {
                key.with_const_ptr(|k| js_object_get_field_ic_slow(handle(p), k, slot, &packed))
            })
        };
        for round in 0..4 {
            for (i, o) in objs.iter().enumerate() {
                assert_eq!(
                    read(o, &mut slot).to_bits(),
                    expect(i),
                    "round {round} receiver {i}"
                );
            }
        }
        assert!(
            cache[crate::object::field_get_set::ic_miss::PIC_WAY_STATE] < 0,
            "premise: the site latched megamorphic"
        );
        let before = answered();
        for (i, o) in objs.iter().enumerate() {
            assert_eq!(
                read(o, &mut slot).to_bits(),
                expect(i),
                "latched read, receiver {i}"
            );
        }
        answered() - before
    }

    /// S3b: one key text reaching the runtime as TWO string objects — each
    /// receiver grows it through its own fresh copy — is still ONE string in
    /// every shape's key list: the text's atom, the same object a read site's
    /// pooled key is. So the site matches by pointer; and a site holding yet
    /// another copy (not the atom) still gets the right answer by bytes.
    #[test]
    fn a_key_text_in_two_string_objects_is_one_atom_in_every_shape() {
        let _lock = crate::gc::global_side_table_test_lock();
        let scope = crate::gc::RuntimeHandleScope::new();
        let text = b"s3b_two_objects_kind";
        let atom = scope.root_string_ptr(atom_of(text));
        let copy = scope.root_string_ptr(key_of(text));
        let addr = |h: &crate::gc::RuntimeHandle<'_>| {
            h.with_const_ptr(|p: *const crate::StringHeader| p as usize)
        };
        assert_ne!(
            addr(&atom),
            addr(&copy),
            "premise: two string objects, one text"
        );
        assert!(
            unsafe { crate::string::is_atom_for_test(atom.with_const_ptr(|p| p)) },
            "premise: the pool mint made an atom"
        );
        let objs = megamorphic_receivers_keyed(&scope, 48, text, "s3b_two_x");
        let atom_bits = crate::value::js_nanbox_string(addr(&atom) as i64).to_bits();
        for (i, o) in objs.iter().enumerate() {
            let stored = o.with_const_ptr(|p: *const ObjectHeader| unsafe {
                crate::object::object_keys(p).get(2).bits()
            });
            assert_eq!(
                stored, atom_bits,
                "INVARIANT: receiver {i}'s shape holds the atom, not the copy it grew with"
            );
        }
        // Identity is not eligibility: the atom is not an interned string, and
        // a computed key with the text interns to its own string, never to the
        // atom (`GC_FLAG_INTERNED` admits keys to the own-property lanes; an
        // atom must not widen them).
        let atom_flags = atom.with_const_ptr(|p: *const crate::StringHeader| unsafe {
            (*((p as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader))
                .gc_flags
        });
        assert_eq!(
            atom_flags & crate::gc::GC_FLAG_INTERNED,
            0,
            "INVARIANT: an atom is never flagged interned"
        );
        let interned = copy.with_const_ptr(|p: *const crate::StringHeader| {
            crate::string::js_string_intern(
                p,
                crate::object::key_bytes_hash(text.as_ptr(), text.len()),
            ) as usize
        });
        assert_ne!(
            interned,
            addr(&atom),
            "INVARIANT: interning never returns the atom"
        );
        // The site holds the atom: every latched read is the receiver's own
        // value, answered by its shape.
        let by_atom = latched_pass(&objs, &atom, |i| (100.0 + i as f64).to_bits());
        assert!(
            by_atom >= 47,
            "the shape answers a pointer-equal key: {by_atom}"
        );
        // A site holding a different string object with the same text (not
        // the atom): the answer is the same, found by bytes.
        let fresh = scope.root_string_ptr(key_of(text));
        let by_bytes = latched_pass(&objs, &fresh, |i| (100.0 + i as f64).to_bits());
        assert!(
            by_bytes >= 47,
            "a non-atom key text still matches by bytes: {by_bytes}"
        );
    }

    /// A list written BEFORE its key's atom existed holds another string. A
    /// pointer mismatch is not an answer: the shape still answers by bytes.
    #[test]
    fn a_list_written_before_the_atom_existed_still_answers() {
        let _lock = crate::gc::global_side_table_test_lock();
        let scope = crate::gc::RuntimeHandleScope::new();
        let text = b"s3b_pre_atom_kind";
        let objs = megamorphic_receivers_keyed(&scope, 48, text, "s3b_pre_x");
        // The lists hold the string the grow path interned; a collision
        // evicts it from the cache before the atom is minted, so the atom is
        // a different object.
        crate::string::test_evict_interned(text);
        let atom = scope.root_string_ptr(atom_of(text));
        let atom_bits = atom.with_const_ptr(|p: *const crate::StringHeader| {
            crate::value::js_nanbox_string(p as i64).to_bits()
        });
        let stored = objs[0].with_const_ptr(|p: *const ObjectHeader| unsafe {
            crate::object::object_keys(p).get(2).bits()
        });
        assert_ne!(stored, atom_bits, "premise: the list predates the atom");
        let n = latched_pass(&objs, &atom, |i| (100.0 + i as f64).to_bits());
        assert!(n >= 47, "the shape answers by bytes: {n}");
    }

    /// Dictionary receivers keep their ShapeId across layout changes, so the
    /// shape can never answer for one by position: a latched site reading
    /// half-dictionary receivers answers every read correctly, and not one
    /// dictionary read is counted as shape-answered.
    #[test]
    fn a_dictionary_receiver_is_never_answered_by_position() {
        let _lock = crate::gc::global_side_table_test_lock();
        let scope = crate::gc::RuntimeHandleScope::new();
        let text = b"s3b_dict_kind";
        let atom = scope.root_string_ptr(atom_of(text));
        let objs = megamorphic_receivers_keyed(&scope, 48, text, "s3b_dict_x");
        let ordinary = latched_pass(&objs, &atom, |i| (100.0 + i as f64).to_bits());
        assert!(
            ordinary >= 47,
            "premise: ordinary receivers are shape-answered ({ordinary})"
        );
        let mut dict = 0;
        for o in objs.iter().step_by(2) {
            if o.with_mut_ptr(|p: *mut ObjectHeader| unsafe {
                crate::object::dictionary::latch_object_to_dictionary(p)
            }) {
                dict += 1;
                let bound = o.with_const_ptr(|p: *const ObjectHeader| {
                    crate::object::shapes::test_position_bound_of(p)
                });
                assert_eq!(bound, Some(0), "a dictionary shape has no position bound");
            }
        }
        assert_eq!(
            dict, 24,
            "premise: every other receiver latched to dictionary"
        );
        let n = latched_pass(&objs, &atom, |i| (100.0 + i as f64).to_bits());
        assert!(
            n <= 24,
            "INVARIANT: at most the 24 ordinary receivers are shape-answered, got {n}"
        );
    }

    /// Tombstones: a receiver whose list carries a HOLE (a tombstone delete)
    /// cannot be answered by position — its shape reports no position bound —
    /// and every read still returns the receiver's own value; the deleted key
    /// reads undefined.
    #[test]
    fn a_receiver_with_a_tombstoned_key_is_never_answered_by_position() {
        struct Restore;
        impl Drop for Restore {
            fn drop(&mut self) {
                crate::object::delete_rest::test_set_tombstone_deletes(None);
            }
        }
        crate::object::delete_rest::test_set_tombstone_deletes(Some(true));
        let _restore = Restore;
        let _lock = crate::gc::global_side_table_test_lock();
        let scope = crate::gc::RuntimeHandleScope::new();
        let text = b"s3b_tomb_kind";
        let atom = scope.root_string_ptr(atom_of(text));
        let objs = megamorphic_receivers_keyed(&scope, 48, text, "s3b_tomb_x");
        let _ = latched_pass(&objs, &atom, |i| (100.0 + i as f64).to_bits());
        let end = scope.root_string_ptr(key_of(b"end"));
        let mut tombstoned = 0;
        for (i, o) in objs.iter().enumerate().step_by(3) {
            // The first delete of a shared list compacts (it takes ownership);
            // the second tombstones in place (`tombstone_tests.rs`).
            let extra = format!("s3b_tomb_x{i}");
            let extra = scope.root_string_ptr(key_of(extra.as_bytes()));
            for k in [&extra, &end] {
                o.with_mut_ptr(|p: *mut ObjectHeader| {
                    k.with_const_ptr(|kp| crate::object::js_object_delete_field(p, kp))
                });
            }
            let (holes, bound) = o.with_const_ptr(|p: *const ObjectHeader| unsafe {
                (
                    crate::object::shapes::object_shape_hole_count(p),
                    crate::object::shapes::test_position_bound_of(p),
                )
            });
            if holes > 0 {
                tombstoned += 1;
                assert_eq!(
                    bound,
                    Some(0),
                    "INVARIANT: a tombstoned shape has no position bound"
                );
            }
        }
        assert_eq!(
            tombstoned, 16,
            "premise: every third receiver carries a tombstone"
        );
        let _ = latched_pass(&objs, &atom, |i| (100.0 + i as f64).to_bits());
        let _ = latched_pass(&objs, &end, |i| {
            if i % 3 == 0 {
                crate::value::TAG_UNDEFINED
            } else {
                2.0f64.to_bits()
            }
        });
    }

    /// The slot-guess confirm for shape `shape_id`, as the megamorphic entry
    /// asks it (`shapes::slot_guess_confirmed`): `Some(guess)` when the shape
    /// says key position `guess` is inline slot `guess` and holds exactly the
    /// key `site_key_bits` names, `None` for a decline.
    unsafe fn guess_walk(shape_id: u32, guess: u64, site_key_bits: u64) -> Option<usize> {
        let key = (site_key_bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::StringHeader;
        crate::object::shapes::slot_guess_confirmed(shape_id, key, guess as usize)
            .then_some(guess as usize)
    }

    fn stamp_of(o: &crate::gc::RuntimeHandle<'_>) -> u32 {
        o.with_const_ptr(|p: *const ObjectHeader| unsafe {
            crate::object::shapes::object_shape_stamp(p)
        })
    }

    /// For every ordinary receiver the slot-guess confirm reaches the
    /// receiver's own record, confirms the site's guess against the
    /// receiver's own key, and names the slot holding the receiver's own
    /// value. A guess whose position holds a DIFFERENT key, a guess past the
    /// shape's bound, an id outside the ShapeId range and an id whose page
    /// was never allocated all decline.
    #[test]
    fn the_slot_guess_is_confirmed_only_against_the_receivers_own_key() {
        let _lock = crate::gc::global_side_table_test_lock();
        let scope = crate::gc::RuntimeHandleScope::new();
        let text = b"s3c_walk_kind";
        let atom = scope.root_string_ptr(atom_of(text));
        let site = atom.with_const_ptr(|p: *const crate::StringHeader| {
            crate::value::js_nanbox_string(p as i64).to_bits()
        });
        let objs = megamorphic_receivers_keyed(&scope, 48, text, "s3c_walk_x");
        for (i, o) in objs.iter().enumerate() {
            let id = stamp_of(o);
            let slot = unsafe { guess_walk(id, 2, site) };
            assert_eq!(slot, Some(2), "receiver {i}: the right guess is confirmed");
            let value = o.with_const_ptr(|p: *const ObjectHeader| unsafe {
                *((p as *const u8).add(std::mem::size_of::<ObjectHeader>() + 2 * 8) as *const f64)
            });
            assert_eq!(
                value,
                100.0 + i as f64,
                "receiver {i}: the confirmed slot is its own"
            );
            for wrong in [0u64, 1, 3] {
                assert_eq!(
                    unsafe { guess_walk(id, wrong, site) },
                    None,
                    "receiver {i}: guess {wrong} holds another key and must decline"
                );
            }
            assert_eq!(unsafe { guess_walk(id, 4, site) }, None, "past the bound");
            assert_eq!(unsafe { guess_walk(id, u64::from(u32::MAX), site) }, None);
        }
        assert_eq!(
            unsafe { guess_walk(5, 2, site) },
            None,
            "a class id is no ShapeId"
        );
        assert_eq!(
            unsafe { guess_walk(0xBFFF_FFFF, 2, site) },
            None,
            "a page never minted"
        );
    }

    /// The slot-guess confirm on dictionary and tombstoned receivers: declines,
    /// whatever the guess.
    #[test]
    fn the_slot_guess_never_matches_a_dictionary_or_tombstoned_receiver() {
        struct Restore;
        impl Drop for Restore {
            fn drop(&mut self) {
                crate::object::delete_rest::test_set_tombstone_deletes(None);
            }
        }
        crate::object::delete_rest::test_set_tombstone_deletes(Some(true));
        let _restore = Restore;
        let _lock = crate::gc::global_side_table_test_lock();
        let scope = crate::gc::RuntimeHandleScope::new();
        let text = b"s3c_walk2_kind";
        let atom = scope.root_string_ptr(atom_of(text));
        let site = atom.with_const_ptr(|p: *const crate::StringHeader| {
            crate::value::js_nanbox_string(p as i64).to_bits()
        });
        let objs = megamorphic_receivers_keyed(&scope, 8, text, "s3c_walk2_x");
        let dict = &objs[0];
        assert!(
            dict.with_mut_ptr(|p: *mut ObjectHeader| unsafe {
                crate::object::dictionary::latch_object_to_dictionary(p)
            }),
            "premise: the receiver latched to dictionary"
        );
        for guess in 0..4 {
            assert_eq!(
                unsafe { guess_walk(stamp_of(dict), guess, site) },
                None,
                "dictionary"
            );
        }
        let tomb = &objs[1];
        let end = scope.root_string_ptr(key_of(b"end"));
        let extra = scope.root_string_ptr(key_of(b"s3c_walk2_x1"));
        for k in [&extra, &end] {
            tomb.with_mut_ptr(|p: *mut ObjectHeader| {
                k.with_const_ptr(|kp| crate::object::js_object_delete_field(p, kp))
            });
        }
        let holes = tomb.with_const_ptr(|p: *const ObjectHeader| unsafe {
            crate::object::shapes::object_shape_hole_count(p)
        });
        assert!(holes > 0, "premise: the receiver carries a tombstone");
        for guess in 0..4 {
            assert_eq!(
                unsafe { guess_walk(stamp_of(tomb), guess, site) },
                None,
                "tombstoned"
            );
        }
    }

    fn boxed(p: *mut ObjectHeader) -> f64 {
        f64::from_bits(0x7FFD_0000_0000_0000 | (p as u64 & 0x0000_FFFF_FFFF_FFFF))
    }

    /// A [[Prototype]] change is a shape transition (the prototype is part of
    /// shape identity) that moves no own key and no own slot: receivers built
    /// by `new F()` / `setPrototypeOf` before their fields are assigned — every
    /// tsc AST node — get a NEW shape with the SAME own-key layout as a
    /// prototype-less twin, and the megamorphic read answers them from that
    /// shape, by the key-list scan and by the slot-guess confirm.
    #[test]
    fn a_prototype_change_is_a_new_shape_with_the_same_layout_and_is_answered() {
        let _lock = crate::gc::global_side_table_test_lock();
        let scope = crate::gc::RuntimeHandleScope::new();
        let text = b"s3_proto_kind";
        let atom = scope.root_string_ptr(atom_of(text));
        let site = atom.with_const_ptr(|p: *const crate::StringHeader| {
            crate::value::js_nanbox_string(p as i64).to_bits()
        });
        let proto = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 2));
        let build = |with_proto: bool, i: usize| {
            let obj = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 8));
            if with_proto {
                let (o, p) = (
                    obj.with_mut_ptr(|o: *mut ObjectHeader| o),
                    proto.with_mut_ptr(|p: *mut ObjectHeader| p),
                );
                crate::object::object_ops::js_object_set_prototype_of(boxed(o), boxed(p));
            }
            let extra = format!("s3_proto_x{i}");
            for (k, v) in [
                (&b"pos"[..], 1.0),
                (&b"end"[..], 2.0),
                (&text[..], 100.0 + i as f64),
                (extra.as_bytes(), 7.0),
            ] {
                let key = scope.root_string_ptr(key_of(k));
                obj.with_mut_ptr(|o| {
                    key.with_const_ptr(|kp| crate::object::js_object_set_field_by_name(o, kp, v))
                });
            }
            obj
        };
        let objs: Vec<_> = (0..48).map(|i| build(true, i)).collect();
        let twin = build(false, 0);
        let (shape, twin_shape) = (stamp_of(&objs[0]), stamp_of(&twin));
        assert_ne!(
            shape, twin_shape,
            "premise: the prototype is part of shape identity"
        );
        let d = |id| crate::object::shapes::shape_descriptor_by_id(id).expect("live shape");
        let (d, t) = (d(shape), d(twin_shape));
        assert_eq!(
            (
                d.keys,
                d.logical_key_count,
                d.live_inline_slot_count,
                d.semantic_generation
            ),
            (
                t.keys,
                t.logical_key_count,
                t.live_inline_slot_count,
                t.semantic_generation
            ),
            "the prototype change moved no own key and bumped no generation"
        );
        for (i, o) in objs.iter().enumerate() {
            assert_eq!(
                unsafe { guess_walk(stamp_of(o), 2, site) },
                Some(2),
                "INVARIANT: the guess confirm answers prototype-linked receiver {i}"
            );
        }
        let n = latched_pass(&objs, &atom, |i| (100.0 + i as f64).to_bits());
        assert!(
            n >= 47,
            "INVARIANT: prototype-linked receivers are shape-answered: {n}"
        );
    }

    /// A shape over a SHIFTED keys array (a front offset): the slot-guess
    /// confirm reads LOGICAL key position `i`, past the front offset, so the
    /// guess for the key now at logical 0 is confirmed at 0, and the physical
    /// position it used to occupy (1) is not.
    #[test]
    fn a_shape_over_a_shifted_keys_array_is_answered_by_logical_position() {
        let _lock = crate::gc::global_side_table_test_lock();
        let scope = crate::gc::RuntimeHandleScope::new();
        let arr = scope.root_raw_mut_ptr(crate::array::js_array_alloc(4));
        let mut keys = Vec::new();
        for name in [&b"sh_a"[..], b"sh_b", b"sh_c"] {
            let k = scope.root_string_ptr(key_of(name));
            let bits = k.with_const_ptr(|p: *const crate::StringHeader| {
                crate::value::js_nanbox_string(p as i64).to_bits()
            });
            arr.with_mut_ptr(|a| crate::array::js_array_push(a, crate::JSValue::from_bits(bits)));
            keys.push(k);
        }
        let a = arr.with_mut_ptr(|a: *mut crate::array::ArrayHeader| a);
        let bound =
            |id: u32| crate::object::shapes::test_position_bound_of_id(id).expect("a live shape");
        let flat = crate::object::shapes::shape_descriptor_ensure(a, 3, 3).expect("mints");
        assert_eq!(bound(flat), 3, "premise: an unshifted keys array");
        crate::array::js_array_shift_f64(a);
        assert_ne!(
            unsafe { crate::array::array_front_offset(a) },
            0,
            "premise: shift left a front offset"
        );
        let shifted = crate::object::shapes::shape_descriptor_ensure(a, 2, 2).expect("mints");
        assert_eq!(bound(shifted), 2, "premise: the shifted shape has two keys");
        let sh_b = keys[1].with_const_ptr(|p: *const crate::StringHeader| {
            crate::value::js_nanbox_string(p as i64).to_bits()
        });
        assert_eq!(
            unsafe { guess_walk(shifted, 0, sh_b) },
            Some(0),
            "INVARIANT: the confirm reads the logical position"
        );
        assert_eq!(
            unsafe { guess_walk(shifted, 1, sh_b) },
            None,
            "INVARIANT: the physical position is not a key position"
        );
    }

    /// The positional bit is a stored FACT of the shape record, read on every
    /// latched miss; it must equal its definition for every record the agent
    /// has minted. Mints ordinary, dictionary, tombstoned, accessor and
    /// class-keyed shapes, then walks the whole slab.
    #[test]
    fn every_minted_records_positional_bit_matches_its_facts() {
        struct Restore;
        impl Drop for Restore {
            fn drop(&mut self) {
                crate::object::delete_rest::test_set_tombstone_deletes(None);
            }
        }
        crate::object::delete_rest::test_set_tombstone_deletes(Some(true));
        let _restore = Restore;
        let _lock = crate::gc::global_side_table_test_lock();
        let scope = crate::gc::RuntimeHandleScope::new();
        let objs = megamorphic_receivers_keyed(&scope, 24, b"pos_fact_kind", "pos_fact_x");
        // Tombstones: the in-place stable-tombstone update rewrites the hole
        // count (two deletes: the first compacts, the second tombstones).
        let end = scope.root_string_ptr(key_of(b"end"));
        for (i, o) in objs.iter().enumerate().step_by(3) {
            let extra = format!("pos_fact_x{i}");
            let extra = scope.root_string_ptr(key_of(extra.as_bytes()));
            for k in [&extra, &end] {
                o.with_mut_ptr(|p: *mut ObjectHeader| {
                    k.with_const_ptr(|kp| crate::object::js_object_delete_field(p, kp))
                });
            }
        }
        // Dictionaries.
        for o in objs.iter().skip(1).step_by(3) {
            o.with_mut_ptr(|p: *mut ObjectHeader| unsafe {
                crate::object::dictionary::latch_object_to_dictionary(p)
            });
        }
        // Keep the tombstoned receivers growing: re-adds take the cached
        // stable-tombstone update.
        let again = scope.root_string_ptr(key_of(b"pos_fact_again"));
        for o in objs.iter().step_by(3) {
            o.with_mut_ptr(|p| {
                again.with_const_ptr(|kp| crate::object::js_object_set_field_by_name(p, kp, 3.0))
            });
        }
        // Accessor keys: the summary is an input of the bit.
        let acc = scope.root_string_ptr(key_of(b"pos_fact_acc"));
        for o in objs.iter().skip(2).step_by(3) {
            let (ov, kv) = (
                o.with_const_ptr(|p: *const ObjectHeader| {
                    crate::value::js_nanbox_pointer(p as i64)
                }),
                acc.with_const_ptr(|p: *const crate::StringHeader| {
                    crate::value::js_nanbox_string(p as i64)
                }),
            );
            let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
            crate::object::js_object_define_accessor(ov, kv, undef, undef);
        }
        let (n, positional, accessor, bad) = crate::object::shapes::test_positional_census();
        assert!(
            positional > 0 && positional < n,
            "premise: the slab holds both answerable and unanswerable shapes ({positional} of {n})"
        );
        assert!(
            accessor > 0,
            "premise: an ordinary shape with an accessor key was minted"
        );
        assert!(
            bad.is_empty(),
            "INVARIANT: the stored positional bit equals its definition; {} of {n} records disagree: {bad:x?}",
            bad.len()
        );
    }

    /// A plain own data read that has never primed: the entry must fall all the
    /// way through to the miss handler, answer the field, and leave the site
    /// primed exactly as the old `js_object_get_field_ic_miss_packed` edge did.
    #[test]
    fn plain_miss_answers_the_field_and_primes_both_caches() {
        let _lock = crate::gc::global_side_table_test_lock();
        let scope = crate::gc::RuntimeHandleScope::new();
        let obj = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 8));
        let key = scope.root_string_ptr(key_of(b"ic_slow_plain"));
        obj.with_mut_ptr(|o| {
            key.with_const_ptr(|k| crate::object::js_object_set_field_by_name(o, k, 7.0))
        });

        let mut cache: PicCache = [0; PIC_CACHE_WORDS];
        let mut slot: PicCacheSlot = &mut cache;
        let packed = AtomicU64::new(0);
        let v = obj.with_mut_ptr(|o: *mut ObjectHeader| {
            key.with_const_ptr(|k| js_object_get_field_ic_slow(handle(o), k, &mut slot, &packed))
        });
        assert_eq!(v, 7.0);
        assert_ne!(cache[0], 0, "the full cache must be primed by the miss");
        assert_eq!(cache[1], 0, "the first own data field lives in slot 0");
        let word = packed.load(Ordering::Relaxed);
        assert_ne!(word, 0, "the compact MRU must be published too");
        assert_eq!(word >> 32, 0, "slot 0, with no overflow bit");
    }

    /// An SSO receiver can never be dereferenced as an ObjectHeader. `.length`
    /// is served from the NaN-box payload; any other key answers undefined.
    #[test]
    fn sso_receiver_routes_to_the_by_name_helper() {
        let _lock = crate::gc::global_side_table_test_lock();
        let scope = crate::gc::RuntimeHandleScope::new();
        let sso = crate::value::JSValue::try_short_string(b"hey").unwrap();
        let key = scope.root_string_ptr(key_of(b"length"));
        let v = key.with_const_ptr(|k| js_object_get_field_ic_nonptr(sso.bits() as i64, k, 0));
        assert_eq!(v, 3.0, "the SSO length byte");
        let other = scope.root_string_ptr(key_of(b"nope_not_here"));
        let v = other.with_const_ptr(|k| js_object_get_field_ic_nonptr(sso.bits() as i64, k, 0));
        assert_eq!(v.to_bits(), crate::value::TAG_UNDEFINED);
    }

    /// The overflow arm, both directions.
    ///
    /// A field past the inline region primes with `IC_SLOT_OVERFLOW_BIT`
    /// (#9287), which is the exact state that sends the emitted MRU hit here
    /// instead of doing the `obj + header + slot*8` arithmetic — that address
    /// is not where the value lives. The positive direction is the spill read;
    /// the negative one is the tombstone, where this entry must fall back to
    /// the full miss handler rather than answer the hole.
    #[test]
    fn an_overflow_slot_reads_through_the_spill_and_falls_back_when_tombstoned() {
        let _lock = crate::gc::global_side_table_test_lock();
        let scope = crate::gc::RuntimeHandleScope::new();
        let obj = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
        // INLINE_SLOT_FLOOR is 2, so the fourth key is past the inline region.
        let mut keys = Vec::new();
        for (i, name) in [
            b"ic_slow_ovf_a".as_slice(),
            b"ic_slow_ovf_b".as_slice(),
            b"ic_slow_ovf_c".as_slice(),
            b"ic_slow_ovf_d".as_slice(),
        ]
        .iter()
        .enumerate()
        {
            let k = scope.root_string_ptr(key_of(name));
            obj.with_mut_ptr(|o| {
                k.with_const_ptr(|kp| crate::object::js_object_set_field_by_name(o, kp, i as f64))
            });
            keys.push(k);
        }
        let key = keys.last().expect("four keys");

        let mut cache: PicCache = [0; PIC_CACHE_WORDS];
        let mut slot: PicCacheSlot = &mut cache;
        let packed = AtomicU64::new(0);
        let read = |slot: &mut PicCacheSlot, packed: &AtomicU64| {
            obj.with_mut_ptr(|o: *mut ObjectHeader| {
                key.with_const_ptr(|k| js_object_get_field_ic_slow(handle(o), k, slot, packed))
            })
        };
        assert_eq!(read(&mut slot, &packed), 3.0, "the priming read");
        let word = packed.load(Ordering::Relaxed);
        assert_ne!(word, 0, "test premise: the site primed");
        let (_, _, is_spill) = super::super::ic_miss::packed_get_decode(word)
            .expect("test premise: the priming read published a compact entry");
        assert!(
            is_spill,
            "test premise: the fourth field must live past the inline region, \
             or this test never reaches the overflow arm (packed word {word:#x})"
        );
        // The hit the emitted code now delegates: same packed pair, read
        // through `overflow_get`.
        assert_eq!(read(&mut slot, &packed), 3.0, "the overflow-arm read");

        // Tombstone it. The spill slot no longer holds a value, so the arm must
        // decline and let the full miss handler answer for the prototype chain.
        obj.with_mut_ptr(|o: *mut ObjectHeader| {
            key.with_const_ptr(|k| {
                assert_eq!(crate::object::js_object_delete_field(o, k), 1);
            })
        });
        assert_eq!(
            read(&mut slot, &packed).to_bits(),
            crate::value::TAG_UNDEFINED,
            "a tombstoned overflow slot must miss, not answer the hole"
        );
    }

    // ------------------------------------------------------------------
    // S5: the ShapeId alone proves where a spill-located key's value lives.
    //
    // The emitted `pic.spill.hit` loads `meta -> spill -> [index]` with no
    // null, bound or hole test, so every carrier of a shape that has a key at
    // a spill position must have storage there. These tests hold the
    // producers that used to break that, and the read-side changes.
    // ------------------------------------------------------------------

    /// An object with two inline slots (`INLINE_SLOT_FLOOR`) and the given
    /// keys written by name, in order; keys from the third on live in spill.
    fn spilled_object<'s>(
        scope: &'s crate::gc::RuntimeHandleScope,
        names: &[&str],
        value: impl Fn(usize) -> f64,
    ) -> crate::gc::RuntimeHandle<'s> {
        let obj = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
        for (i, name) in names.iter().enumerate() {
            let key = scope.root_string_ptr(key_of(name.as_bytes()));
            obj.with_mut_ptr(|o| {
                key.with_const_ptr(|kp| crate::object::js_object_set_field_by_name(o, kp, value(i)))
            });
        }
        obj
    }

    fn live_slots(obj: &crate::gc::RuntimeHandle<'_>) -> u32 {
        obj.with_mut_ptr(|o: *mut ObjectHeader| unsafe { crate::object::object_live_slot_count(o) })
    }

    fn present(obj: &crate::gc::RuntimeHandle<'_>, index: usize) -> Option<u64> {
        obj.with_mut_ptr(|o: *mut ObjectHeader| crate::object::spill_get_present(o as usize, index))
    }

    /// `Object.defineProperty` with no `value` (and every other keys-only
    /// claim) at a spill position must give the key storage holding
    /// `undefined` — the same state a data write of `undefined` leaves.
    #[test]
    fn a_keys_only_claim_at_a_spill_position_reserves_storage() {
        let _lock = crate::gc::global_side_table_test_lock();
        let scope = crate::gc::RuntimeHandleScope::new();
        let obj = spilled_object(&scope, &["s5_ka", "s5_kb", "s5_kc"], |i| i as f64);
        assert_eq!(live_slots(&obj), 2, "test premise: two inline slots");
        assert!(present(&obj, 3).is_none(), "test premise: nothing at 3 yet");
        let claimed = scope.root_string_ptr(key_of(b"s5_kd"));
        obj.with_mut_ptr(|o| {
            claimed.with_const_ptr(|k| unsafe { crate::object::ensure_key_in_keys_array(o, k) })
        });
        assert_eq!(
            present(&obj, 3),
            Some(crate::value::TAG_UNDEFINED),
            "a keys-only claim at spill position 3 must reserve storage"
        );
        // ...and a value written later lands in that same storage.
        obj.with_mut_ptr(|o| {
            claimed.with_const_ptr(|k| crate::object::js_object_set_field_by_name(o, k, 9.0))
        });
        assert_eq!(present(&obj, 3), Some(9.0f64.to_bits()));
    }

    /// A keys list installed wholesale (`js_object_set_keys`, perry-stdlib)
    /// longer than the live inline bound puts the tail at spill positions,
    /// and each of them must have storage.
    #[test]
    fn a_wholesale_keys_list_reserves_every_spill_position() {
        let _lock = crate::gc::global_side_table_test_lock();
        let scope = crate::gc::RuntimeHandleScope::new();
        let obj = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 2));
        let list = scope.root_raw_mut_ptr(crate::array::js_array_alloc(5));
        for name in ["s5_w0", "s5_w1", "s5_w2", "s5_w3", "s5_w4"] {
            let key = key_of(name.as_bytes());
            let grown = list.with_mut_ptr(|l| {
                crate::array::js_array_push(l, crate::JSValue::string_ptr(key as *mut _))
            });
            list.set_raw_mut_ptr(grown);
        }
        obj.with_mut_ptr(|o| list.with_mut_ptr(|l| crate::object::js_object_set_keys(o, l)));
        let live = live_slots(&obj) as usize;
        assert!(
            live < 5,
            "test premise: some keys past the inline bound ({live})"
        );
        for index in live..5 {
            assert_eq!(
                present(&obj, index),
                Some(crate::value::TAG_UNDEFINED),
                "spill position {index} of a wholesale keys list must have storage"
            );
        }
    }

    /// A stored `undefined` is a VALUE: the spill buffer's growth copy must
    /// carry it over instead of leaving the new slot a hole.
    #[test]
    fn a_stored_undefined_survives_spill_growth() {
        let _lock = crate::gc::global_side_table_test_lock();
        let scope = crate::gc::RuntimeHandleScope::new();
        let names: Vec<String> = (0..24).map(|i| format!("s5_g{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
        // Key 2 (the first spill position) holds `undefined`; the rest grow the
        // buffer well past its first capacity of 8.
        let obj = spilled_object(&scope, &refs, |i| if i == 2 { undef } else { i as f64 });
        assert_eq!(
            present(&obj, 2),
            Some(crate::value::TAG_UNDEFINED),
            "spill position 2 held `undefined` before growth, and must after it"
        );
        assert_eq!(present(&obj, 23), Some(23.0f64.to_bits()));
    }

    /// A spill key holding `undefined` primes the site (the legacy read
    /// reported it absent, so such a site never primed), and the published
    /// entry is the flipped one.
    #[test]
    fn a_spill_key_holding_undefined_primes_the_site() {
        let _lock = crate::gc::global_side_table_test_lock();
        let scope = crate::gc::RuntimeHandleScope::new();
        let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
        let obj = spilled_object(&scope, &["s5_ua", "s5_ub", "s5_uc"], |i| {
            if i == 2 {
                undef
            } else {
                i as f64
            }
        });
        let key = scope.root_string_ptr(key_of(b"s5_uc"));
        let mut cache: PicCache = [0; PIC_CACHE_WORDS];
        let mut slot: PicCacheSlot = &mut cache;
        let packed = AtomicU64::new(PACKED_GET_EMPTY_WORD);
        let v = obj.with_mut_ptr(|o: *mut ObjectHeader| {
            key.with_const_ptr(|k| js_object_get_field_ic_slow(handle(o), k, &mut slot, &packed))
        });
        assert_eq!(v.to_bits(), crate::value::TAG_UNDEFINED);
        let word = packed.load(Ordering::Relaxed);
        assert_eq!(
            super::super::ic_miss::packed_get_decode(word).map(|(_, i, s)| (i, s)),
            Some((2, true)),
            "the site must publish the flipped spill entry for index 2 (word {word:#x})"
        );
    }

    /// A latched megamorphic site answers a SPILL-located key from the
    /// receiver's shape too (S3 answered inline keys only), with each
    /// receiver's own value — including a stored `undefined`.
    #[test]
    fn a_latched_megamorphic_site_answers_a_spill_key_from_the_shape() {
        let _lock = crate::gc::global_side_table_test_lock();
        let scope = crate::gc::RuntimeHandleScope::new();
        let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
        let objs: Vec<_> = (0..48)
            .map(|i| {
                let extra = format!("s5_mx{i}");
                spilled_object(
                    &scope,
                    &["s5_ma", "s5_mb", "s5_mk", extra.as_str()],
                    move |k| match (k, i) {
                        (2, 7) => undef,
                        (2, _) => 100.0 + i as f64,
                        _ => k as f64,
                    },
                )
            })
            .collect();
        let key = scope.root_string_ptr(key_of(b"s5_mk"));
        let mut cache: PicCache = [0; PIC_CACHE_WORDS];
        let mut slot: PicCacheSlot = &mut cache;
        let packed = AtomicU64::new(PACKED_GET_EMPTY_WORD);
        let read = |o: &crate::gc::RuntimeHandle<'_>, slot: &mut PicCacheSlot| {
            o.with_mut_ptr(|p: *mut ObjectHeader| {
                key.with_const_ptr(|k| js_object_get_field_ic_slow(handle(p), k, slot, &packed))
            })
        };
        let want = |i: usize| {
            if i == 7 {
                undef.to_bits()
            } else {
                (100.0 + i as f64).to_bits()
            }
        };
        for (i, o) in objs.iter().enumerate() {
            assert_eq!(read(o, &mut slot).to_bits(), want(i), "priming read {i}");
        }
        // A rotation whose every shape holds the key in spill never arms the
        // ways, so it never latches by itself: latch it, as a site that also
        // saw inline shapes would be.
        // SAFETY: `slot` points at `cache`, alive for the whole test.
        unsafe { (*slot)[crate::object::field_get_set::ic_miss::PIC_WAY_STATE] = -1_000_000 };
        let before = crate::object::shapes::SHAPE_ANSWERED_SPILL_READS.load(Ordering::Relaxed);
        for (i, o) in objs.iter().enumerate() {
            assert_eq!(read(o, &mut slot).to_bits(), want(i), "latched read {i}");
        }
        let answered =
            crate::object::shapes::SHAPE_ANSWERED_SPILL_READS.load(Ordering::Relaxed) - before;
        assert!(
            answered >= 47,
            "every latched spill read the word cannot serve must be answered by the shape: {answered}"
        );
    }

    /// A non-nullish, non-pointer receiver keeps the old fall-through: no
    /// throw, no cache, `undefined` unless the runtime knows the typed shape.
    #[test]
    fn plain_number_receiver_returns_undefined_without_priming() {
        let _lock = crate::gc::global_side_table_test_lock();
        let key = key_of(b"ic_slow_on_a_double");
        let v = js_object_get_field_ic_nonptr(3.5f64.to_bits() as i64, key, 0);
        assert_eq!(v.to_bits(), crate::value::TAG_UNDEFINED);
    }

    /// The nullish arm must reach `js_throw_type_error_property_access` with
    /// the key's bytes. `js_throw` exits the process when `TRY_DEPTH == 0`, so
    /// the assertion here is on the pre-throw routing decision: `undefined` and
    /// `null` are the only two bit patterns that take it, which is what the
    /// emitted `pget.throw_nullish` predicate tested.
    #[test]
    fn only_undefined_and_null_take_the_throwing_arm() {
        for bits in [crate::value::TAG_UNDEFINED, crate::value::TAG_NULL] {
            assert!(
                (bits >> 48) & 0xFFFD != 0x7FFD,
                "a nullish tag must not look like a heap pointer"
            );
        }
        // TAG_FALSE/TAG_TRUE sit in the same 0x7FFC band and must NOT throw.
        let key = key_of(b"ic_slow_on_a_bool");
        let v = js_object_get_field_ic_nonptr(crate::value::TAG_TRUE as i64, key, 0);
        assert_eq!(v.to_bits(), crate::value::TAG_UNDEFINED);
    }

    /// A hole in the primed inline slot (the field was deleted) must take the
    /// ordinary miss and answer what the full lookup answers — never the raw
    /// `TAG_HOLE` word.
    #[test]
    fn a_deleted_inline_slot_takes_the_ordinary_miss() {
        let _lock = crate::gc::global_side_table_test_lock();
        let scope = crate::gc::RuntimeHandleScope::new();
        let obj = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 8));
        let key = scope.root_string_ptr(key_of(b"ic_slow_deleted"));
        obj.with_mut_ptr(|o| {
            key.with_const_ptr(|k| crate::object::js_object_set_field_by_name(o, k, 5.0))
        });

        let mut cache: PicCache = [0; PIC_CACHE_WORDS];
        let mut slot: PicCacheSlot = &mut cache;
        let packed = AtomicU64::new(0);
        let first = obj.with_mut_ptr(|o: *mut ObjectHeader| {
            key.with_const_ptr(|k| js_object_get_field_ic_slow(handle(o), k, &mut slot, &packed))
        });
        assert_eq!(first, 5.0);
        assert_ne!(packed.load(Ordering::Relaxed), 0, "test premise: primed");

        obj.with_mut_ptr(|o: *mut ObjectHeader| {
            key.with_const_ptr(|k| {
                assert_eq!(crate::object::js_object_delete_field(o, k), 1);
            })
        });
        let after = obj.with_mut_ptr(|o: *mut ObjectHeader| {
            key.with_const_ptr(|k| js_object_get_field_ic_slow(handle(o), k, &mut slot, &packed))
        });
        assert_eq!(
            after.to_bits(),
            crate::value::TAG_UNDEFINED,
            "a deleted field reads undefined, not the hole word"
        );
    }

    /// S6: cache word 2 is not site state any more. A word 2 that matches the
    /// receiver's ObjectMeta named-prefix token used to serve cache word 1's
    /// slot WITHOUT the receiver's ShapeId matching anything the site holds.
    /// Now only a `(ShapeId, slot)` pair can serve a read, so this plants a
    /// matching token beside a WRONG slot and requires the receiver's own
    /// value: under the old arm the read returns the wrong slot's value.
    #[test]
    fn a_matching_named_prefix_token_does_not_serve_a_read() {
        let _lock = crate::gc::global_side_table_test_lock();
        let scope = crate::gc::RuntimeHandleScope::new();
        let obj = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 8));
        let first = scope.root_string_ptr(key_of(b"ic_slow_prefix_first"));
        let key = scope.root_string_ptr(key_of(b"ic_slow_prefix"));
        obj.with_mut_ptr(|o| {
            first.with_const_ptr(|k| crate::object::js_object_set_field_by_name(o, k, 99.0))
        });
        obj.with_mut_ptr(|o| {
            key.with_const_ptr(|k| crate::object::js_object_set_field_by_name(o, k, 11.0))
        });
        // Give the receiver a meta record and an arbitrary prefix token.
        const TOKEN: u64 = 0xA11CE;
        // Collections are suppressed around the allocating meta mint, so the
        // receiver cannot move while the scoped pointer is live.
        let meta = obj.with_mut_ptr(|o: *mut ObjectHeader| {
            let _no_gc = crate::gc::GcSuppressScope::new();
            unsafe { crate::object::object_meta_ensure(o) }
        });
        assert!(
            !meta.is_null(),
            "test premise: the receiver has a meta record"
        );
        unsafe { (*meta).array_subclass_named_prefix_token = TOKEN };

        let mut cache: PicCache = [0; PIC_CACHE_WORDS];
        // Word 2 as the retired arm read it, matching the receiver's token,
        // beside the WRONG slot: slot 0 holds `ic_slow_prefix_first` (99).
        // Word 0 stays 0 and `packed` stays 0, so no `(ShapeId, slot)` pair
        // names this receiver.
        cache[2] = TOKEN as i64;
        cache[1] = 0;
        let mut slot: PicCacheSlot = &mut cache;
        let packed = AtomicU64::new(0);
        let v = obj.with_mut_ptr(|o: *mut ObjectHeader| {
            key.with_const_ptr(|k| js_object_get_field_ic_slow(handle(o), k, &mut slot, &packed))
        });
        assert_eq!(
            v, 11.0,
            "the receiver's own value, not slot 0's (99): word 2 must not serve"
        );
        assert_ne!(
            packed.load(Ordering::Relaxed),
            0,
            "the read reached the priming miss handler, which primed the \
             receiver's own (ShapeId, slot)"
        );
    }

    /// A site whose cache slot has not been resolved yet reads as a null
    /// cache, and the named-prefix arm must not dereference it.
    ///
    /// The read is of a key the receiver does NOT own, so the miss handler
    /// answers from the prototype chain without priming — which is what keeps
    /// this test on the arm it is about. (Codegen always passes the address of
    /// a real `@perry_ic_N` global; a genuinely null slot only ever reaches a
    /// runtime entry from a test or from the write PIC's poly tail.)
    #[test]
    fn an_unresolved_cache_slot_is_never_dereferenced() {
        let _lock = crate::gc::global_side_table_test_lock();
        let scope = crate::gc::RuntimeHandleScope::new();
        let obj = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 8));
        let own = scope.root_string_ptr(key_of(b"ic_slow_present"));
        obj.with_mut_ptr(|o| {
            own.with_const_ptr(|k| crate::object::js_object_set_field_by_name(o, k, 3.0))
        });
        let absent = scope.root_string_ptr(key_of(b"ic_slow_absent"));
        let packed = AtomicU64::new(0);
        // A never-published slot: nothing may read a cache word out of it.
        let mut slot: PicCacheSlot = std::ptr::null_mut();
        let v = obj.with_mut_ptr(|o: *mut ObjectHeader| {
            absent.with_const_ptr(|k| js_object_get_field_ic_slow(handle(o), k, &mut slot, &packed))
        });
        assert_eq!(v.to_bits(), crate::value::TAG_UNDEFINED);
        assert!(slot.is_null(), "an absent key must not resolve a cache");
        assert_eq!(packed.load(Ordering::Relaxed), 0, "and must not prime");
    }
}
