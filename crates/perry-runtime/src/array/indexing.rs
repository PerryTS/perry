//! Indexing — length / element get / element set / hybrid string-or-index dispatch.
use super::header::{array_numeric_layout, NumericArrayLayout};
use super::*;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

const MAX_DENSE_ARRAY_GROW_LENGTH: u32 = 1_000_000;

/// Largest hole (`index - length`) an extending write may create while still
/// growing the dense backing store, once the array is past
/// `MAX_DENSE_ARRAY_GROW_LENGTH`. Sparse storage is for *jumps* far beyond the
/// current length (`a[2**32-2] = v` on a 3-element array must not allocate
/// 34 GB); sequential growth (`for (i...) arr[i] = v`, gap 0) must stay dense
/// no matter how large the array gets — routing it through string-keyed
/// property sets is quadratic and hung the 10M-element `03_array_write`
/// benchmark for 6 hours (Regression Check, v0.5.1129–v0.5.1150).
const DENSE_ARRAY_GAP_LIMIT: u32 = 1024;

/// Lazily-memoized address of the `Array.prototype` array, and a sticky flag
/// recording whether anyone has installed an indexed property on it. An
/// out-of-bounds element read on an ordinary array must fall through to
/// `Array.prototype[index]` (ECMA-262 OrdinaryGet → prototype chain), but in
/// real code nobody adds numeric indices to `Array.prototype`, so the hot OOB
/// path stays a single relaxed atomic load until the (rare) write flips the
/// flag. `usize::MAX` marks the address as not-yet-computed.
static ARRAY_PROTO_ADDR: AtomicUsize = AtomicUsize::new(usize::MAX);
static ARRAY_PROTO_HAS_INDEX: AtomicBool = AtomicBool::new(false);

/// Same idea for `Object.prototype`: a numeric index installed there
/// (`Object.prototype[2] = 2`, or a defineProperty accessor) shows through
/// array HOLES and OOB reads (chain: arr → Array.prototype →
/// Object.prototype; test262 concat/S15.4.4.4_A3_T3). Flipped by the object
/// index-write/defineProperty hooks; consulted by the typed-feedback guards
/// and the hole/OOB read fallbacks.
static OBJECT_PROTO_ADDR: AtomicUsize = AtomicUsize::new(usize::MAX);
static OBJECT_PROTO_HAS_INDEX: AtomicBool = AtomicBool::new(false);

pub(crate) fn object_prototype_addr() -> usize {
    let cached = OBJECT_PROTO_ADDR.load(Ordering::Relaxed);
    if cached != usize::MAX {
        return cached;
    }
    let ctor = crate::object::js_get_global_this_builtin_value(b"Object".as_ptr(), 6);
    let ctor_value = crate::value::JSValue::from_bits(ctor.to_bits());
    let addr = if ctor_value.is_pointer() {
        let ctor_ptr = ctor_value.as_pointer::<u8>() as usize;
        let proto = crate::closure::closure_get_dynamic_prop(ctor_ptr, "prototype");
        let proto_value = crate::value::JSValue::from_bits(proto.to_bits());
        if proto_value.is_pointer() {
            proto_value.as_pointer::<u8>() as usize
        } else {
            0
        }
    } else {
        0
    };
    // Cache only a successful resolution — an early call (before globalThis
    // init) must retry later rather than pinning 0.
    if addr != 0 {
        OBJECT_PROTO_ADDR.store(addr, Ordering::Relaxed);
    }
    addr
}

/// Record (if `obj` is the canonical `Object.prototype`) that it now carries
/// an indexed property. Called from the object index-write / numeric
/// defineProperty paths; cheap (relaxed loads + compare).
#[inline]
pub(crate) fn note_object_prototype_index_write(obj: usize) {
    if !OBJECT_PROTO_HAS_INDEX.load(Ordering::Relaxed) && obj != 0 && obj == object_prototype_addr()
    {
        OBJECT_PROTO_HAS_INDEX.store(true, Ordering::Relaxed);
    }
}

pub(crate) fn object_prototype_has_index_flag() -> bool {
    OBJECT_PROTO_HAS_INDEX.load(Ordering::Relaxed)
}

/// `true` when `addr` is the canonical `Object.prototype` (cheap: cached
/// atomic + compare; lazily computes the address on first use).
pub(crate) fn object_prototype_addr_matches(addr: usize) -> bool {
    addr != 0 && addr == object_prototype_addr()
}

/// Sticky flag: user code replaced or deleted `Array.prototype[Symbol.iterator]`.
/// `js_get_iterator`'s array short-circuit assumes the builtin values iterator;
/// once this flips, GetIterator on an array must consult the (patched) method
/// per spec — or throw TypeError when it was deleted. Same single-relaxed-load
/// hot-path shape as `ARRAY_PROTO_HAS_INDEX` above.
static ARRAY_PROTO_ITERATOR_MODIFIED: AtomicBool = AtomicBool::new(false);

/// Record (if `obj` is `Array.prototype` and `sym_key` is the well-known
/// `Symbol.iterator`) that the array iteration protocol has been tampered
/// with. Called from the symbol-property set/delete paths.
pub(crate) fn note_array_proto_iterator_write(obj: usize, sym_key: usize) {
    if ARRAY_PROTO_ITERATOR_MODIFIED.load(Ordering::Relaxed) || obj == 0 || sym_key == 0 {
        return;
    }
    if obj == array_prototype_addr()
        && sym_key == crate::symbol::well_known_symbol("iterator") as usize
    {
        ARRAY_PROTO_ITERATOR_MODIFIED.store(true, Ordering::Relaxed);
    }
}

pub(crate) fn array_proto_iterator_modified() -> bool {
    ARRAY_PROTO_ITERATOR_MODIFIED.load(Ordering::Relaxed)
}

pub(crate) fn array_prototype_addr() -> usize {
    let cached = ARRAY_PROTO_ADDR.load(Ordering::Relaxed);
    if cached != usize::MAX {
        return cached;
    }
    let ctor = crate::object::js_get_global_this_builtin_value(b"Array".as_ptr(), 5);
    let ctor_value = crate::value::JSValue::from_bits(ctor.to_bits());
    let addr = if ctor_value.is_pointer() {
        let ctor_ptr = ctor_value.as_pointer::<u8>() as usize;
        let proto = crate::closure::closure_get_dynamic_prop(ctor_ptr, "prototype");
        let proto_value = crate::value::JSValue::from_bits(proto.to_bits());
        if proto_value.is_pointer() {
            proto_value.as_pointer::<u8>() as usize
        } else {
            0
        }
    } else {
        0
    };
    // Don't poison the cache with 0: during runtime init the global `Array`
    // constructor may not be materialized yet (symbol writes on other builtin
    // prototypes call into here via `note_array_proto_iterator_write`).
    // Re-derive until it resolves.
    if addr != 0 {
        ARRAY_PROTO_ADDR.store(addr, Ordering::Relaxed);
    }
    addr
}

