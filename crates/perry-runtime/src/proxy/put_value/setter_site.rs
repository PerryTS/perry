//! One compiled class setter at a static-key PutValue site.
//!
//! The packed store's first eight words are own-data ways and word eight is
//! the key-add chain verdict. Word nine names this bounded, collecting-path
//! setter entry. The emitted leaf never reads it.
//!
//! A hit is shape facts and the lane's own value, nothing global:
//! * the receiver's ShapeId proves the key is not own and names the
//!   receiver's prototype identity, hence the holder (a recorded serial, or a
//!   bare class whose registry link retires the holder's ShapeId if it is ever
//!   replaced: `class_registry::retire_displaced_decl_prototype`);
//! * each intermediate ShapeId proves absence and pins the next holder;
//! * the holder's ShapeId proves the key's slot is still an accessor lane;
//! * the lane still holds the primed pair, which names the compiled setter.
//!
//! Any uncertainty falls through to ordinary `[[Set]]`, which re-primes.

use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

const SITE_TAG: u64 = crate::codegen_abi::SETTER_SITE_TAG;
// The emitted arm admits only SITE_TAG; deep entries take the collecting
// hit, which can validate every intermediate ShapeId without changing ABI.
const DEEP_SITE_TAG: u64 = SITE_TAG ^ (1 << 48);
const _: () = assert!(PACKED_SET_SETTER_WORD == crate::codegen_abi::PACKED_SET_SETTER_WORD);
const _: () = assert!(crate::value::POINTER_MASK == crate::codegen_abi::SETTER_SITE_ADDRESS_MASK);

/// The site's entry. The emitted store tower reads it at the offsets
/// `perry_abi::SETTER_SITE_*_OFFSET` pin (`setter_arm.rs` in codegen).
#[repr(C)]
struct Entry {
    receiver_shape: u32,
    holder_shape: u32,
    /// The holder (a strong root, rewritten on move).
    holder: usize,
    slot: u32,
    /// The pair the holder's lane held at prime time: its raw address, a
    /// strong root rewritten on move, so a hit compares one loaded word.
    pair: usize,
    /// The compiled setter the pair names.
    raw_set: usize,
    /// The interned key (a strong root).
    key: usize,
    /// Every intermediate holder's shape pins absence and the next link.
    hops: [(usize, u32); crate::object::method_site::read_holder::HOLDER_MAX_DEPTH - 1],
    depth: usize,
}
const _: () = {
    use crate::codegen_abi as abi;
    assert!(std::mem::offset_of!(Entry, receiver_shape) == abi::SETTER_SITE_RECV_SHAPE_OFFSET);
    assert!(std::mem::offset_of!(Entry, holder_shape) == abi::SETTER_SITE_HOLDER_SHAPE_OFFSET);
    assert!(std::mem::offset_of!(Entry, holder) == abi::SETTER_SITE_HOLDER_OFFSET);
    assert!(std::mem::offset_of!(Entry, slot) == abi::SETTER_SITE_SLOT_OFFSET);
    assert!(std::mem::offset_of!(Entry, pair) == abi::SETTER_SITE_PAIR_OFFSET);
    assert!(std::mem::offset_of!(Entry, raw_set) == abi::SETTER_SITE_CODE_OFFSET);
};

crate::perry_thread_local! {
    static ENTRIES: std::cell::UnsafeCell<Vec<*mut Entry>> =
        const { std::cell::UnsafeCell::new(Vec::new()) };
}

static HITS: AtomicU64 = AtomicU64::new(0);
static PRIMES: AtomicU64 = AtomicU64::new(0);
static ROOT_REWRITES: AtomicU64 = AtomicU64::new(0);

fn stats_enabled() -> bool {
    #[cfg(test)]
    {
        true
    }
    #[cfg(not(test))]
    {
        per_test_global! {
            static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        }
        *ON.get_or_init(|| {
            let on = std::env::var_os("PERRY_SETTER_SITE_STATS").is_some();
            if on {
                extern "C" fn report() {
                    eprintln!(
                        "[setter-site] primes={} hits={} root_rewrites={}",
                        PRIMES.load(Ordering::Relaxed),
                        HITS.load(Ordering::Relaxed),
                        ROOT_REWRITES.load(Ordering::Relaxed),
                    );
                }
                unsafe { libc::atexit(report) };
            }
            on
        })
    }
}

