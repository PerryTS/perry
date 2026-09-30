//! One direct compiled class setter at a static-key PutValue site.
//!
//! The packed store's first eight words are own-data ways and word eight is
//! the key-add chain verdict. Word nine names this bounded, collecting-path
//! setter entry. The emitted leaf never reads it. A miss validates the live
//! class-prototype link, receiver and holder shapes, inline accessor slot and
//! raw setter before calling with the original receiver. Any uncertainty
//! falls through to ordinary `[[Set]]`.

use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

const SITE_TAG: u64 = 0xA2C2_0000_0000_0000;

struct Entry {
    key: usize,
    holder: usize,
    class_id: u32,
    receiver_shape: u32,
    holder_shape: u32,
    slot: u32,
    raw_set: usize,
    validity: u64,
    vtable_gen: u64,
}

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
        static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
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
    if word & !crate::value::POINTER_MASK != SITE_TAG {
        return None;
    }
    let ptr = (word & crate::value::POINTER_MASK) as usize as *mut Entry;
    (!ptr.is_null()).then_some(&mut *ptr)
}

unsafe fn class_link(recv: *const crate::ObjectHeader) -> Option<*const crate::ObjectHeader> {
    use crate::object::shapes::{
        object_proto_id, object_shape_stamp, shape_proto_id, PROTO_ID_CLASS, PROTO_ID_MIXED,
        PROTO_ID_UNIQUE,
    };
    let pid = shape_proto_id(object_shape_stamp(recv))?;
    if object_proto_id(recv) != pid {
        return None;
    }
    let holder = if (PROTO_ID_MIXED..PROTO_ID_UNIQUE).contains(&pid) {
        let meta = (*recv).meta;
        if meta.is_null() {
            return None;
        }
        let p = crate::JSValue::from_bits((*meta).prototype);
        if !p.is_pointer() {
            return None;
        }
        p.as_pointer::<crate::ObjectHeader>()
    } else if (PROTO_ID_CLASS..PROTO_ID_MIXED).contains(&pid) {
        crate::object::class_decl_prototype_object((*recv).class_id)
    } else {
        return None;
    };
    (!holder.is_null() && holder != recv).then_some(holder)
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
    if !recv_shape.object_kind.is_ordinary_layout() {
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

    let holder = class_link(recv)?;
    let holder_gc = crate::value::addr_class::try_read_gc_header(holder as usize)?;
    if holder_gc.obj_type != crate::gc::GC_TYPE_OBJECT
        || holder_gc.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || crate::object::dictionary::is_dictionary(holder)
    {
        return None;
    }
    let shape = crate::object::shapes::object_shape_descriptor(holder)?;
    if !shape.object_kind.is_ordinary_layout() {
        return None;
    }
    let keys = shape.keys as usize as *const crate::array::ArrayHeader;
    if keys.is_null() {
        return None;
    }
    let slot =
        crate::object::keys_find_slot_by_bytes_resolved(keys, shape.logical_key_count, name)?;
    if slot >= shape.live_inline_slot_count
        || crate::object::key_attrs::keys_entry(keys, slot)
            & crate::object::key_attrs::ENTRY_ACCESSOR
            == 0
    {
        return None;
    }
    let field = (holder as *const u8)
        .add(std::mem::size_of::<crate::ObjectHeader>() + slot as usize * 8)
        as *const u64;
    let acc = crate::object::accessor_pair::pair_of_value(*field)?;
    if acc.raw_set == 0 {
        return None;
    }
    // The direct declared pair is the only admitted route. A registered
    // ancestor or a closure-backed replacement keeps the generic walk.
    let name_str = std::str::from_utf8(name).ok()?;
    if !crate::object::class_chain_has_instance_accessor(class_id, name_str) {
        return None;
    }
    Some(Entry {
        key: key as usize,
        holder: holder as usize,
        class_id,
        receiver_shape: crate::object::shapes::object_shape_stamp(recv),
        holder_shape: crate::object::shapes::object_shape_stamp(holder),
        slot,
        raw_set: acc.raw_set,
        validity: crate::object::proto_validity::proto_validity(),
        vtable_gen: crate::object::vtable_generation(),
    })
}

unsafe fn validated_raw_set(
    e: &Entry,
    recv: *const crate::ObjectHeader,
    key: *const crate::StringHeader,
) -> Option<usize> {
    let gc = crate::value::addr_class::try_read_gc_header(recv as usize)?;
    if gc.obj_type != crate::gc::GC_TYPE_OBJECT
        || gc.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || gc._reserved & crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO != 0
        || crate::object::dictionary::is_dictionary(recv)
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
    if e.key != key as usize
        || e.class_id != (*recv).class_id
        || e.receiver_shape != crate::object::shapes::object_shape_stamp(recv)
        || e.validity != crate::object::proto_validity::proto_validity()
        || e.vtable_gen != crate::object::vtable_generation()
        || class_link(recv)? as usize != e.holder
        || crate::object::shapes::object_shape_stamp(e.holder as *const crate::ObjectHeader)
            != e.holder_shape
    {
        return None;
    }
    let holder_gc = crate::value::addr_class::try_read_gc_header(e.holder)?;
    if holder_gc.obj_type != crate::gc::GC_TYPE_OBJECT
        || holder_gc.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return None;
    }
    let field = (e.holder as *const u8)
        .add(std::mem::size_of::<crate::ObjectHeader>() + e.slot as usize * 8)
        as *const u64;
    let acc = crate::object::accessor_pair::pair_of_value(*field)?;
    (acc.raw_set == e.raw_set && acc.raw_set != 0).then_some(acc.raw_set)
}