/// Record (if `arr` is `Array.prototype`) that the prototype now carries an
/// indexed property, so subsequent out-of-bounds reads consult it. Called from
/// the array element-write paths; cheap (two relaxed atomic loads + compare).
#[inline]
pub(crate) fn note_array_index_write(arr: usize) {
    if !ARRAY_PROTO_HAS_INDEX.load(Ordering::Relaxed) && arr != 0 && arr == array_prototype_addr() {
        ARRAY_PROTO_HAS_INDEX.store(true, Ordering::Relaxed);
    }
}

/// Out-of-bounds element read fallback: `Array.prototype[index]` when the
/// prototype has indexed properties (see `ARRAY_PROTO_HAS_INDEX`). Returns the
/// inherited value, or `undefined` if absent. Skipped entirely when the
/// receiver IS `Array.prototype` (avoids self-recursion) or the flag is unset.
#[inline]
unsafe fn array_oob_prototype_get(receiver: usize, index: u32) -> f64 {
    const TAG_UNDEFINED_F64: f64 = f64::from_bits(0x7FFC_0000_0000_0001u64);
    // A custom array [[Prototype]] (Object.setPrototypeOf(arr, otherArray))
    // replaces the default chain — gated on a global relaxed flag.
    if crate::object::prototype_chain::array_static_proto_recorded() {
        if let Some(proto_arr) = array_custom_array_prototype(receiver as *const ArrayHeader) {
            if index < (*proto_arr).length && array_has_own_index(proto_arr, index) {
                return js_array_get_f64(proto_arr, index);
            }
        }
    }
    if ARRAY_PROTO_HAS_INDEX.load(Ordering::Relaxed) {
        let proto = array_prototype_addr();
        if proto != 0 && proto != receiver {
            let proto_arr = proto as *const ArrayHeader;
            if index < (*proto_arr).length && array_has_own_index(proto_arr, index) {
                return js_array_get_f64(proto_arr, index);
            }
        }
    }
    // Object.prototype indexed property (data or defineProperty accessor):
    // arr → Array.prototype → Object.prototype (concat/S15.4.4.4_A3_T3).
    if OBJECT_PROTO_HAS_INDEX.load(Ordering::Relaxed)
        && crate::array::object_prototype_has_index_prop(index)
    {
        return crate::array::sort_object_prototype_index_get(index);
    }
    TAG_UNDEFINED_F64
}

#[inline]
unsafe fn array_sparse_index_property_get(arr: *const ArrayHeader, index: u32) -> Option<f64> {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() || index < (*arr).capacity {
        return None;
    }
    let key = index.to_string();
    array_named_property_get_by_name(arr, &key)
}

unsafe fn array_sparse_index_property_set(arr: *mut ArrayHeader, index: u32, value: f64) {
    let key = index.to_string();
    let key_ptr = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    array_named_property_set(arr, key_ptr, value);
    let new_length = index + 1;
    if (*arr).length < new_length {
        (*arr).length = new_length;
    }
}

/// Whether iterating `arr` with the raw dense-store loop would diverge from the
/// spec `[[HasProperty]]`/`[[Get]]` protocol. True ("exotic") when the array has
/// index accessors / custom-attr descriptors, lives in (partly) sparse storage,
/// or the prototype chain carries indexed properties. When false the fast loop
/// is byte-identical to the spec, so callers keep their hot path.
#[inline]
pub(crate) fn array_iteration_is_exotic(arr: *const ArrayHeader) -> bool {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return false;
    }
    if array_object_flags(arr) & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS != 0 {
        return true;
    }
    if ARRAY_PROTO_HAS_INDEX.load(Ordering::Relaxed) {
        return true;
    }
    if OBJECT_PROTO_HAS_INDEX.load(Ordering::Relaxed) {
        return true;
    }
    // Live indices beyond the dense backing store are stored in the sparse
    // named-property map, which the raw element loop never reads.
    unsafe { (*arr).length > (*arr).capacity }
}

/// Spec `OrdinaryGetOwnProperty(O, ToString(index)) != undefined` for an Array:
/// is `index` present as an *own* property (dense non-hole slot, sparse named
/// data property, or an accessor descriptor)?
pub(crate) unsafe fn array_has_own_index(arr: *const ArrayHeader, index: u32) -> bool {
    if crate::object::descriptors_in_use() {
        let key = index.to_string();
        if crate::object::get_accessor_descriptor(arr as usize, &key).is_some() {
            return true;
        }
    }
    let key = index.to_string();
    if array_named_property_get_by_name(arr, &key).is_some() {
        return true;
    }
    if index < (*arr).length && index < (*arr).capacity {
        let elements = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const u64;
        if ptr::read(elements.add(index as usize)) != crate::value::TAG_HOLE {
            return true;
        }
    }
    false
}

/// Spec `[[HasProperty]]`(O, ToString(index)) for an ordinary Array receiver:
/// own property OR inherited indexed property from `Array.prototype`.
pub(crate) fn array_spec_has_index(arr: *const ArrayHeader, index: u32) -> bool {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return false;
    }
    unsafe {
        if array_has_own_index(arr, index) {
            return true;
        }
        // An explicit `Object.setPrototypeOf(arr, otherArray)` replaces the
        // default chain — consult that array's own indices first (test262
        // copyWithin/coerced-values-start-change-*).
        if let Some(proto_arr) = array_custom_array_prototype(arr) {
            if index < (*proto_arr).length && array_has_own_index(proto_arr, index) {
                return true;
            }
        }
        if ARRAY_PROTO_HAS_INDEX.load(Ordering::Relaxed) {
            let proto = array_prototype_addr();
            if proto != 0 && proto != arr as usize {
                let proto_arr = proto as *const ArrayHeader;
                if index < (*proto_arr).length && array_has_own_index(proto_arr, index) {
                    return true;
                }
            }
        }
        if OBJECT_PROTO_HAS_INDEX.load(Ordering::Relaxed)
            && crate::array::object_prototype_has_index_prop(index)
        {
            return true;
        }
        false
    }
}