#[inline]
fn primary_only() -> bool {
    crate::object::method_site::WORKER_AGENTS_EXIST.load(Ordering::SeqCst) == 0
        && crate::agent::current_agent() == crate::agent::PRIMARY_AGENT
}

unsafe fn entry(slot: *mut PackedSetWaysSlot) -> Option<&'static mut Entry> {
    if slot.is_null() {
        return None;
    }
    let cache = crate::object::pic_slot_peek(slot);
    if cache.is_null() {
        return None;
    }
    let word = (*cache)[PACKED_SET_SETTER_WORD];
    let tag = word & !crate::value::POINTER_MASK;
    if tag != SITE_TAG && tag != DEEP_SITE_TAG {
        return None;
    }
    let ptr = (word & crate::value::POINTER_MASK) as usize as *mut Entry;
    (!ptr.is_null()).then_some(&mut *ptr)
}

unsafe fn candidate(
    recv: *const crate::ObjectHeader,
    key: *const crate::StringHeader,
) -> Option<Entry> {
    if key.is_null() || !crate::value::addr_class::is_above_handle_band(key as usize) {
        return None;
    }
    let key_gc = crate::value::addr_class::try_read_gc_header(key as usize)?;
    if key_gc.obj_type != crate::gc::GC_TYPE_STRING
        || key_gc.gc_flags & (crate::gc::GC_FLAG_FORWARDED | crate::gc::GC_FLAG_INTERNED)
            != crate::gc::GC_FLAG_INTERNED
    {
        return None;
    }
    let name = crate::string::header_str_checked(key)?.as_bytes();
    if name.is_empty() || name[0] == b'#' || name[0].is_ascii_digit() {
        return None;
    }
    let gc = crate::value::addr_class::try_read_gc_header(recv as usize)?;
    if gc.obj_type != crate::gc::GC_TYPE_OBJECT
        || gc.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || gc._reserved & crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO != 0
        || crate::object::dictionary::is_dictionary(recv)
    {
        return None;
    }
    let class_id = (*recv).class_id;
    if class_id == 0
        || class_id == crate::object::NATIVE_MODULE_CLASS_ID
        || crate::object::is_anon_shape_class_id(class_id)
    {
        return None;
    }
    let meta = (*recv).meta;
    if !meta.is_null()
        && ((*meta).elements != 0
            || (*meta).flags & crate::object::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER != 0)
    {
        return None;
    }
    let recv_shape = crate::object::shapes::object_shape_descriptor(recv)?;
    if !matches!(
        recv_shape.object_kind,
        crate::object::shapes::ShapeObjectKind::Ordinary
            | crate::object::shapes::ShapeObjectKind::OrdinaryNumericProof
    ) {
        return None;
    }
    let recv_keys = recv_shape.keys as usize as *const crate::array::ArrayHeader;
    if !recv_keys.is_null()
        && crate::object::keys_find_slot_by_bytes_resolved(
            recv_keys,
            recv_shape.logical_key_count,
            name,
        )
        .is_some()
    {
        return None;
    }

    let holder = crate::object::method_site::read_holder::accessor_walk(recv, name)?;
    if holder.slot & crate::codegen_abi::PIC_HOLDER_SLOT_SPILL_BIT as u32 != 0 {
        return None;
    }
    let lane = lane_bits(holder.holder, holder.slot);
    let acc = crate::object::accessor_pair::pair_of_value(lane)?;
    if acc.raw_set == 0 {
        return None;
    }
    Some(Entry {
        key: key as usize,
        holder: holder.holder,
        receiver_shape: crate::object::shapes::object_shape_stamp(recv),
        holder_shape: holder.shape,
        slot: holder.slot,
        pair: (lane & crate::value::POINTER_MASK) as usize,
        raw_set: acc.raw_set,
        hops: holder.hops,
        depth: holder.depth - 1,
    })
}