unsafe fn invoke(raw_set: usize, target: f64, value: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let recv_h = scope.root_nanbox_f64(target);
    let value_h = scope.root_nanbox_f64(value);
    let f = crate::closure::body_call::js_method_body_fn!(raw_set as *const u8; value);
    let _ = f(recv_h.get_nanbox_f64(), value_h.get_nanbox_f64());
    value_h.get_nanbox_f64()
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
    let bits = target.to_bits();
    if bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG {
        return None;
    }
    let addr = (bits & crate::value::POINTER_MASK) as usize;
    if !crate::value::addr_class::is_above_handle_band(addr) {
        return None;
    }
    let recv = addr as *const crate::ObjectHeader;
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
    let cache = packed_set_cache_resolve(slot);
    if !cache.is_null() {
        if let Some(e) = entry(slot) {
            *e = fresh;
        } else {
            let ptr = Box::into_raw(Box::new(fresh));
            ENTRIES.with(|cell| (*cell.get()).push(ptr));
            let addr = ptr as usize as u64;
            assert_eq!(addr & !crate::value::POINTER_MASK, 0);
            (*cache)[PACKED_SET_SETTER_WORD] = SITE_TAG | addr;
        }
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
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static FIRST: AtomicU32 = AtomicU32::new(0);
    static SECOND: AtomicU32 = AtomicU32::new(0);

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
    fn direct_setter_rechecks_same_shape_relink_own_shadow_and_worker_gate() {
        let _lock = crate::gc::global_side_table_test_lock();
        const CID: u32 = 0x0C3C_79B5;
        let before_gate = crate::object::method_site::WORKER_AGENTS_EXIST.swap(0, Ordering::SeqCst);
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
        let cache: &'static mut PackedSetWays = Box::leak(Box::new(packed_set_cache_empty()));
        assert_eq!(cache[PACKED_SET_SETTER_WORD], 0);
        let mut slot: PackedSetWaysSlot = cache;
        let target = recv.with_const_ptr::<crate::ObjectHeader, _>(|p| {
            crate::value::js_nanbox_pointer(p as i64)
        });
        let key_ptr = key.get_raw_const_ptr::<crate::StringHeader>();
        assert_eq!(
            unsafe { try_set(&mut slot, target, key_ptr, 5.0) },
            Some(5.0)
        );
        assert_eq!(
            unsafe { try_set(&mut slot, target, key_ptr, 6.0) },
            Some(6.0)
        );
        assert_eq!(FIRST.load(Ordering::Relaxed), 2);
        p2.with_const_ptr::<crate::ObjectHeader, _>(|p| {
            crate::object::test_seed_class_decl_prototype_object_root(CID, p as usize)
        });
        assert_eq!(
            unsafe {
                validated_raw_set(entry(&mut slot).unwrap(), recv.get_raw_const_ptr(), key_ptr)
            },
            None
        );
        assert_eq!(
            unsafe { try_set(&mut slot, target, key_ptr, 7.0) },
            Some(7.0)
        );
        assert_eq!(SECOND.load(Ordering::Relaxed), 1);
        crate::object::method_site::WORKER_AGENTS_EXIST.store(1, Ordering::SeqCst);
        assert_eq!(unsafe { try_set(&mut slot, target, key_ptr, 8.0) }, None);
        crate::object::method_site::WORKER_AGENTS_EXIST.store(before_gate, Ordering::SeqCst);
        recv.with_mut_ptr::<crate::ObjectHeader, _>(|p| unsafe {
            crate::object::object_ops::define_property_force_store_value(p, key_ptr, 99.0);
        });
        assert_ne!(
            recv.with_const_ptr::<crate::ObjectHeader, _>(|p| unsafe {
                crate::object::shapes::object_shape_stamp(p)
            }),
            recv_shape
        );
        assert_eq!(
            unsafe {
                validated_raw_set(entry(&mut slot).unwrap(), recv.get_raw_const_ptr(), key_ptr)
            },
            None
        );
    }
}