/// A custom `[[Prototype]]` installed on `arr` via `Object.setPrototypeOf`
/// that happens to be a real array — `null` otherwise.
unsafe fn array_custom_array_prototype(arr: *const ArrayHeader) -> Option<*const ArrayHeader> {
    let bits = crate::object::prototype_chain::object_static_prototype(arr as usize)?;
    // The recorded proto may be NaN-boxed (0x7FFD) or a RAW untagged pointer
    // (module-level arrays are stored as raw I64s).
    let raw = if (bits >> 48) == 0x7FFD {
        (bits & crate::value::POINTER_MASK) as usize
    } else if (bits >> 48) == 0 && bits > 0x10000 {
        bits as usize
    } else {
        return None;
    };
    if raw < crate::gc::GC_HEADER_SIZE + 0x1000 || raw == arr as usize {
        return None;
    }
    // #5625: the recorded prototype may be a *grown* array whose stored pointer
    // was left FORWARDED by `js_array_grow` — its first 8 bytes now hold the
    // forwarding pointer to the live head instead of length+capacity. (A real
    // array grows when `Object.setPrototypeOf(arr, p)` captured `p` before a
    // later push reallocated it, or the proto itself was built by appends — as
    // in test262 copyWithin/coerced-values-start-change-start, whose
    // `longDenseArray()` fills a `[0]` to 1024 elements.) Resolve the chain so
    // we deref the current array head; reading the defunct old location yields
    // the forwarding pointer's low 32 bits as a garbage `length`, making
    // inherited-index reads silently miss (nondeterministic copyWithin output).
    let resolved = clean_arr_ptr(raw as *const ArrayHeader);
    if resolved.is_null() || resolved as usize == arr as usize {
        return None;
    }
    let hdr = (resolved as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    if (*hdr).obj_type == crate::gc::GC_TYPE_ARRAY {
        Some(resolved)
    } else {
        None
    }
}

/// Spec `[[Get]]`(O, ToString(index)) for an ordinary Array receiver: own value
/// (firing index accessors via `js_array_get_f64`) or, for an absent own index,
/// the inherited `Array.prototype[index]`. Returns `undefined` when absent.
pub(crate) fn array_spec_get(arr: *const ArrayHeader, index: u32) -> f64 {
    const TAG_UNDEFINED_F64: f64 = f64::from_bits(0x7FFC_0000_0000_0001u64);
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return TAG_UNDEFINED_F64;
    }
    unsafe {
        if array_has_own_index(arr, index) {
            return js_array_get_f64(arr, index);
        }
        if let Some(proto_arr) = array_custom_array_prototype(arr) {
            if index < (*proto_arr).length && array_has_own_index(proto_arr, index) {
                return js_array_get_f64(proto_arr, index);
            }
        }
        if ARRAY_PROTO_HAS_INDEX.load(Ordering::Relaxed) {
            let proto = array_prototype_addr();
            if proto != 0 && proto != arr as usize {
                let proto_arr = proto as *const ArrayHeader;
                if index < (*proto_arr).length && array_has_own_index(proto_arr, index) {
                    return js_array_get_f64(proto_arr, index);
                }
            }
        }
        if OBJECT_PROTO_HAS_INDEX.load(Ordering::Relaxed)
            && crate::array::object_prototype_has_index_prop(index)
        {
            return crate::array::sort_object_prototype_index_get(index);
        }
        TAG_UNDEFINED_F64
    }
}

fn array_get_property_by_key(arr: *const ArrayHeader, key: *const crate::StringHeader) -> f64 {
    let value =
        crate::object::js_object_get_field_by_name(arr as *const crate::object::ObjectHeader, key);
    f64::from_bits(value.bits())
}

#[no_mangle]
pub extern "C" fn js_array_length(arr: *const ArrayHeader) -> u32 {
    // #5135: a Proxy typed (statically) as an array (immer drafts) reaches here
    // with the masked proxy id. Read `length` through the proxy `get` trap
    // rather than deref-ing the id as an `ArrayHeader`.
    if let Some(proxy) = array_ptr_as_proxy(arr) {
        let key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
        let key_f64 = crate::value::js_nanbox_string(key as i64);
        let n = crate::builtins::js_number_coerce(crate::proxy::js_proxy_get(proxy, key_f64));
        return if n.is_finite() && n > 0.0 {
            n.min(u32::MAX as f64) as u32
        } else {
            0
        };
    }
    let arr = {
        let bits = arr as u64;
        let top16 = bits >> 48;
        if top16 >= 0x7FF8 {
            if top16 != (crate::value::POINTER_TAG >> 48) {
                return 0;
            }
            (bits & crate::value::POINTER_MASK) as *const ArrayHeader
        } else {
            arr
        }
    };
    if !arr.is_null() {
        let addr = arr as usize;
        if crate::set::is_registered_set(addr) {
            return crate::set::js_set_size(arr as *const crate::set::SetHeader);
        }
        if crate::map::is_registered_map(addr) {
            return crate::map::js_map_size(arr as *const crate::map::MapHeader);
        }
    }
    // Issue #179 Phase 2: lazy array fast path. Check BEFORE
    // `clean_arr_ptr` because that helper rejects pointers whose
    // first two u32s look implausible as (length, capacity) — and a
    // `LazyArrayHeader`'s first fields are (magic, cached_length),
    // which trip the guard. Strip the NaN-box tag manually first.
    unsafe {
        let bits = arr as u64;
        let top16 = bits >> 48;
        let raw_ptr = if top16 >= 0x7FF8 {
            if top16 == 0x7FFC {
                return 0;
            }
            (bits & 0x0000_FFFF_FFFF_FFFF) as *const ArrayHeader
        } else {
            arr
        };
        if !raw_ptr.is_null() && (raw_ptr as usize) >= crate::gc::GC_HEADER_SIZE + 0x1000 {
            let gc_header =
                (raw_ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            // Runtime plain-object receiver behind a statically-Array
            // variable (`var x = []; … x = {0:0}; x.length` — test262
            // splice/S15.4.4.12_A4_T1 #10): reading the ObjectHeader words
            // as (length, capacity) returns garbage. Read the `length`
            // property like any object instead.
            if crate::value::addr_class::is_above_handle_band(raw_ptr as usize)
                && crate::object::is_valid_obj_ptr(raw_ptr as *const u8)
                && ((*gc_header).obj_type == crate::gc::GC_TYPE_OBJECT
                    || (*gc_header).obj_type == crate::gc::GC_TYPE_CLOSURE)
            {
                let key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
                let v = crate::object::js_object_get_field_by_name_f64(
                    raw_ptr as *const crate::object::ObjectHeader,
                    key,
                );
                let n = crate::builtins::js_number_coerce(v);
                return if n.is_nan() || n <= 0.0 {
                    0
                } else {
                    n.min(u32::MAX as f64) as u32
                };
            }
            if (*gc_header).obj_type == crate::gc::GC_TYPE_LAZY_ARRAY {
                let lazy = raw_ptr as *const crate::json_tape::LazyArrayHeader;
                if (*lazy).magic == crate::json_tape::LAZY_ARRAY_MAGIC {
                    // If we've already materialized (e.g. an indexed
                    // access forced it), read the authoritative length
                    // from the materialized tree.
                    if !(*lazy).materialized.is_null() {
                        return (*(*lazy).materialized).length;
                    }
                    return (*lazy).cached_length;
                }
            }
        }
    }
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return 0;
    }
    unsafe { (*arr).length }
}

/// Get the length of an array (i64 bridge for perry-ui-macos)
#[no_mangle]
pub extern "C" fn js_array_get_length(arr: i64) -> i64 {
    js_array_length(arr as *const ArrayHeader) as i64
}

/// Get an element from an array by index (i64 bridge for perry-ui-macos)
#[no_mangle]
pub extern "C" fn js_array_get_element(arr: i64, index: i64) -> f64 {
    js_array_get_f64(arr as *const ArrayHeader, index as u32)
}

/// Alias for js_array_get_element (used by perry-ui-windows dialog)
#[no_mangle]
pub extern "C" fn js_array_get_element_f64(arr: i64, index: i64) -> f64 {
    js_array_get_f64(arr as *const ArrayHeader, index as u32)
}

