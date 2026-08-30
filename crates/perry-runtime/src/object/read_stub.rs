//! Megamorphic stub cache for dynamic string-keyed property READS.
//!
//! The read twin of the dynamic-write stub in `proxy::put_value`, and it exists
//! for the same reason: a site that rotates more keys than a per-site cache can
//! hold gets no benefit from one, so the cache has to be keyed on the PROGRAM's
//! live `(shape, key)` pairs instead of on a site.
//!
//! What a hit skips is the point. `js_object_get_field_by_name`'s fast lane
//! re-proves a long chain on every read — address-class checks, the interned-key
//! flag, arena classification, header type/flags/class, keys-array validation —
//! and then consults the read-plan cache, whose epoch is bumped by the
//! incremental collector at loop-poll cadence, so on a plain read loop it is
//! repeatedly cold and falls through to a shape-index hash lookup. A stub hit
//! replaces all of that with a handful of loads and compares.
//!
//! # Why entries cannot go stale dangerously
//!
//! Every hit re-validates the receiver's CURRENT state: heap-object type, not
//! forwarded, none of the blocking flags, a real class id, and — decisively —
//! the receiver's current shape token. The token identifies the exact key set
//! AND order, so a matching token means the cached slot still names this key.
//! A stale entry therefore misses; it cannot resolve to the wrong property.
//!
//! Entries hold no roots and no addresses: the key is stored as CONTENT bits
//! (an SSO immediate, or a short ASCII heap string folded to the bits its
//! content would encode as), so a key that dies and has its address recycled
//! cannot produce a false hit. Keys that do not fit the inline form are simply
//! not cached — the same rule the write stub follows, and for the same reason.
//!
//! # Two ways, not one
//!
//! Direct-mapped was measured on the write side and it is a trap: a colliding
//! pair evicts each other on every rotation through the key set, so both miss
//! FOREVER — the miss is permanent, not probabilistic. Making that table 2-way
//! at equal capacity was worth 50% on the write loop (#8977). This one starts
//! 2-way for that reason.

use crate::object::ObjectHeader;

const READ_STUB_BUCKETS: usize = 2048;
const READ_STUB_ASSOC: usize = 2;

crate::perry_thread_local! {
    static READ_STUB: [[std::cell::Cell<(u64, u64, u64)>; READ_STUB_ASSOC]; READ_STUB_BUCKETS] =
        std::array::from_fn(|_| std::array::from_fn(|_| std::cell::Cell::new((0, 0, 0))));
}

/// Content bits for a key, or `None` when it must not be cached.
///
/// Only keys representable inline are admitted, so an entry names a STRING
/// VALUE and never an address. See the module note on staleness.
#[inline(always)]
pub(crate) fn read_stub_key_bits(key: *const crate::StringHeader) -> Option<u64> {
    unsafe { crate::string::short_ascii_sso_bits(key) }
}

#[inline(always)]
fn bucket_of(token: u64, key_bits: u64) -> usize {
    // Multiplicative mixing taking the TOP bits of the product. An SSO key's
    // LOW bits are its first byte, so a low-bit index collapses a whole key
    // family onto a few buckets — measured on the write side before #8977.
    let h = (token ^ key_bits).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    ((h >> 40) as usize) & (READ_STUB_BUCKETS - 1)
}

#[inline(always)]
pub(crate) fn read_stub_probe(token: u64, key_bits: u64) -> Option<u32> {
    READ_STUB.with(|t| {
        for way in t[bucket_of(token, key_bits)].iter() {
            let (tok, kb, slot) = way.get();
            if tok == token && kb == key_bits && tok != 0 {
                return Some(slot as u32);
            }
        }
        None
    })
}

#[inline(always)]
pub(crate) fn read_stub_insert(token: u64, key_bits: u64, slot: u32) {
    if token == 0 || key_bits == 0 {
        return;
    }
    READ_STUB.with(|t| {
        let bucket = &t[bucket_of(token, key_bits)];
        let entry = (token, key_bits, slot as u64);
        for way in bucket.iter() {
            let (tok, kb, _) = way.get();
            if tok == token && kb == key_bits {
                way.set(entry);
                return;
            }
        }
        for way in bucket.iter() {
            if way.get().0 == 0 {
                way.set(entry);
                return;
            }
        }
        for i in (1..READ_STUB_ASSOC).rev() {
            bucket[i].set(bucket[i - 1].get());
        }
        bucket[0].set(entry);
    });
}