/// The value of `holder`'s inline slot `slot`.
#[inline]
unsafe fn lane_bits(holder: usize, slot: u32) -> u64 {
    std::ptr::read(
        (holder as *const u8).add(std::mem::size_of::<crate::ObjectHeader>() + slot as usize * 8)
            as *const u64,
    )
}

/// The compiled setter `e` names for `recv`, when every fact still holds: the
/// receiver's ShapeId (key not own, prototype identity, hence the holder),
/// the holder's ShapeId (the slot is an accessor lane) and the lane's pair.
/// A ShapeId match also proves a live, non-forwarded ordinary object (#10828
/// rule 3), so nothing per-object is re-read.
#[inline]
unsafe fn validated_raw_set(
    e: &Entry,
    recv: *const crate::ObjectHeader,
    key: *const crate::StringHeader,
) -> Option<usize> {
    let stamp = crate::object::shapes::object_shape_stamp(recv);
    (stamp != 0
        && e.receiver_shape == stamp
        && e.hops[..e.depth].iter().all(|&(addr, shape)| {
            crate::object::shapes::object_shape_stamp(addr as *const crate::ObjectHeader) == shape
        })
        && e.key == key as usize
        && crate::object::shapes::object_shape_stamp(e.holder as *const crate::ObjectHeader)
            == e.holder_shape
        && lane_bits(e.holder, e.slot) == crate::value::POINTER_TAG | e.pair as u64)
        .then_some(e.raw_set)
}