/// Fast-path array element access: skips all polymorphic registry checks
/// (buffer, set, map). Only does bounds checking and element access.
/// Use when the codegen KNOWS the pointer is a plain Array (not Map/Set/Buffer).
#[no_mangle]
pub extern "C" fn js_array_get_f64_unchecked(arr: *const ArrayHeader, index: u32) -> f64 {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return f64::NAN;
    }
    // Index accessors / custom attrs installed via `Object.defineProperty`
    // need the descriptor-aware getter.
    if array_object_flags(arr) & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS != 0 {
        return js_array_get_f64(arr, index);
    }
    const TAG_UNDEFINED_F64: f64 = f64::from_bits(0x7FFC_0000_0000_0001u64);
    unsafe {
        let length = (*arr).length;
        if index >= length {
            return array_oob_prototype_get(arr as usize, index);
        }
        // Sparse consult only when the index is past the dense backing store:
        // `array_sparse_index_property_get` always returns None below capacity,
        // so checking capacity first keeps the dense hot path call-free.
        if index >= (*arr).capacity {
            if let Some(value) = array_sparse_index_property_get(arr, index) {
                return value;
            }
            return array_oob_prototype_get(arr as usize, index);
        }
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        let raw = *elements_ptr.add(index as usize);
        // Issue #323: translate HOLE sentinel (set by `new Array(n)`) back to
        // `undefined`. The sentinel is internal — user code only ever sees
        // TAG_UNDEFINED for unset slots.
        if raw.to_bits() == crate::value::TAG_HOLE {
            return TAG_UNDEFINED_F64;
        }
        raw
    }
}

#[no_mangle]
pub extern "C" fn js_array_numeric_get_f64_unboxed(arr: *mut ArrayHeader, index: u32) -> f64 {
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return js_array_get_f64(arr, index);
    }

    // Hot path for guarded raw-f64 arrays. The typed-feedback guard already
    // proved this receiver is a non-forwarded plain Array with raw numeric
    // layout, so keep the helper leaf-small: avoid re-running the expensive
    // rebuild/descriptor path on every indexed read in numeric loops.
    unsafe {
        if array_numeric_layout(arr) == Some(NumericArrayLayout::RawF64)
            && array_object_flags(arr) & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS == 0
            && index < (*arr).length
        {
            let elements_ptr =
                (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
            return *elements_ptr.add(index as usize);
        }

        if let Some(value) = array_numeric_raw_f64_get(arr, index) {
            return value;
        }
    }
    js_array_get_f64(arr, index)
}

/// Get an element from an array by index (returns f64)
#[no_mangle]
pub extern "C" fn js_array_get_f64(arr: *const ArrayHeader, index: u32) -> f64 {
    // Issue #179 Phase 5: lazy fast path — must run BEFORE
    // `clean_arr_ptr` because that helper force-materializes a lazy
    // pointer into a regular ArrayHeader. For the common read-only
    // shape (`parsed[i]` on a lazy result), force-materializing the
    // whole tree on first access dominates the workload; the sparse
    // per-element cache only materializes the touched subtree.
    //
    // Same tag-strip pattern as `js_array_length`: v0.5.206 added a
    // lazy guard in `clean_arr_ptr` that force-materializes, but
    // for the sparse-cache path we want to keep the LazyArrayHeader
    // around so the cache persists across calls. Strip the NaN-box
    // tag manually and check obj_type without going through the
    // clean-and-validate helper.
    unsafe {
        let bits = arr as u64;
        let top16 = bits >> 48;
        let raw_ptr = if top16 >= 0x7FF8 {
            if top16 == 0x7FFC {
                return f64::NAN;
            }
            (bits & 0x0000_FFFF_FFFF_FFFF) as *const ArrayHeader
        } else {
            arr
        };
        if !raw_ptr.is_null() && (raw_ptr as usize) >= crate::gc::GC_HEADER_SIZE + 0x1000 {
            let gc_header =
                (raw_ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            if (*gc_header).obj_type == crate::gc::GC_TYPE_LAZY_ARRAY {
                let lazy = raw_ptr as *mut crate::json_tape::LazyArrayHeader;
                if (*lazy).magic == crate::json_tape::LAZY_ARRAY_MAGIC {
                    let value = crate::json_tape::lazy_get(lazy, index);
                    return f64::from_bits(value.bits());
                }
            }
        }
    }
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return f64::NAN;
    }
    // Check if this is actually a TypedArray — dispatch through typed array helper
    if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
        return crate::typedarray::js_typed_array_get(
            arr as *const crate::typedarray::TypedArrayHeader,
            index as i32,
        );
    }
    // Check if this is actually a buffer (Uint8Array) — read individual bytes
    if crate::buffer::is_registered_buffer(arr as usize) {
        let byte_val =
            crate::buffer::js_buffer_get(arr as *const crate::buffer::BufferHeader, index as i32);
        return byte_val as f64;
    }
    // Check if this is a Set — read from elements pointer (not inline)
    if crate::set::is_registered_set(arr as usize) {
        let set = arr as *const crate::set::SetHeader;
        unsafe {
            let size = (*set).size;
            if index >= size {
                return TAG_UNDEFINED_F64;
            }
            let elements = (*set).elements as *const f64;
            return std::ptr::read(elements.add(index as usize));
        }
    }
    // Check if this is a Map — return entries as [key, value] pairs
    if crate::map::is_registered_map(arr as usize) {
        let map = arr as *const crate::map::MapHeader;
        unsafe {
            let size = (*map).size;
            if index >= size {
                return TAG_UNDEFINED_F64;
            }
            let entries = (*map).entries as *const f64;
            // Map entries: key at index*2, return key for simple iteration
            return std::ptr::read(entries.add(index as usize * 2));
        }
    }
    if crate::object::descriptors_in_use() {
        let key = index.to_string();
        if let Some(acc) = crate::object::get_accessor_descriptor(arr as usize, &key) {
            if acc.get != 0 {
                let receiver = crate::value::js_nanbox_pointer(arr as i64);
                return f64::from_bits(
                    unsafe { crate::object::invoke_accessor_getter(acc.get, receiver) }.bits(),
                );
            }
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
    }
    // JS spec: out-of-bounds array access returns `undefined`, not NaN.
    // This matters for destructuring defaults (`const [a, b, c = 30] = [1, 2]`)
    // where the `?? fallback` must see TAG_UNDEFINED, not NaN.
    const TAG_UNDEFINED_F64: f64 = f64::from_bits(0x7FFC_0000_0000_0001u64);
    unsafe {
        let length = (*arr).length;
        if index >= length {
            // Out of bounds: fall through to `Array.prototype[index]` (gated;
            // see `array_oob_prototype_get`). Common case is one atomic load.
            return array_oob_prototype_get(arr as usize, index);
        }
        // Capacity check first: the sparse helper always returns None below
        // capacity, so the dense hot path stays call-free (#4648 put the
        // sparse consult unconditionally first — +28% on 04_array_read).
        if index >= (*arr).capacity {
            if let Some(value) = array_sparse_index_property_get(arr, index) {
                return value;
            }
            return array_oob_prototype_get(arr as usize, index);
        }
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        let raw = *elements_ptr.add(index as usize);
        // Issue #323: translate HOLE sentinel back to `undefined` (see
        // `js_array_alloc_with_length` for context). Per OrdinaryGet a hole
        // falls through to the prototype chain — a custom array prototype or
        // an `Array.prototype[i]` element shows through (test262
        // concat/S15.4.4.4_A3_T2 reads `a[2]` with a hole at 2). Both probes
        // are gated (registry lookup / relaxed atomic) so the dense hot path
        // is unchanged.
        if raw.to_bits() == crate::value::TAG_HOLE {
            if let Some(proto_arr) = array_custom_array_prototype(arr) {
                if index < (*proto_arr).length && array_has_own_index(proto_arr, index) {
                    return js_array_get_f64(proto_arr, index);
                }
            }
            return array_oob_prototype_get(arr as usize, index);
        }
        raw
    }
}