/// The receiver's shape token, or `None` when it has no live shape.
///
/// Same discriminated form the write ICs use, so the two caches agree on what
/// "this shape" means.
#[inline(always)]
pub(crate) unsafe fn receiver_shape_token(obj: *const ObjectHeader) -> Option<u64> {
    let stamp = crate::object::shapes::object_shape_stamp(obj);
    if stamp == 0 {
        return None;
    }
    Some(crate::object::shapes::PIC_ID_TOKEN_BIT | stamp as u64)
}

/// Resolve an own data slot straight from an SSO key's CONTENT bits, without
/// building a `StringHeader` for it at all.
///
/// The computed-read lowering hands the key to `js_get_string_pointer_unified`
/// before calling the by-name entry, because that entry's signature wants a
/// `*const StringHeader`. For an SSO key that means materialising inline bytes
/// onto the heap — an intern hash and table probe — on EVERY read, purely to
/// satisfy a pointer signature. On the combined overwrite loop
/// `intern_dispatch_bytes` is 5.5% of self time, all of it that.
///
/// Validation is the read stub's usual one, so this can only answer for a
/// receiver the stub was primed from: heap-object type, not forwarded, no
/// blocking flags, a real class id, and the receiver's CURRENT shape token,
/// which pins the key set and order. Anything else returns `None` and the
/// caller takes its normal route.
///
/// # Safety
/// `obj` must be a plausible heap address or null; nothing is dereferenced
/// before the GC header read classifies it.
pub(crate) unsafe fn try_read_by_content_bits(
    obj: *const ObjectHeader,
    key_bits: u64,
) -> Option<f64> {
    if obj.is_null() {
        return None;
    }
    let addr = obj as usize;
    let gc = crate::value::addr_class::try_read_gc_header(addr)?;
    const STUB_BLOCKING: u16 =
        crate::gc::OBJ_FLAG_HAS_DESCRIPTORS | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO;
    if gc.obj_type != crate::gc::GC_TYPE_OBJECT
        || gc.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || gc._reserved & STUB_BLOCKING != 0
    {
        return None;
    }
    let class_id = (*obj).class_id;
    if class_id == 0 || class_id == crate::object::NATIVE_MODULE_CLASS_ID {
        return None;
    }
    let token = receiver_shape_token(obj)?;
    let slot = read_stub_probe(token, key_bits)?;
    read_slot_by_tag(obj, addr, slot)
}

/// Read the value a bit-tagged cached slot names. The inline/overflow verdict
/// was decided at prime time (`IC_SLOT_OVERFLOW_BIT`, see `proxy::put_value`)
/// under the exact shape id the caller's token match just re-proved, so no
/// bound is fetched here. #9064's private stable-tombstone state is the one
/// mutable shape epoch: its deleted slot contains `TAG_HOLE`, which must miss
/// so the ordinary lookup can continue through the prototype chain.
#[inline(always)]
pub(crate) unsafe fn read_slot_by_tag(
    obj: *const ObjectHeader,
    addr: usize,
    slot: u32,
) -> Option<f64> {
    use crate::proxy::IC_SLOT_OVERFLOW_BIT;
    if slot & IC_SLOT_OVERFLOW_BIT != 0 {
        return crate::object::overflow_get(addr, (slot & !IC_SLOT_OVERFLOW_BIT) as usize)
            .filter(|&bits| bits != crate::value::TAG_HOLE)
            .map(f64::from_bits);
    }
    let fields_ptr =
        (obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const crate::JSValue;
    let val = *fields_ptr.add(slot as usize);
    // Same null-POINTER_TAG guard as `js_object_get_field`'s inline half: the
    // pattern is never a legitimate stored value.
    if val.bits() == 0x7FFD_0000_0000_0000 {
        return Some(f64::from_bits(crate::value::TAG_UNDEFINED));
    }
    if val.bits() == crate::value::TAG_HOLE {
        return None;
    }
    Some(f64::from_bits(val.bits()))
}
