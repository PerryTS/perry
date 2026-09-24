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
#[no_mangle]
pub extern "C" fn js_object_get_field_ic_nonptr(
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
    let idx = index as usize;
    if let Some(v) = crate::object::overflow_get(obj as usize, idx) {
        if v != crate::value::TAG_HOLE {
            return f64::from_bits(v);
        }
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
#[no_mangle]
pub extern "C" fn js_object_get_field_ic_slow(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{PicCache, PIC_CACHE_WORDS};

    fn key_of(bytes: &[u8]) -> *const crate::StringHeader {
        crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
    }

    /// What the emitted site passes: the NaN box with its tag masked off. The
    /// tag itself was already tested inline, which is why this entry never
    /// sees it.
    fn handle(obj: *mut ObjectHeader) -> i64 {
        (obj as u64 & 0x0000_FFFF_FFFF_FFFF) as i64
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