/// Relaxed read of the `Array.prototype`-has-indexed-properties flag, for the
/// typed-feedback guards (a polluted prototype invalidates the raw-slot fast
/// path: holes must read through the chain).
pub(crate) fn array_prototype_has_index_flag() -> bool {
    ARRAY_PROTO_HAS_INDEX.load(Ordering::Relaxed)
}

/// Fast-path array element write: skips all polymorphic registry checks
/// (buffer). Only does bounds checking and element write.
/// Use when the codegen KNOWS the pointer is a plain Array (not Buffer).
#[no_mangle]
pub extern "C" fn js_array_set_f64_unchecked(arr: *mut ArrayHeader, index: u32, value: f64) {
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return;
    }
    if array_is_frozen(arr) {
        return;
    }
    // Index accessors / non-writable attrs need the descriptor-aware setter.
    if array_object_flags(arr) & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS != 0 {
        js_array_set_f64_extend(arr, index, value);
        return;
    }
    unsafe {
        let length = (*arr).length;
        if index >= length {
            return;
        }
        if index >= (*arr).capacity {
            array_sparse_index_property_set(arr, index, value);
            return;
        }
        let value = canonicalize_array_numeric_store_value(arr, value);
        let value_bits = value.to_bits();
        let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        // GC_STORE_AUDIT(BARRIERED): unchecked array set is immediately recorded via note_array_slot.
        ptr::write(elements_ptr.add(index as usize), value);
        note_array_slot(arr, index as usize, value_bits);
    }
}

#[no_mangle]
pub extern "C" fn js_array_numeric_set_f64_unboxed(
    arr: *mut ArrayHeader,
    index: u32,
    value: f64,
) -> i32 {
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return 0;
    }

    let flags = array_object_flags(arr);
    if flags & (crate::gc::OBJ_FLAG_FROZEN | crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS) != 0 {
        return 0;
    }

    // Hot path for the codegen's guarded numeric-array store. Raw-f64 arrays
    // are pointer-free, so an in-bounds numeric overwrite can update the
    // payload directly without per-slot layout notes or revalidating/rebuilding
    // the whole layout on every iteration. Preserve the helper fallback for
    // direct runtime calls and arrays that have not been converted yet.
    unsafe {
        if index < (*arr).length && array_numeric_layout(arr) == Some(NumericArrayLayout::RawF64) {
            let Some(number) = value_bits_to_number(value.to_bits()) else {
                clear_array_numeric_layout(arr);
                return 0;
            };
            let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
            // GC_STORE_AUDIT(POINTER_FREE): RawF64-layout payload slot —
            // `number` is a plain f64, never a NaN-boxed pointer, so no
            // write barrier is needed.
            ptr::write(elements_ptr.add(index as usize), number);
            return 1;
        }

        if array_numeric_raw_f64_set_inbounds(arr, index, value) {
            return 1;
        }
    }
    0
}

/// Set an element in an array by index
/// Note: This does NOT extend the array if index >= length
#[no_mangle]
pub extern "C" fn js_array_set_f64(arr: *mut ArrayHeader, index: u32, value: f64) {
    // A uniquely-owned string assigned to an element (`arr[i] = s`) aliases this
    // slot — demote it to shared so a later `s += x` doesn't mutate the stored
    // element in place. No-op for SSO / non-string.
    crate::string::js_string_addref_if_heap_string(value);
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return;
    }
    // Check if this is actually a buffer (Uint8Array) — write individual bytes
    if crate::buffer::is_registered_buffer(arr as usize) {
        crate::buffer::js_buffer_set(
            arr as *mut crate::buffer::BufferHeader,
            index as i32,
            value as i32,
        );
        return;
    }
    // Check if this is a typed array — route through per-kind store.
    if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
        crate::typedarray::js_typed_array_set(
            arr as *mut crate::typedarray::TypedArrayHeader,
            index as i32,
            value,
        );
        return;
    }
    if array_is_frozen(arr) {
        return;
    }
    unsafe {
        let length = (*arr).length;
        if index >= length {
            return;
        }
        if index >= (*arr).capacity {
            array_sparse_index_property_set(arr, index, value);
            return;
        }
        let value = canonicalize_array_numeric_store_value(arr, value);
        let value_bits = value.to_bits();
        let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        // GC_STORE_AUDIT(BARRIERED): array set is immediately recorded via note_array_slot.
        ptr::write(elements_ptr.add(index as usize), value);
        note_array_slot(arr, index as usize, value_bits);
    }
}