/// Call the compiled setter with `target` as `this`. The store's result is
/// `value`; a Number needs no root across the call, anything else is rooted
/// (a heap value can move while the setter runs).
#[inline]
unsafe fn invoke(raw_set: usize, target: f64, value: f64) -> f64 {
    let f = crate::closure::body_call::js_method_body_fn!(raw_set as *const u8; value);
    if crate::value::JSValue::from_bits(value.to_bits()).is_number() {
        let _ = f(target, value);
        return value;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let value_h = scope.root_nanbox_f64(value);
    let _ = f(target, value);
    value_h.get_nanbox_f64()
}

/// The receiver `target` names, when it is a heap pointer the entry could
/// describe.
#[inline]
fn receiver_of(target: f64) -> Option<*const crate::ObjectHeader> {
    let bits = target.to_bits();
    if bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG {
        return None;
    }
    let addr = (bits & crate::value::POINTER_MASK) as usize;
    crate::value::addr_class::is_above_handle_band(addr)
        .then_some(addr as *const crate::ObjectHeader)
}

/// The entry's hit and nothing else: allocation-free until the setter runs,
/// and never a candidate walk, so the store-miss entry asks it before any
/// other miss work.
#[inline]
pub(super) unsafe fn try_hit(
    slot: *mut PackedSetWaysSlot,
    target: f64,
    key: *const crate::StringHeader,
    value: f64,
) -> Option<f64> {
    let e = entry(slot)?;
    if crate::object::method_site::WORKER_AGENTS_EXIST.load(Ordering::SeqCst) != 0 {
        return None;
    }
    let raw_set = validated_raw_set(e, receiver_of(target)?, key)?;
    if stats_enabled() {
        HITS.fetch_add(1, Ordering::Relaxed);
    }
    Some(invoke(raw_set, target, value))
}

/// Collecting miss only; the emitted GC-leaf store never consults this word.
pub(super) unsafe fn try_set(
    slot: *mut PackedSetWaysSlot,
    target: f64,
    key: *const crate::StringHeader,
    value: f64,
) -> Option<f64> {
    if !primary_only() || slot.is_null() || key.is_null() {
        return None;
    }
    let recv = receiver_of(target)?;
    if let Some(e) = entry(slot) {
        if let Some(raw_set) = validated_raw_set(e, recv, key) {
            if stats_enabled() {
                HITS.fetch_add(1, Ordering::Relaxed);
            }
            return Some(invoke(raw_set, target, value));
        }
    }
    let fresh = candidate(recv, key)?;
    let raw_set = fresh.raw_set;
    let tag = if fresh.depth == 0 {
        SITE_TAG
    } else {
        DEEP_SITE_TAG
    };
    let cache = packed_set_cache_resolve(slot);
    if !cache.is_null() {
        let addr = if let Some(e) = entry(slot) {
            *e = fresh;
            e as *mut Entry as usize as u64
        } else {
            let ptr = Box::into_raw(Box::new(fresh));
            ENTRIES.with(|cell| (*cell.get()).push(ptr));
            ptr as usize as u64
        };
        assert_eq!(addr & !crate::value::POINTER_MASK, 0);
        (*cache)[PACKED_SET_SETTER_WORD] = tag | addr;
        if stats_enabled() {
            PRIMES.fetch_add(1, Ordering::Relaxed);
        }
    }
    Some(invoke(raw_set, target, value))
}

pub(crate) fn scan_roots(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    if !primary_only() {
        return;
    }
    ENTRIES.with(|cell| unsafe {
        for &ptr in (*cell.get()).iter() {
            let e = &mut *ptr;
            if visitor.visit_tagged_usize_slot(&mut e.key, crate::value::STRING_TAG) {
                ROOT_REWRITES.fetch_add(1, Ordering::Relaxed);
            }
            if visitor.visit_tagged_usize_slot(&mut e.holder, crate::value::POINTER_TAG) {
                ROOT_REWRITES.fetch_add(1, Ordering::Relaxed);
            }
            if visitor.visit_tagged_usize_slot(&mut e.pair, crate::value::POINTER_TAG) {
                ROOT_REWRITES.fetch_add(1, Ordering::Relaxed);
            }
            for (addr, _) in &mut e.hops[..e.depth] {
                if visitor.visit_tagged_usize_slot(addr, crate::value::POINTER_TAG) {
                    ROOT_REWRITES.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    per_test_global! {
        static FIRST: AtomicU32 = AtomicU32::new(0);
        static SECOND: AtomicU32 = AtomicU32::new(0);
    }

    extern "C" fn first(_recv: f64, _value: f64) -> f64 {
        FIRST.fetch_add(1, Ordering::Relaxed);
        0.0
    }
    extern "C" fn second(_recv: f64, _value: f64) -> f64 {
        SECOND.fetch_add(1, Ordering::Relaxed);
        0.0
    }
    fn fnv1a(bytes: &[u8]) -> u64 {
        bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
        })
    }

    #[test]
    fn deep_setter_rechecks_intermediate_shapes() {
        if !crate::object::method_site::run_with_fresh_worker_gate(
            "deep_setter_rechecks_intermediate_shapes",
        ) {
            return;
        }
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_gc = crate::gc::GcSuppressScope::new();
        const CID: u32 = 0x0C3C_79B6;
        unsafe { crate::object::js_register_class_id(CID) };
        FIRST.store(0, Ordering::Relaxed);
        SECOND.store(0, Ordering::Relaxed);
        let scope = crate::gc::RuntimeHandleScope::new();
        let far = crate::object::js_object_alloc(0, 1);
        let near = crate::object::js_object_alloc(0x0C3C_79B7, 1);
        crate::object::set_builtin_accessor_pair(
            far as usize,
            "points".to_owned(),
            crate::object::accessor_pair::Accessor {
                raw_set: first as *const () as usize,
                ..Default::default()
            },
            crate::object::PropertyAttrs::new(true, false, true),
        );
        crate::object::object_ops::js_object_set_prototype_of(
            crate::value::js_nanbox_pointer(near as i64),
            crate::value::js_nanbox_pointer(far as i64),
        );
        crate::object::test_seed_class_decl_prototype_object_root(CID, near as usize);
        let keys = crate::object::js_build_class_keys_array(CID, 1, b"own".as_ptr(), 3, 0);
        let recv_shape = crate::object::shapes::js_object_shape_id_for_class_keys(
            keys as usize as u64,
            1,
            CID,
            0,
        );
        let recv = crate::object::js_object_alloc_class_inline_keys_stamped(
            CID, 0, 1, keys, recv_shape, 0,
        );
        let target = crate::value::js_nanbox_pointer(recv as i64);
        let key = crate::string::js_string_from_bytes(b"points".as_ptr(), 6);
        let key = crate::string::js_string_intern(key, fnv1a(b"points"));
        let _far = scope.root_raw_mut_ptr(far);
        let _near = scope.root_raw_mut_ptr(near);
        let _recv = scope.root_raw_mut_ptr(recv);
        let _key = scope.root_string_ptr(key);
        let cache: &'static mut PackedSetWays = Box::leak(Box::new(packed_set_cache_empty()));
        let mut slot: PackedSetWaysSlot = cache;
        unsafe {
            assert_eq!(try_set(&mut slot, target, key, 5.0), Some(5.0));
            let e = entry(&mut slot).expect("deep setter entry");
            assert_eq!(e.receiver_shape, recv_shape);
            assert_eq!(e.depth, 1);
            assert_eq!(
                (*slot)[PACKED_SET_SETTER_WORD] & !crate::value::POINTER_MASK,
                DEEP_SITE_TAG,
                "the emitted direct arm must decline",
            );
            assert_eq!(try_hit(&mut slot, target, key, 6.0), Some(6.0));
            assert_eq!(FIRST.load(Ordering::Relaxed), 2);
            crate::object::set_builtin_accessor_pair(
                near as usize,
                "points".to_owned(),
                crate::object::accessor_pair::Accessor {
                    raw_set: second as *const () as usize,
                    ..Default::default()
                },
                crate::object::PropertyAttrs::new(true, false, true),
            );
            assert_eq!(try_hit(&mut slot, target, key, 7.0), None);
            assert_eq!(try_set(&mut slot, target, key, 7.0), Some(7.0));
            assert_eq!(SECOND.load(Ordering::Relaxed), 1);
            assert_eq!(entry(&mut slot).unwrap().depth, 0);
            assert_eq!(entry(&mut slot).unwrap().receiver_shape, recv_shape);
            assert_eq!(
                (*slot)[PACKED_SET_SETTER_WORD] & !crate::value::POINTER_MASK,
                SITE_TAG,
            );
        }
    }

    #[test]
    fn direct_setter_rechecks_same_shape_relink_own_shadow_and_worker_gate() {
        if !crate::object::method_site::run_with_fresh_worker_gate(
            "direct_setter_rechecks_same_shape_relink_own_shadow_and_worker_gate",
        ) {
            return;
        }
        let _lock = crate::gc::global_side_table_test_lock();
        const CID: u32 = 0x0C3C_79B5;
        FIRST.store(0, Ordering::Relaxed);
        SECOND.store(0, Ordering::Relaxed);
        unsafe {
            crate::object::js_register_class_id(CID);
            crate::object::js_register_class_setter(
                CID as i64,
                b"points".as_ptr(),
                6,
                first as *const () as i64,
                1,
            );
        }
        let scope = crate::gc::RuntimeHandleScope::new();
        let p1 = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 2));
        let p2 = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 2));
        for (holder, raw_set) in [
            (&p1, first as *const () as usize),
            (&p2, second as *const () as usize),
        ] {
            holder.with_mut_ptr::<crate::ObjectHeader, _>(|ptr| {
                crate::object::set_builtin_accessor_pair(
                    ptr as usize,
                    "points".to_owned(),
                    crate::object::accessor_pair::Accessor {
                        raw_set,
                        ..Default::default()
                    },
                    crate::object::PropertyAttrs::new(true, false, true),
                );
            });
        }
        let shape1 = p1.with_const_ptr::<crate::ObjectHeader, _>(|ptr| unsafe {
            crate::object::shapes::object_shape_stamp(ptr)
        });
        let shape2 = p2.with_const_ptr::<crate::ObjectHeader, _>(|ptr| unsafe {
            crate::object::shapes::object_shape_stamp(ptr)
        });
        assert_eq!(shape1, shape2);
        let packed = b"own";
        let keys = crate::object::js_build_class_keys_array(
            CID,
            1,
            packed.as_ptr(),
            packed.len() as u32,
            0,
        );
        let recv_shape = crate::object::shapes::js_object_shape_id_for_class_keys(
            keys as usize as u64,
            1,
            CID,
            0,
        );
        let recv =
            scope.root_raw_mut_ptr(crate::object::js_object_alloc_class_inline_keys_stamped(
                CID, 0, 1, keys, recv_shape, 0,
            ));
        let key_raw = crate::string::js_string_from_bytes(b"points".as_ptr(), 6);
        let key = scope.root_string_ptr(crate::string::js_string_intern(key_raw, fnv1a(b"points")));
        p1.with_const_ptr::<crate::ObjectHeader, _>(|p| {
            crate::object::test_seed_class_decl_prototype_object_root(CID, p as usize)
        });
        // This unit proves shape/link invalidation. The end-to-end setter
        // fixture separately proves moving-GC behavior; keep the direct raw
        // setter stubs and descriptor mutation noncollecting here.
        let _no_gc = crate::gc::GcSuppressScope::new();
        let cache: &'static mut PackedSetWays = Box::leak(Box::new(packed_set_cache_empty()));
        assert_eq!(cache[PACKED_SET_SETTER_WORD], 0);
        let mut slot: PackedSetWaysSlot = cache;
        macro_rules! call_set {
            ($value:expr) => {
                recv.with_const_ptr::<crate::ObjectHeader, _>(|recv_ptr| {
                    let target = crate::value::js_nanbox_pointer(recv_ptr as i64);
                    key.with_const_ptr::<crate::StringHeader, _>(|key_ptr| unsafe {
                        try_set(&mut slot, target, key_ptr, $value)
                    })
                })
            };
        }
        macro_rules! validated {
            () => {
                recv.with_const_ptr::<crate::ObjectHeader, _>(|recv_ptr| {
                    key.with_const_ptr::<crate::StringHeader, _>(|key_ptr| unsafe {
                        validated_raw_set(entry(&mut slot).unwrap(), recv_ptr, key_ptr)
                    })
                })
            };
        }
        assert_eq!(call_set!(5.0), Some(5.0));
        assert_eq!(call_set!(6.0), Some(6.0));
        assert_eq!(
            unsafe { entry(&mut slot).unwrap().raw_set },
            first as *const () as usize
        );
        assert_eq!(FIRST.load(Ordering::Relaxed), 2);
        p2.with_const_ptr::<crate::ObjectHeader, _>(|p| {
            crate::object::test_seed_class_decl_prototype_object_root(CID, p as usize)
        });
        assert_eq!(validated!(), None);
        assert_eq!(call_set!(7.0), Some(7.0));
        assert_eq!(SECOND.load(Ordering::Relaxed), 1);
        crate::object::method_site::WORKER_AGENTS_EXIST.store(1, Ordering::SeqCst);
        assert_eq!(call_set!(8.0), None);
        let p1_value = p1.with_const_ptr::<crate::ObjectHeader, _>(|p| {
            crate::value::js_nanbox_pointer(p as i64)
        });
        recv.with_const_ptr::<crate::ObjectHeader, _>(|p| {
            let target = crate::value::js_nanbox_pointer(p as i64);
            crate::object::object_ops::js_object_set_prototype_of(target, p1_value)
        });
        let explicit_shape = recv.with_const_ptr::<crate::ObjectHeader, _>(|p| unsafe {
            crate::object::shapes::object_shape_stamp(p)
        });
        assert_ne!(explicit_shape, recv_shape);
        assert_eq!(validated!(), None);
        recv.with_mut_ptr::<crate::ObjectHeader, _>(|p| {
            key.with_const_ptr::<crate::StringHeader, _>(|key_ptr| unsafe {
                crate::object::object_ops::define_property_force_store_value(p, key_ptr, 99.0);
            })
        });
        assert_ne!(
            recv.with_const_ptr::<crate::ObjectHeader, _>(|p| unsafe {
                crate::object::shapes::object_shape_stamp(p)
            }),
            explicit_shape
        );
        assert_eq!(validated!(), None);
    }
}