/// Set an element in an array by index, extending the array if needed
/// Returns the (possibly reallocated) array pointer
/// This mimics JavaScript's arr[i] = value behavior
#[no_mangle]
pub extern "C" fn js_array_set_f64_extend(
    arr: *mut ArrayHeader,
    index: u32,
    value: f64,
) -> *mut ArrayHeader {
    // Demote a uniquely-owned string source — see `js_array_set_f64`.
    crate::string::js_string_addref_if_heap_string(value);
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return js_array_alloc(0);
    }
    // If this write targets `Array.prototype`, mark the prototype as carrying an
    // indexed property so out-of-bounds element reads on ordinary arrays consult
    // it (ECMA-262 OrdinaryGet → prototype chain). Cheap no-op otherwise.
    note_array_index_write(arr as usize);
    // Check if this is actually a buffer (Uint8Array) — write individual bytes
    if crate::buffer::is_registered_buffer(arr as usize) {
        crate::buffer::js_buffer_set(
            arr as *mut crate::buffer::BufferHeader,
            index as i32,
            value as i32,
        );
        return arr;
    }
    // Check if this is a typed array — route through per-kind store (no extension).
    if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
        crate::typedarray::js_typed_array_set(
            arr as *mut crate::typedarray::TypedArrayHeader,
            index as i32,
            value,
        );
        return arr;
    }
    let flags = array_object_flags(arr);
    let is_frozen = flags & crate::gc::OBJ_FLAG_FROZEN != 0;
    let blocks_extension =
        flags & (crate::gc::OBJ_FLAG_SEALED | crate::gc::OBJ_FLAG_NO_EXTEND) != 0;
    let scope = crate::gc::RuntimeHandleScope::new();
    let _arr_handle = scope.root_raw_mut_ptr(arr);
    let value_handle = scope.root_nanbox_f64(value);
    unsafe {
        let length = (*arr).length;

        if index == u32::MAX {
            return arr;
        }

        // Index properties customized via `Object.defineProperty`: dispatch
        // accessor setters and honor non-writable data attributes before the
        // dense-element store. Gated on the per-array descriptor flag so the
        // common fast path pays one header-flag test.
        if flags & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS != 0 {
            let key = index.to_string();
            if let Some(acc) = crate::object::get_accessor_descriptor(arr as usize, &key) {
                if acc.set != 0 {
                    crate::object::invoke_accessor_setter(
                        acc.set,
                        crate::value::js_nanbox_pointer(arr as i64),
                        value_handle.get_nanbox_f64(),
                    );
                }
                return arr;
            }
            if let Some(attrs) = crate::object::get_property_attrs(arr as usize, &key) {
                if !attrs.writable() {
                    return arr;
                }
            }
            // Extending past `length` requires a writable `length`.
            if index >= length {
                let len_writable = crate::object::get_property_attrs(arr as usize, "length")
                    .map(|a| a.writable())
                    .unwrap_or(true);
                if !len_writable {
                    return arr;
                }
            }
        }

        // If index is within bounds, just set it
        if index < length {
            if is_frozen {
                return arr;
            }
            if index >= (*arr).capacity {
                let value = value_handle.get_nanbox_f64();
                array_sparse_index_property_set(arr, index, value);
                return arr;
            }
            let value = canonicalize_array_numeric_store_value(arr, value);
            let value_bits = value.to_bits();
            let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
            // GC_STORE_AUDIT(BARRIERED): in-bounds extending set is immediately recorded via note_array_slot.
            ptr::write(elements_ptr.add(index as usize), value);
            note_array_slot(arr, index as usize, value_bits);
            return arr;
        }

        if is_frozen || blocks_extension {
            return arr;
        }

        // Need to extend the array
        let new_length = index + 1;
        if new_length > (*arr).capacity
            && new_length > MAX_DENSE_ARRAY_GROW_LENGTH
            && index - length > DENSE_ARRAY_GAP_LIMIT
        {
            let value = value_handle.get_nanbox_f64();
            array_sparse_index_property_set(arr, index, value);
            return arr;
        }
        let arr = if new_length > (*arr).capacity {
            js_array_grow(arr, new_length)
        } else {
            arr
        };
        let value = value_handle.get_nanbox_f64();

        // Fill any gap with TAG_HOLE so subsequent reads / iteration /
        // JSON.stringify treat them as holes (per ECMA-262 §22.1.3.30
        // step 5.b: holes serialize to "null"). Pre-fix this wrote 0.0
        // which was indistinguishable from a real numeric 0 — sparse
        // arrays serialized as `[0, 0, ...]` instead of `[null, null,
        // ...]`. Read paths translate TAG_HOLE → TAG_UNDEFINED via
        // `js_array_get_f64`'s post-#323 hole handling.
        let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        let hole = f64::from_bits(crate::value::TAG_HOLE);
        for i in length..index {
            // GC_STORE_AUDIT(BARRIERED): sparse gap sentinel is immediately recorded via note_array_slot.
            ptr::write(elements_ptr.add(i as usize), hole);
            note_array_slot(arr, i as usize, crate::value::TAG_HOLE);
        }

        // Set the value
        let value = canonicalize_array_numeric_store_value(arr, value);
        let value_bits = value.to_bits();
        // GC_STORE_AUDIT(BARRIERED): extending set value is immediately recorded via note_array_slot.
        ptr::write(elements_ptr.add(index as usize), value);
        note_array_slot(arr, index as usize, value_bits);
        (*arr).length = new_length;

        arr
    }
}

/// Bulk numeric dense fill for compiler-proven trivial loops:
/// `for (let i = 0; i < end; i++) arr[i] = constant`.
///
/// This keeps JavaScript semantics for frozen/sealed/descriptor arrays by
/// falling back to the ordinary extending setter. The fast path is restricted
/// to dense writes from index 0 through `end - 1`, so the resulting live
/// payload is entirely numeric and can be marked raw-f64 / pointer-free once.
#[no_mangle]
pub extern "C" fn js_array_fill_f64_const_extend(
    arr: *mut ArrayHeader,
    end: u32,
    value: f64,
) -> *mut ArrayHeader {
    if end == 0 {
        return clean_arr_ptr_mut(arr);
    }
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        let new_arr = js_array_alloc(end);
        return js_array_fill_f64_const_extend(new_arr, end, value);
    }
    note_array_index_write(arr as usize);
    let flags = array_object_flags(arr);
    if flags
        & (crate::gc::OBJ_FLAG_FROZEN
            | crate::gc::OBJ_FLAG_SEALED
            | crate::gc::OBJ_FLAG_NO_EXTEND
            | crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS)
        != 0
        || crate::buffer::is_registered_buffer(arr as usize)
        || crate::typedarray::lookup_typed_array_kind(arr as usize).is_some()
    {
        let mut out = arr;
        for i in 0..end {
            out = js_array_set_f64_extend(out, i, value);
        }
        return out;
    }

    let Some(number) = value_bits_to_number(value.to_bits()) else {
        let mut out = arr;
        for i in 0..end {
            out = js_array_set_f64_extend(out, i, value);
        }
        return out;
    };

    unsafe {
        let old_length = (*arr).length;
        let raw_before = array_numeric_layout(arr) == Some(NumericArrayLayout::RawF64);
        if old_length > end && !raw_before {
            let mut fallback = arr;
            for i in 0..end {
                fallback = js_array_set_f64_extend(fallback, i, value);
            }
            return fallback;
        }

        let mut out = if end > (*arr).capacity {
            js_array_grow(arr, end)
        } else {
            arr
        };
        out = clean_arr_ptr_mut(out);
        if out.is_null() || end > (*out).capacity {
            let mut fallback = arr;
            for i in 0..end {
                fallback = js_array_set_f64_extend(fallback, i, value);
            }
            return fallback;
        }
        let elements_ptr = (out as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        for i in 0..end as usize {
            // GC_STORE_AUDIT(POINTER_FREE): bulk numeric fill writes raw f64s only.
            ptr::write(elements_ptr.add(i), number);
        }
        if (*out).length < end {
            (*out).length = end;
        }
        set_array_numeric_layout(out, NumericArrayLayout::RawF64);
        out
    }
}

/// Bulk numeric dense fill for loops bounded by current array length:
/// `for (let i = 0; i < arr.length; i++) arr[i] = constant`.
///
/// If the receiver is exotic (frozen/sealed/descriptors/etc.), the fallback
/// re-reads `.length` on every iteration so it preserves the source loop's
/// observable behavior when setters mutate array length.
#[no_mangle]
pub extern "C" fn js_array_fill_f64_const_len_extend(
    arr: *mut ArrayHeader,
    value: f64,
) -> *mut ArrayHeader {
    let arr = clean_arr_ptr_mut(arr);
    let end = js_array_length(arr);
    if end == 0 || arr.is_null() {
        return arr;
    }
    note_array_index_write(arr as usize);
    let flags = array_object_flags(arr);
    if flags
        & (crate::gc::OBJ_FLAG_FROZEN
            | crate::gc::OBJ_FLAG_SEALED
            | crate::gc::OBJ_FLAG_NO_EXTEND
            | crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS)
        != 0
        || crate::buffer::is_registered_buffer(arr as usize)
        || crate::typedarray::lookup_typed_array_kind(arr as usize).is_some()
    {
        let mut out = arr;
        let mut i = 0;
        while i < js_array_length(out) {
            out = js_array_set_f64_extend(out, i, value);
            i += 1;
        }
        return out;
    }
    js_array_fill_f64_const_extend(arr, end, value)
}

/// Bulk numeric dense fill for compiler-proven trivial loops:
/// `for (let i = 0; i < end; i++) arr[i] = i`.
#[no_mangle]
pub extern "C" fn js_array_fill_f64_iota_extend(
    arr: *mut ArrayHeader,
    end: u32,
) -> *mut ArrayHeader {
    if end == 0 {
        return clean_arr_ptr_mut(arr);
    }
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        let new_arr = js_array_alloc(end);
        return js_array_fill_f64_iota_extend(new_arr, end);
    }
    note_array_index_write(arr as usize);
    let flags = array_object_flags(arr);
    if flags
        & (crate::gc::OBJ_FLAG_FROZEN
            | crate::gc::OBJ_FLAG_SEALED
            | crate::gc::OBJ_FLAG_NO_EXTEND
            | crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS)
        != 0
        || crate::buffer::is_registered_buffer(arr as usize)
        || crate::typedarray::lookup_typed_array_kind(arr as usize).is_some()
    {
        let mut out = arr;
        for i in 0..end {
            out = js_array_set_f64_extend(out, i, i as f64);
        }
        return out;
    }

    unsafe {
        let old_length = (*arr).length;
        let raw_before = array_numeric_layout(arr) == Some(NumericArrayLayout::RawF64);
        if old_length > end && !raw_before {
            let mut fallback = arr;
            for i in 0..end {
                fallback = js_array_set_f64_extend(fallback, i, i as f64);
            }
            return fallback;
        }

        let mut out = if end > (*arr).capacity {
            js_array_grow(arr, end)
        } else {
            arr
        };
        out = clean_arr_ptr_mut(out);
        if out.is_null() || end > (*out).capacity {
            let mut fallback = arr;
            for i in 0..end {
                fallback = js_array_set_f64_extend(fallback, i, i as f64);
            }
            return fallback;
        }
        let elements_ptr = (out as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        for i in 0..end as usize {
            // GC_STORE_AUDIT(POINTER_FREE): bulk iota fill writes raw f64s only.
            ptr::write(elements_ptr.add(i), i as f64);
        }
        if (*out).length < end {
            (*out).length = end;
        }
        set_array_numeric_layout(out, NumericArrayLayout::RawF64);
        out
    }
}

/// Bulk numeric dense fill for loops bounded by current array length:
/// `for (let i = 0; i < arr.length; i++) arr[i] = i`.
///
/// If the receiver is exotic (frozen/sealed/descriptors/etc.), the fallback
/// re-reads `.length` on every iteration so it preserves the source loop's
/// observable behavior when setters mutate array length.
#[no_mangle]
pub extern "C" fn js_array_fill_f64_iota_len_extend(arr: *mut ArrayHeader) -> *mut ArrayHeader {
    let arr = clean_arr_ptr_mut(arr);
    let end = js_array_length(arr);
    if end == 0 || arr.is_null() {
        return arr;
    }
    note_array_index_write(arr as usize);
    let flags = array_object_flags(arr);
    if flags
        & (crate::gc::OBJ_FLAG_FROZEN
            | crate::gc::OBJ_FLAG_SEALED
            | crate::gc::OBJ_FLAG_NO_EXTEND
            | crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS)
        != 0
        || crate::buffer::is_registered_buffer(arr as usize)
        || crate::typedarray::lookup_typed_array_kind(arr as usize).is_some()
    {
        let mut out = arr;
        let mut i = 0;
        while i < js_array_length(out) {
            out = js_array_set_f64_extend(out, i, i as f64);
            i += 1;
        }
        return out;
    }
    js_array_fill_f64_iota_extend(arr, end)
}

#[used]
static KEEP_ARRAY_FILL_F64_CONST_EXTEND: extern "C" fn(
    *mut ArrayHeader,
    u32,
    f64,
) -> *mut ArrayHeader = js_array_fill_f64_const_extend;
#[used]
static KEEP_ARRAY_FILL_F64_IOTA_EXTEND: extern "C" fn(*mut ArrayHeader, u32) -> *mut ArrayHeader =
    js_array_fill_f64_iota_extend;
#[used]
static KEEP_ARRAY_FILL_F64_CONST_LEN_EXTEND: extern "C" fn(
    *mut ArrayHeader,
    f64,
) -> *mut ArrayHeader = js_array_fill_f64_const_len_extend;
#[used]
static KEEP_ARRAY_FILL_F64_IOTA_LEN_EXTEND: extern "C" fn(*mut ArrayHeader) -> *mut ArrayHeader =
    js_array_fill_f64_iota_len_extend;

/// `arr[stringKey] = value` — handles the JS spec rule that numeric-string
/// keys on arrays are coerced to integer indices. Pre-fix the codegen's
/// IndexSet array fast-path applied `fptosi(double, i32)` directly to the
/// NaN-boxed string value, producing garbage indices that all collapsed
/// onto slot 0 (every iteration overwrote the previous).
///
/// Spec: an "array index" is a string whose canonical numeric form is a
/// non-negative integer < 2^32-1. Such writes update the array's element
/// storage; non-numeric string keys fall through to the object-property
/// path on the array's expando map (rare).
///
/// Issue #637 followup: this helper is also called from the polymorphic
/// IndexSet dispatch when the receiver type isn't statically known —
/// the runtime detects the receiver's gc_type byte and routes to the
/// per-kind setter. For Object/Closure receivers, fall through to
/// `js_object_set_field_by_name`. For Array receivers, parse the key
/// as integer and route to `js_array_set_f64_extend`.
#[no_mangle]
pub extern "C" fn js_array_set_string_key(
    arr: *mut ArrayHeader,
    key: *const crate::StringHeader,
    value: f64,
) -> *mut ArrayHeader {
    if arr.is_null() || key.is_null() {
        return arr;
    }
    // A class-ref value (INT32 tag 0x7FFE) reaching this polymorphic setter
    // (`C[name] = v` where `C` is a runtime class-ref value) is not an array —
    // its high bits are set, so the `is_array` GC-header probe below would
    // dereference unmapped memory. Route to the by-name object setter, which
    // detects the class-ref tag and stores into the static-field tables.
    if (arr as u64) >> 48 == 0x7FFE {
        crate::object::js_object_set_field_by_name(
            arr as *mut crate::object::ObjectHeader,
            key,
            value,
        );
        return arr;
    }
    // Issue #637: also called from polymorphic IndexSet — detect the
    // receiver's gc_type and route accordingly. For Object/Closure
    // (non-array) receivers, just call the object setter directly so
    // the standard expando-property path runs.
    let is_array = unsafe {
        if (arr as usize) >= crate::gc::GC_HEADER_SIZE + 0x1000 {
            let gc_header =
                (arr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            (*gc_header).obj_type == crate::gc::GC_TYPE_ARRAY
        } else {
            false
        }
    };
    if !is_array {
        crate::object::js_object_set_field_by_name(
            arr as *mut crate::object::ObjectHeader,
            key,
            value,
        );
        return arr;
    }
    // Read the key as a Rust &str via the standard StringHeader layout.
    let key_str = unsafe {
        let len = (*key).byte_len as usize;
        if len == 0 {
            return arr;
        }
        let data = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let bytes = std::slice::from_raw_parts(data, len);
        match std::str::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => return arr,
        }
    };
    // Try parse as a non-negative integer in array-index range.
    if let Ok(idx) = key_str.parse::<u32>() {
        // Reject leading zeros / signs that would round-trip differently
        // (e.g. "01" -> 1, but the canonical form is "1"; per spec only
        // "1" is a valid array index, "01" is a generic property).
        let canonical = idx.to_string();
        if canonical == key_str && idx < u32::MAX {
            return js_array_set_f64_extend(arr, idx, value);
        }
    }
    if array_is_frozen(arr) {
        return arr;
    }
    let existing = unsafe { array_named_property_get(arr, key).is_some() };
    if !existing && array_is_sealed_or_no_extend(arr) {
        return arr;
    }
    // Named accessor installed via `Object.defineProperty(arr, "prop",
    // {get,set})`: dispatch the setter instead of the expando store.
    if crate::object::descriptors_in_use() {
        if let Some(acc) = crate::object::get_accessor_descriptor(arr as usize, key_str) {
            if acc.set != 0 {
                unsafe {
                    crate::object::invoke_accessor_setter(
                        acc.set,
                        crate::value::js_nanbox_pointer(arr as i64),
                        value,
                    );
                }
            }
            return arr;
        }
    }
    if let Some(attrs) = crate::object::get_property_attrs(arr as usize, key_str) {
        if !attrs.writable() {
            return arr;
        }
    }
    // Non-numeric string key — fall through to object-property set on the
    // array's expando map. Arrays with named properties are rare but spec-
    // legal.
    unsafe {
        array_named_property_set(arr, key, value);
    }
    arr
}

/// `arr[idx]` where `idx` may be a number or property-key value. This mirrors
/// `js_array_set_index_or_string` for read paths that cannot safely narrow the
/// key through i32 codegen.
#[no_mangle]
pub extern "C" fn js_array_get_index_or_string(arr: *const ArrayHeader, idx: f64) -> f64 {
    if arr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let bits = idx.to_bits();
    let top16 = bits >> 48;
    if top16 == 0x7FFF {
        let key = (bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::StringHeader;
        return array_get_property_by_key(arr, key);
    }
    if top16 == 0x7FF9 {
        let key = crate::value::js_get_string_pointer_unified(idx) as *const crate::StringHeader;
        return array_get_property_by_key(arr, key);
    }

    let numeric = if (bits & crate::value::TAG_MASK) == crate::value::INT32_TAG {
        Some(crate::value::JSValue::from_bits(bits).as_int32() as f64)
    } else if !(0x7FF8..=0x7FFF).contains(&top16) {
        Some(idx)
    } else {
        None
    };
    if let Some(n) = numeric {
        if n.is_finite() && n.trunc() == n && n >= 0.0 && n < u32::MAX as f64 {
            return js_array_get_f64(arr, n as u32);
        }
        if n.is_finite() && n.trunc() == n {
            let key = if n == 0.0 {
                "0".to_string()
            } else {
                format!("{:.0}", n)
            };
            let key_ptr = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
            return array_get_property_by_key(arr, key_ptr);
        }
    }

    if unsafe { crate::symbol::js_is_symbol(idx) } != 0 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let key = crate::value::js_jsvalue_to_string(idx);
    if key.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    array_get_property_by_key(arr, key as *const crate::StringHeader)
}

/// `arr[idx] = value` where idx may be a NaN-boxed string (numeric-string
/// key) OR a number. Dispatches at runtime: string tags → parse and route
/// to `js_array_set_string_key`; otherwise treat as numeric and route to
/// `js_array_set_f64_extend`. Issue #637 followup: the array fast-path's
/// `fptosi(idx_double, i32)` collapsed every NaN-boxed string to slot 0
/// (NaN→i32 = 0 on most platforms), so `forEach((k) => arr[k] = ...)`
/// over `["0","1","2"]` overwrote slot 0 three times. Codegen routes
/// the array fast-path here when the index expression isn't statically
/// numeric.
#[no_mangle]
pub extern "C" fn js_array_set_index_or_string(
    arr: *mut ArrayHeader,
    idx: f64,
    value: f64,
) -> *mut ArrayHeader {
    if arr.is_null() {
        return arr;
    }
    let bits = idx.to_bits();
    let top16 = bits >> 48;
    // STRING_TAG (0x7FFF) heap pointer — dispatch through the string-key
    // helper which parses the numeric value and routes appropriately.
    // SHORT_STRING_TAG (0x7FF9) is the SSO variant; same path via
    // `js_get_string_pointer_unified` — handled inside `js_string_*` helpers.
    if top16 == 0x7FFF {
        let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::StringHeader;
        return js_array_set_string_key(arr, ptr, value);
    }
    if top16 == 0x7FF9 {
        // SHORT_STRING_TAG (SSO). Materialize as a real StringHeader
        // via `js_get_string_pointer_unified` so `js_array_set_string_key`
        // can read the bytes through the standard layout.
        let str_ptr =
            crate::value::js_get_string_pointer_unified(idx) as *const crate::StringHeader;
        return js_array_set_string_key(arr, str_ptr, value);
    }
    // Treat numeric keys according to the array-index boundary. Only
    // integers in 0..2^32-2 extend element storage; 2^32-1 and larger are
    // ordinary string properties.
    let numeric = if (bits & crate::value::TAG_MASK) == crate::value::INT32_TAG {
        Some(crate::value::JSValue::from_bits(bits).as_int32() as f64)
    } else if !(0x7FF8..=0x7FFF).contains(&top16) {
        Some(idx)
    } else {
        None
    };
    if let Some(n) = numeric {
        if n.is_finite() && n.trunc() == n && n >= 0.0 && n < u32::MAX as f64 {
            return js_array_set_f64_extend(arr, n as u32, value);
        }
        // Any other finite/non-finite number that is NOT a canonical array
        // index (2^32-1 and above, negatives, and non-integer floats such as
        // `a[1.5]`) becomes an ordinary string property. Route through
        // `js_jsvalue_to_string` so the key is the spec ToString of the
        // number ("4294967295", "-1", "1.5", "NaN") rather than a truncated
        // integer — `js_array_set_string_key` then stores it on the expando
        // map without touching `length` or any element slot. (Issue #4543.)
        let key = crate::value::js_jsvalue_to_string(idx);
        if !key.is_null() {
            return js_array_set_string_key(arr, key as *const crate::StringHeader, value);
        }
    }
    // Fallback for a NON-numeric key: a primitive (`a[null]`, `a[undefined]`,
    // `a[true]`, `a[10n]`) or a boxed object (`a[new Number(1)]`). Per
    // ToPropertyKey these become string property keys (or, for `10n`, the
    // canonical index "10"); `js_array_set_string_key` routes accordingly.
    // Arrays previously DROPPED these writes (plain objects handled them).
    // Restricted to `numeric.is_none()`: numeric keys (including non-integer
    // finite floats) are handled above. Symbols stay symbol-keyed.
    if numeric.is_none() && unsafe { crate::symbol::js_is_symbol(idx) } == 0 {
        let key = crate::value::js_jsvalue_to_string(idx);
        if !key.is_null() {
            return js_array_set_string_key(arr, key as *const crate::StringHeader, value);
        }
    }
    arr
}
