//! Indexed and named field get/set: the inline-cache hot path
//! (`js_object_get_field_by_name`, `js_object_get_field_ic_miss`,
//! `js_object_set_field_by_name`), plus keys/values/entries/has_property
//! and the polymorphic index accessors.
//!
//! Split out of `object.rs` (issue #1103). Pure relocation — no logic
//! changes.

use super::*;

const CLASS_ID_BOXED_NUMBER: u32 = 0xFFFF_00D0;
const CLASS_ID_BOXED_STRING: u32 = 0xFFFF_00D1;
const CLASS_ID_BOXED_BOOLEAN: u32 = 0xFFFF_00D2;
const CLASS_ID_BOXED_BIGINT: u32 = 0xFFFF_00D3;
const CLASS_ID_BOXED_SYMBOL: u32 = 0xFFFF_00D4;

const CRYPTO_USAGE_ENCRYPT: u32 = 1 << 0;
const CRYPTO_USAGE_DECRYPT: u32 = 1 << 1;
const CRYPTO_USAGE_SIGN: u32 = 1 << 2;
const CRYPTO_USAGE_VERIFY: u32 = 1 << 3;
const CRYPTO_USAGE_DERIVE_KEY: u32 = 1 << 4;
const CRYPTO_USAGE_DERIVE_BITS: u32 = 1 << 5;
const CRYPTO_USAGE_WRAP_KEY: u32 = 1 << 6;
const CRYPTO_USAGE_UNWRAP_KEY: u32 = 1 << 7;

unsafe fn crypto_key_property_value(addr: usize, key_bytes: &[u8]) -> Option<JSValue> {
    let (algo, hash, kind, extractable, usages) = crate::buffer::crypto_key_meta(addr)?;
    match key_bytes {
        b"algorithm" => Some(crypto_key_algorithm_value(addr, algo, hash)),
        b"extractable" => Some(JSValue::bool(extractable)),
        b"type" => Some(string_value(match kind {
            2 => "private",
            3 => "public",
            _ => "secret",
        })),
        b"usages" => Some(crypto_key_usages_value(usages)),
        _ => None,
    }
}

unsafe fn crypto_key_algorithm_value(addr: usize, algo: u8, hash: u8) -> JSValue {
    let obj = js_object_alloc(0, 3);
    if obj.is_null() {
        return JSValue::undefined();
    }
    set_string_field(obj, b"name", crypto_key_algorithm_name(algo));
    if crypto_key_algorithm_has_hash(algo) {
        let hash_obj = js_object_alloc(0, 1);
        if !hash_obj.is_null() {
            set_string_field(hash_obj, b"name", crypto_key_hash_name(hash));
            set_value_field(obj, b"hash", JSValue::pointer(hash_obj as *const u8));
        }
    }
    if crypto_key_algorithm_has_length(algo) {
        let key = addr as *const crate::buffer::BufferHeader;
        let bits = if key.is_null() {
            0.0
        } else {
            crate::buffer::js_buffer_length(key) as f64 * 8.0
        };
        set_value_field(obj, b"length", JSValue::number(bits));
    }
    if let Some(curve) = crypto_key_named_curve(algo) {
        set_string_field(obj, b"namedCurve", curve);
    }
    JSValue::pointer(obj as *const u8)
}

fn crypto_key_algorithm_name(algo: u8) -> &'static str {
    match algo {
        1 => "HMAC",
        2 => "AES-GCM",
        3 => "AES-KW",
        4 => "AES-CBC",
        5 => "AES-CTR",
        6 => "HKDF",
        7 => "PBKDF2",
        8 => "ECDSA",
        9 => "ECDH",
        10 => "Ed25519",
        11 => "X25519",
        12 => "RSASSA-PKCS1-v1_5",
        13 => "RSA-OAEP",
        14 => "RSA-PSS",
        _ => "",
    }
}

fn crypto_key_hash_name(hash: u8) -> &'static str {
    match hash {
        1 => "SHA-1",
        3 => "SHA-384",
        4 => "SHA-512",
        _ => "SHA-256",
    }
}

fn crypto_key_algorithm_has_hash(algo: u8) -> bool {
    matches!(algo, 1 | 12 | 13 | 14)
}

fn crypto_key_algorithm_has_length(algo: u8) -> bool {
    matches!(algo, 1 | 2 | 3 | 4 | 5)
}

fn crypto_key_named_curve(algo: u8) -> Option<&'static str> {
    match algo {
        8 | 9 => Some("P-256"),
        10 => Some("Ed25519"),
        11 => Some("X25519"),
        _ => None,
    }
}

unsafe fn crypto_key_usages_value(usages: u32) -> JSValue {
    let entries = [
        (CRYPTO_USAGE_ENCRYPT, "encrypt"),
        (CRYPTO_USAGE_DECRYPT, "decrypt"),
        (CRYPTO_USAGE_SIGN, "sign"),
        (CRYPTO_USAGE_VERIFY, "verify"),
        (CRYPTO_USAGE_DERIVE_KEY, "deriveKey"),
        (CRYPTO_USAGE_DERIVE_BITS, "deriveBits"),
        (CRYPTO_USAGE_WRAP_KEY, "wrapKey"),
        (CRYPTO_USAGE_UNWRAP_KEY, "unwrapKey"),
    ];
    let count = entries.iter().filter(|(bit, _)| usages & *bit != 0).count();
    let mut arr = crate::array::js_array_alloc(count as u32);
    for (bit, name) in entries {
        if usages & bit == 0 {
            continue;
        }
        let s = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        arr = crate::array::js_array_push(arr, JSValue::string_ptr(s));
    }
    JSValue::array_ptr(arr)
}

unsafe fn set_string_field(obj: *mut ObjectHeader, key: &[u8], value: &str) {
    let key = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    let value = crate::string::js_string_from_bytes(value.as_ptr(), value.len() as u32);
    js_object_set_field_by_name(obj, key, f64::from_bits(JSValue::string_ptr(value).bits()));
}

unsafe fn set_value_field(obj: *mut ObjectHeader, key: &[u8], value: JSValue) {
    let key = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    js_object_set_field_by_name(obj, key, f64::from_bits(value.bits()));
}

unsafe fn string_value(value: &str) -> JSValue {
    let s = crate::string::js_string_from_bytes(value.as_ptr(), value.len() as u32);
    JSValue::string_ptr(s)
}

/// Get a field from an object by index
///
/// #1129/#1136: the small-pointer guard below previously used a 16 MB
/// floor (0x1000000), which rejected legitimate iOS-device heap
/// pointers from libsystem_malloc — `splitDeepLink()` returning
/// `{ segments }` and the caller destructuring `const { segments } = …`
/// silently produced `undefined`. The real liveness check is the
/// downstream `is_valid_obj_ptr` / `obj_type` validation; this gate
/// only needs to keep the small-handle range and null/guard pages
/// out before unsafe deref. 64 KB matches the bar used elsewhere in
/// this module (e.g. `js_object_get_field_ic_miss`).
#[no_mangle]
pub extern "C" fn js_object_get_field(obj: *const ObjectHeader, field_index: u32) -> JSValue {
    let obj = {
        let b = obj as u64;
        let t = b >> 48;
        if t >= 0x7FF8 {
            if t == 0x7FFC
                || (b & 0x0000_FFFF_FFFF_FFFF) == 0
                || (b & 0x0000_FFFF_FFFF_FFFF) < 0x10000
            {
                return JSValue::undefined();
            }
            (b & 0x0000_FFFF_FFFF_FFFF) as *const ObjectHeader
        } else {
            obj
        }
    };
    if obj.is_null() || (obj as usize) < 0x10000 {
        return JSValue::undefined();
    }
    unsafe {
        // Bounds check: check inline fields first, then overflow map
        let fc = (*obj).field_count;
        if field_index >= fc {
            // Check overflow map for fields that didn't fit in inline storage
            return match overflow_get(obj as usize, field_index as usize) {
                Some(bits) => JSValue::from_bits(bits),
                None => JSValue::undefined(),
            };
        }
        // Guard: corrupted objects with unreasonably large field_count
        if fc > 10000 {
            return JSValue::undefined();
        }
        let fields_ptr =
            (obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const JSValue;
        let val = *fields_ptr.add(field_index as usize);
        // Guard: null POINTER_TAG (0x7FFD_0000_0000_0000) is never legitimate — replace with undefined
        if val.bits() == 0x7FFD_0000_0000_0000 {
            eprintln!(
                "[NULL_PTR_FIELD_GET] obj={:p} field_index={} class_id={} field_count={}",
                obj,
                field_index,
                (*obj).class_id,
                (*obj).field_count
            );
            return JSValue::undefined();
        }
        val
    }
}

pub(crate) unsafe fn own_data_field_by_name(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> Option<JSValue> {
    if key.is_null() {
        return None;
    }
    if obj.is_null() || !is_valid_obj_ptr(obj as *const u8) {
        return None;
    }
    let obj_gc = (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    if (*obj_gc).obj_type != crate::gc::GC_TYPE_OBJECT {
        return None;
    }
    let keys = (*obj).keys_array;
    let keys_ptr = keys as usize;
    if keys.is_null() || (keys_ptr as u64) >> 48 != 0 || keys_ptr < 0x10000 {
        return None;
    }
    let keys_gc = (keys as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    if (*keys_gc).obj_type != crate::gc::GC_TYPE_ARRAY {
        return None;
    }

    let key_count = crate::array::js_array_length(keys) as usize;
    if key_count > 65536 {
        return None;
    }
    let alloc_limit = std::cmp::max((*obj).field_count, 8) as usize;
    for i in 0..key_count {
        let key_val = crate::array::js_array_get(keys, i as u32);
        // #1781: accept inline SSO short keys — `is_string()` is
        // STRING_TAG-only, so the pre-fix shape silently skipped any
        // ≤5-byte key stored as a `SHORT_STRING_TAG` value.
        if crate::string::js_string_key_matches(key_val, key) {
            if i < alloc_limit {
                return Some(js_object_get_field(obj, i as u32));
            }
            return Some(match overflow_get(obj as usize, i) {
                Some(bits) => JSValue::from_bits(bits),
                None => JSValue::undefined(),
            });
        }
    }
    None
}

unsafe fn ordinary_object_prototype_method_value(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> Option<JSValue> {
    if obj.is_null() || key.is_null() {
        return None;
    }
    let gc = gc_header_for(obj);
    if (*gc).obj_type != crate::gc::GC_TYPE_OBJECT {
        return None;
    }
    if ((*gc)._reserved & crate::gc::OBJ_FLAG_NULL_PROTO) != 0 {
        return None;
    }
    if super::prototype_chain::object_static_prototype(obj as usize).is_some() {
        return None;
    }
    let class_id = (*obj).class_id;
    if class_id != 0 && !is_anon_shape_class_id(class_id) {
        return None;
    }
    let key_bytes = std::slice::from_raw_parts(
        (key as *const u8).add(std::mem::size_of::<crate::StringHeader>()),
        (*key).byte_len as usize,
    );
    if !is_primitive_proto_method(key_bytes) {
        return None;
    }
    let object_ctor = js_get_global_this_builtin_value(b"Object".as_ptr(), 6);
    let ctor_value = JSValue::from_bits(object_ctor.to_bits());
    if !ctor_value.is_pointer() {
        return None;
    }
    let ctor_ptr = ctor_value.as_pointer::<crate::closure::ClosureHeader>() as usize;
    let proto = crate::closure::closure_get_dynamic_prop(ctor_ptr, "prototype");
    let proto_value = JSValue::from_bits(proto.to_bits());
    if !proto_value.is_pointer() {
        return None;
    }
    let proto_ptr = proto_value.as_pointer::<ObjectHeader>();
    if proto_ptr.is_null() || proto_ptr == obj {
        return None;
    }
    let method = js_object_get_field_by_name(proto_ptr, key);
    if method.is_undefined() {
        None
    } else {
        Some(method)
    }
}

unsafe fn invoke_accessor_getter(get_bits: u64, receiver: f64) -> JSValue {
    let closure = (get_bits & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader;
    if closure.is_null() {
        return JSValue::undefined();
    }
    let prev = super::js_implicit_this_set(receiver);
    let result_f64 = crate::closure::js_closure_call0(closure);
    super::js_implicit_this_set(prev);
    JSValue::from_bits(result_f64.to_bits())
}

/// #4140: builtin *reflection-only* accessors — most prominently the four
/// `%TypedArray%.prototype` getters (`length`/`byteLength`/`byteOffset`/
/// `buffer`) — are installed via [`super::set_builtin_accessor_descriptor`],
/// which deliberately does NOT flip the `ACCESSORS_IN_USE` hot-path gate (these
/// getters are never written and exist purely so reflection sees them, see
/// #2060). The downside: a plain *value* read that resolves to the hosting
/// prototype object (e.g. `Uint8Array.prototype.buffer`, where the per-kind
/// proto inherits from the shared `%TypedArray%.prototype`) skips the gated
/// accessor short-circuit and returns the empty backing slot — `undefined`
/// instead of Node's `TypeError`.
///
/// Invoke the real getter here for the one builtin object that hosts these
/// getters, guarded by a cheap pointer compare so ordinary reads pay nothing.
/// The receiver is the intrinsic prototype itself, which is never a concrete
/// typed array (real `TypedArray` instances short-circuit far earlier in
/// `js_object_get_field_by_name`), so the getter always throws the spec
/// `TypeError` — matching `Uint8Array.prototype.buffer` in Node. When the gate
/// IS on, the inline short-circuit below already handles this, so bail.
unsafe fn builtin_reflection_accessor_read(
    obj: *const ObjectHeader,
    key_bytes: &[u8],
) -> Option<JSValue> {
    // Only the four `%TypedArray%.prototype` accessor names — the cheap key
    // filter keeps this off every other property read entirely.
    if !matches!(
        key_bytes,
        b"buffer" | b"byteLength" | b"byteOffset" | b"length"
    ) {
        return None;
    }
    // This helper runs before the heavy object validation further down, so a
    // caller that passes a NaN-boxed number / raw `f64` as `obj` (e.g. the
    // dynamic `arr.length = …` set path threading a numeric value through the
    // generic getter) must not be dereferenced. A genuine heap pointer has its
    // top 16 bits clear; reject anything else and confirm it points at a real
    // GC object before reading its header below.
    if (obj as u64) >> 48 != 0 || !super::is_valid_obj_ptr(obj as *const u8) {
        return None;
    }
    let intrinsic_proto =
        super::TYPED_ARRAY_INTRINSIC_PROTO_PTR.load(std::sync::atomic::Ordering::Relaxed);
    if intrinsic_proto == 0 {
        return None;
    }
    // Fire for the shared `%TypedArray%.prototype` intrinsic itself and for
    // every per-kind prototype (`Uint8Array.prototype`, …). The per-kind protos
    // carry `OBJ_FLAG_TYPED_ARRAY_PROTO` and resolve their `[[Prototype]]` to
    // the intrinsic only through `Object.getPrototypeOf`'s flag check — they
    // have `class_id == 0` and no recorded static-prototype link, so the normal
    // chain walk in this function never reaches the intrinsic where these
    // accessors live, and the read silently returned the empty slot
    // (`undefined`) instead of Node's `TypeError`. None of these objects is a
    // concrete typed array (real instances short-circuit far earlier via the
    // `TYPED_ARRAY_REGISTRY` arm), so invoking the getter with the proto as the
    // receiver always throws — matching `Uint8Array.prototype.buffer` in Node.
    // #4140.
    let is_intrinsic = obj as i64 == intrinsic_proto;
    // `OBJ_FLAG_TYPED_ARRAY_PROTO` lives in the shared `_reserved` word, whose
    // bits mean different things for `GC_TYPE_ARRAY` (raw-f64 layout, arguments,
    // survival age, …). The per-kind typed-array prototypes are always plain
    // `GC_TYPE_OBJECT`s, so gate the flag read on the object type — otherwise a
    // regular array whose `_reserved` happens to have bit 0x100 set would be
    // misread as a typed-array prototype and its `.length` get would crash.
    let gc = (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    let is_perkind_proto = (*gc).obj_type == crate::gc::GC_TYPE_OBJECT
        && ((*gc)._reserved & crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO) != 0;
    if !is_intrinsic && !is_perkind_proto {
        return None;
    }
    // The accessor descriptors live on the intrinsic prototype, not the per-kind
    // protos, so always resolve the getter off the intrinsic.
    let name = std::str::from_utf8(key_bytes).ok()?;
    let acc = get_accessor_descriptor(intrinsic_proto as usize, name)?;
    if acc.get == 0 {
        return Some(JSValue::undefined());
    }
    let receiver = crate::value::js_nanbox_pointer(obj as i64);
    Some(invoke_accessor_getter(acc.get, receiver))
}

unsafe fn primitive_object_prototype_accessor(name: &str, receiver: f64) -> Option<JSValue> {
    if !ACCESSORS_IN_USE.with(|c| c.get()) {
        return None;
    }
    let object_ctor = super::js_get_global_this_builtin_value(b"Object".as_ptr(), 6);
    let ctor_value = JSValue::from_bits(object_ctor.to_bits());
    if !ctor_value.is_pointer() {
        return None;
    }
    let ctor_ptr = ctor_value.as_pointer::<crate::closure::ClosureHeader>() as usize;
    let proto = crate::closure::closure_get_dynamic_prop(ctor_ptr, "prototype");
    let proto_value = JSValue::from_bits(proto.to_bits());
    if !proto_value.is_pointer() {
        return None;
    }
    let proto_ptr = proto_value.as_pointer::<ObjectHeader>() as usize;
    let acc = get_accessor_descriptor(proto_ptr, name)?;
    if acc.get == 0 {
        return Some(JSValue::undefined());
    }
    Some(invoke_accessor_getter(acc.get, receiver))
}

unsafe fn bind_closure_value_to_receiver(value: JSValue, receiver: f64) -> JSValue {
    let bits = value.bits();
    if (bits & crate::value::TAG_MASK) != crate::value::POINTER_TAG {
        return value;
    }
    let ptr = (bits & crate::value::POINTER_MASK) as usize;
    if !crate::closure::is_closure_ptr(ptr) {
        return value;
    }
    JSValue::from_bits(crate::closure::clone_closure_rebind_this(bits, receiver))
}

unsafe fn primitive_builtin_prototype_property(
    builtin_name: &[u8],
    key: *const crate::StringHeader,
    receiver: f64,
) -> Option<JSValue> {
    if key.is_null() {
        return None;
    }
    let ctor = js_get_global_this_builtin_value(builtin_name.as_ptr(), builtin_name.len());
    let ctor_value = JSValue::from_bits(ctor.to_bits());
    if !ctor_value.is_pointer() {
        return None;
    }
    let ctor_ptr = ctor_value.as_pointer::<crate::closure::ClosureHeader>() as usize;
    let proto = crate::closure::closure_get_dynamic_prop(ctor_ptr, "prototype");
    let proto_value = JSValue::from_bits(proto.to_bits());
    if !proto_value.is_pointer() {
        return None;
    }
    let proto_ptr = proto_value.as_pointer::<ObjectHeader>();
    if proto_ptr.is_null() {
        return None;
    }
    let value = js_object_get_field_by_name(proto_ptr, key);
    if value.is_undefined() {
        return None;
    }
    Some(bind_closure_value_to_receiver(value, receiver))
}

unsafe fn string_index_value(str_value: f64, key: *const crate::StringHeader) -> Option<JSValue> {
    if key.is_null() {
        return None;
    }
    let str_ptr =
        crate::value::js_get_string_pointer_unified(str_value) as *const crate::StringHeader;
    if str_ptr.is_null() {
        return None;
    }
    let key_value = JSValue::string_ptr(key as *mut crate::StringHeader);
    let value = crate::string::js_string_index_get(str_ptr, f64::from_bits(key_value.bits()));
    let js_value = JSValue::from_bits(value.to_bits());
    if js_value.is_undefined() {
        None
    } else {
        Some(js_value)
    }
}

unsafe fn array_prototype_property_value(name: &str, receiver_addr: usize) -> Option<JSValue> {
    let ctor = super::js_get_global_this_builtin_value(b"Array".as_ptr(), 5);
    let ctor_value = JSValue::from_bits(ctor.to_bits());
    if !ctor_value.is_pointer() {
        return None;
    }
    let ctor_ptr = ctor_value.as_pointer::<u8>() as usize;
    let proto = crate::closure::closure_get_dynamic_prop(ctor_ptr, "prototype");
    let proto_value = JSValue::from_bits(proto.to_bits());
    if !proto_value.is_pointer() {
        return None;
    }
    let proto_ptr = proto_value.as_pointer::<u8>() as usize;
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    if let Some(v) = own_data_field_by_name(proto_ptr as *const ObjectHeader, key) {
        return Some(v);
    }
    if let Some(v) = crate::array::array_named_property_get_by_name(
        proto_ptr as *const crate::array::ArrayHeader,
        name,
    ) {
        return Some(JSValue::from_bits(v.to_bits()));
    }
    if proto_ptr == receiver_addr {
        return None;
    }
    let v = js_object_get_field_by_name(proto_ptr as *const ObjectHeader, key);
    if v.is_undefined() {
        None
    } else {
        Some(v)
    }
}

// Issue #922: Rate-limit and bound the [WARN_NULL_PTR] message stream
// + abort the process when a runaway loop is detected.
//
// Background: when codegen emits an `Expr::New { ... }` whose constructor
// args include a NULL POINTER_TAG (typically the result of a cross-module
// reference to an export that didn't link, or an async-step rejected-
// before-resolved capture), every constructor invocation calls
// `js_object_set_field` once per field. Each call previously emitted one
// `eprintln!` line. The gscmaster-api production loop (#922) printed
// 5.7M+ identical lines on a single Fastify route hit before PM2
// declared the process dead -- actionable signal drowned in noise.
//
// Hard limits + circuit breaker:
//   * The per-call [WARN_NULL_PTR] log line is gated behind PERRY_DEBUG=1
//     (issue #924) and ALSO rate-limited to `WARN_NULL_PTR_LOG_LIMIT`
//     (=64) per thread under PERRY_DEBUG so even debug runs don't drown
//     in noise. After the limit a one-time `...further entries suppressed`
//     notice fires.
//   * `WARN_NULL_PTR_ABORT_LIMIT` (=100_000) -- if the SAME obj+
//     field_index has been written with a null POINTER_TAG this many
//     times consecutively, eprintln a one-line diagnostic and trigger
//     `std::process::abort()`. This is UNCONDITIONAL (not gated by
//     PERRY_DEBUG) because a 100K-iteration same-site loop is real
//     corruption, not happy-path noise. The async-step reentry guard
//     at `crates/perry-runtime/src/promise.rs::ASYNC_STEP_REENTRY_BOUND`
//     bounds the loop at 10K iterations BEFORE this fires in the normal
//     case; this is the catch-all for paths the async-step guard misses
//     (e.g. sync `throw_not_callable` inside a non-async fastify hook).
const WARN_NULL_PTR_LOG_LIMIT: u64 = 64;
const WARN_NULL_PTR_ABORT_LIMIT: u64 = 100_000;

thread_local! {
    static WARN_NULL_PTR_STATE: std::cell::Cell<WarnNullPtrState>
        = const { std::cell::Cell::new(WarnNullPtrState {
            total_count: 0,
            last_obj: 0,
            last_field_index: u32::MAX,
            consecutive_same_site: 0,
        }) };
}

#[derive(Copy, Clone)]
struct WarnNullPtrState {
    total_count: u64,
    last_obj: usize,
    last_field_index: u32,
    consecutive_same_site: u64,
}

#[cold]
#[inline(never)]
fn record_warn_null_ptr(obj: *mut ObjectHeader, field_index: u32, class_id: u32) {
    let (total_count, should_abort) = WARN_NULL_PTR_STATE.with(|cell| {
        let mut s = cell.get();
        s.total_count = s.total_count.saturating_add(1);
        let same_site = s.last_obj == obj as usize && s.last_field_index == field_index;
        s.consecutive_same_site = if same_site {
            s.consecutive_same_site.saturating_add(1)
        } else {
            1
        };
        s.last_obj = obj as usize;
        s.last_field_index = field_index;
        let total = s.total_count;
        let abort = s.consecutive_same_site >= WARN_NULL_PTR_ABORT_LIMIT;
        cell.set(s);
        (total, abort)
    });
    // perry#924: the per-call log is gated behind PERRY_DEBUG=1. Even
    // under PERRY_DEBUG we cap at WARN_NULL_PTR_LOG_LIMIT occurrences
    // per thread (issue #922 -- the production loop produced 5.7M of
    // these and the actionable signal got buried).
    if total_count <= WARN_NULL_PTR_LOG_LIMIT && std::env::var_os("PERRY_DEBUG").is_some() {
        eprintln!(
            "[WARN_NULL_PTR] js_object_set_field: null POINTER_TAG at obj={:p} field_index={} class_id={} -- replacing with undefined",
            obj, field_index, class_id
        );
        if total_count == WARN_NULL_PTR_LOG_LIMIT {
            eprintln!(
                "[WARN_NULL_PTR] further entries suppressed after {} occurrences -- this usually indicates an unresolved import or an uninitialized cross-module export being constructed into an object field",
                WARN_NULL_PTR_LOG_LIMIT
            );
        }
    }
    if should_abort {
        eprintln!(
            "[PERRY ABORT] js_object_set_field: detected runaway null POINTER_TAG writes at obj={:p} field_index={} class_id={} ({}+ consecutive same-site writes -- issue #922 circuit breaker). Common cause: an async function throws across an await boundary inside try/catch AND the catch arm re-enters the same await, OR an unresolved import was constructed into a field. Convert to a result-tag pattern (see issue #921 workaround) or check perry --print-hir for an uninitialized capture.",
            obj, field_index, class_id, WARN_NULL_PTR_ABORT_LIMIT
        );
        std::process::abort();
    }
}

/// Set a field on an object by index
#[no_mangle]
pub extern "C" fn js_object_set_field(obj: *mut ObjectHeader, field_index: u32, value: JSValue) {
    let obj = {
        let b = obj as u64;
        let t = b >> 48;
        if t >= 0x7FF8 {
            if t == 0x7FFC
                || (b & 0x0000_FFFF_FFFF_FFFF) == 0
                || (b & 0x0000_FFFF_FFFF_FFFF) < 0x10000
            {
                return;
            }
            (b & 0x0000_FFFF_FFFF_FFFF) as *mut ObjectHeader
        } else {
            obj
        }
    };
    if obj.is_null() || (obj as usize) < 0x10000 {
        return;
    }
    unsafe {
        // Bounds check: guard against out-of-range field writes that corrupt adjacent
        // arena allocations. js_object_alloc_with_shape uses max(field_count, 8) physical
        // slots, but the stored field_count is the logical count. Class objects from
        // js_object_alloc_class_with_keys use exactly field_count slots.
        // We use a generous limit of max(field_count, 8) to avoid false positives from
        // js_object_alloc_with_shape's extra padding while still catching real overflows.
        let stored_field_count = (*obj).field_count;
        let alloc_limit = std::cmp::max(stored_field_count, 8);
        if field_index >= alloc_limit {
            eprintln!(
                "[PERRY WARN] js_object_set_field: OOB write field_index={} alloc_limit={} (field_count={}) obj={:p} class_id={}",
                field_index, alloc_limit, stored_field_count, obj, (*obj).class_id
            );
            return;
        }
        // Guard: null POINTER_TAG (0x7FFD_0000_0000_0000) is never legitimate -- replace with undefined.
        // The diagnostic + circuit breaker live in `record_warn_null_ptr` (issue #922).
        // perry#924: the [WARN_NULL_PTR] log line itself is gated behind
        // `PERRY_DEBUG=1` inside `record_warn_null_ptr`; the circuit
        // breaker abort path is unconditional (it's a real corruption
        // signal, not happy-path noise).
        let vbits = value.bits();
        let value = if (vbits >> 48) == 0x7FFD && (vbits & 0x0000_FFFF_FFFF_FFFF) == 0 {
            record_warn_null_ptr(obj, field_index, (*obj).class_id);
            JSValue::undefined()
        } else {
            value
        };
        let fields_ptr = (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut JSValue;
        let slot = fields_ptr.add(field_index as usize);
        crate::gc::runtime_store_jsvalue_slot(
            obj as usize,
            slot as usize,
            field_index as usize,
            value.bits(),
        );
    }
}

/// Get the class ID of an object.
///
/// Returns 0 unless `obj` is a real GC-arena-allocated class instance.
/// Issue #350 (round 2): the codegen's `idispatch` tower for unknown-receiver
/// method calls (e.g. `set.has(c)` when the static type is `ReadonlySet<T>`,
/// or `a.componentTypeSet.has(c)` where `a` is `Archetype | undefined`) uses
/// this function to compare the receiver's class id against every user
/// class implementing the same method name. Without the GC-type guard we
/// blindly read 4 bytes at offset 4 of the receiver — which for a
/// `SetHeader` (allocated via std::alloc, no GcHeader, layout
/// `{ size: u32, capacity: u32, elements: *mut f64 }`) is its `capacity`
/// field. `js_set_alloc(0)` defaults capacity to 4, which collides with
/// whichever user class lands at id 4, routing the call into the wrong
/// method body and crashing on the bogus `this` pointer.
#[no_mangle]
pub extern "C" fn js_object_get_class_id(obj: *const ObjectHeader) -> u32 {
    if obj.is_null() || (obj as usize) < 0x100000 {
        return 0;
    }
    let addr = obj as usize;
    // Built-in headers (Set / Map / Regex) live in their own per-type
    // registries — they're never user class instances. Reject them first
    // so we never try to read a GcHeader at obj-8, which doesn't exist
    // for these std::alloc'd headers.
    if crate::set::is_registered_set(addr)
        || crate::map::is_registered_map(addr)
        || crate::regex::is_regex_pointer(obj as *const u8)
    {
        return 0;
    }
    unsafe {
        if !is_valid_obj_ptr(obj as *const u8) {
            return 0;
        }
        let gc_header =
            (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc_header).obj_type != crate::gc::GC_TYPE_OBJECT {
            return 0;
        }
        (*obj).class_id
    }
}

/// Free an object (for manual memory management / testing)
#[no_mangle]
pub extern "C" fn js_object_free(_obj: *mut ObjectHeader) {
    // No-op: GC handles deallocation of arena-allocated objects
}

/// Convert an object pointer to a JSValue
#[no_mangle]
pub extern "C" fn js_object_to_value(obj: *const ObjectHeader) -> JSValue {
    JSValue::pointer(obj as *const u8)
}

/// Extract an object pointer from a JSValue
#[no_mangle]
pub extern "C" fn js_value_to_object(value: JSValue) -> *mut ObjectHeader {
    value.as_pointer::<ObjectHeader>() as *mut ObjectHeader
}

/// Get a field as f64 (returns raw JSValue bits as f64)
/// This preserves NaN-boxing for strings and other pointer types
#[no_mangle]
pub extern "C" fn js_object_get_field_f64(obj: *const ObjectHeader, field_index: u32) -> f64 {
    let value = js_object_get_field(obj, field_index);
    f64::from_bits(value.bits())
}

/// Set a field from f64 (interprets raw bits as JSValue)
/// This preserves NaN-boxing for strings and other pointer types
#[no_mangle]
pub extern "C" fn js_object_set_field_f64(obj: *mut ObjectHeader, field_index: u32, value: f64) {
    // Check frozen flag — frozen objects reject all writes
    if !obj.is_null() && (obj as usize) > 0x10000 {
        unsafe {
            let gc =
                (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            if (*gc)._reserved & crate::gc::OBJ_FLAG_FROZEN != 0 {
                return;
            }
        }
    }
    js_object_set_field(obj, field_index, JSValue::from_bits(value.to_bits()));
}

/// Store a raw f64 into an object field slot for the unboxed numeric-field prototype.
///
/// This is only intended for construction sites whose static type has already
/// proven a raw-number slot. Dynamic writes still go through the normal setters,
/// which deopt the typed descriptor before tracing non-number values.
#[no_mangle]
pub extern "C" fn js_object_set_unboxed_f64_field(
    obj: *mut ObjectHeader,
    field_index: u32,
    value: f64,
) {
    let obj = {
        let b = obj as u64;
        let t = b >> 48;
        if t >= 0x7FF8 {
            if t == 0x7FFC
                || (b & 0x0000_FFFF_FFFF_FFFF) == 0
                || (b & 0x0000_FFFF_FFFF_FFFF) < 0x10000
            {
                return;
            }
            (b & 0x0000_FFFF_FFFF_FFFF) as *mut ObjectHeader
        } else {
            obj
        }
    };
    if obj.is_null() || (obj as usize) < 0x10000 {
        return;
    }
    unsafe {
        let gc = (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc)._reserved & crate::gc::OBJ_FLAG_FROZEN != 0 {
            return;
        }
        let stored_field_count = (*obj).field_count;
        let alloc_limit = std::cmp::max(stored_field_count, 8);
        if field_index >= alloc_limit {
            eprintln!(
                "[PERRY WARN] js_object_set_unboxed_f64_field: OOB write field_index={} alloc_limit={} (field_count={}) obj={:p} class_id={}",
                field_index, alloc_limit, stored_field_count, obj, (*obj).class_id
            );
            return;
        }
        let bits = value.to_bits();
        let fields_ptr = (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut u64;
        let slot = fields_ptr.add(field_index as usize);
        crate::gc::runtime_store_jsvalue_slot(
            obj as usize,
            slot as usize,
            field_index as usize,
            bits,
        );
    }
}

/// Read a raw f64 object field slot used by the unboxed numeric-field prototype.
#[no_mangle]
pub extern "C" fn js_object_get_unboxed_f64_field(
    obj: *const ObjectHeader,
    field_index: u32,
) -> f64 {
    f64::from_bits(js_object_get_field(obj, field_index).bits())
}

/// Set a field by index with a raw f64 value (for dynamic object creation)
/// This is a convenience wrapper that takes field_index as u32 and value as f64.
/// Honors `Object.freeze` and per-key `writable: false` descriptors so codegen
/// paths that resolve property writes to a field index still respect the JS
/// invariants set up by `Object.defineProperty`.
#[no_mangle]
pub extern "C" fn js_object_set_field_by_index(
    obj: *mut ObjectHeader,
    key: *const crate::string::StringHeader,
    field_index: u32,
    value: f64,
) {
    if obj.is_null() || (obj as usize) < 0x10000 {
        return;
    }
    unsafe {
        // Frozen objects reject all writes.
        let gc = (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc)._reserved & crate::gc::OBJ_FLAG_FROZEN != 0 {
            return;
        }
        // Per-key writable / accessor check when the key string is provided.
        if !key.is_null() {
            let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let name_len = (*key).byte_len as usize;
            let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
            if let Ok(name) = std::str::from_utf8(name_bytes) {
                if ACCESSORS_IN_USE.with(|c| c.get()) {
                    if let Some(acc) = get_accessor_descriptor(obj as usize, name) {
                        if acc.set != 0 {
                            let closure = (acc.set & crate::value::POINTER_MASK)
                                as *const crate::closure::ClosureHeader;
                            if !closure.is_null() {
                                crate::closure::js_closure_call1(closure, value);
                            }
                        }
                        return;
                    }
                }
                if let Some(attrs) = get_property_attrs(obj as usize, name) {
                    if !attrs.writable() {
                        return;
                    }
                }
            }
        }
    }
    js_object_set_field(obj, field_index, JSValue::from_bits(value.to_bits()));
}

/// Set the keys array for an object (used for Object.keys() support)
/// The keys_array should be an array of string pointers
#[no_mangle]
pub extern "C" fn js_object_set_keys(obj: *mut ObjectHeader, keys_array: *mut ArrayHeader) {
    unsafe {
        set_object_keys_array(obj, keys_array);
    }
}

/// `Object.keys(value)` entry point that inspects the NaN-boxed *value* (not a
/// raw pointer) so it handles primitives safely. A string yields its index
/// keys `"0".."length-1"` (`Object.keys("abc") === ["0","1","2"]`); objects and
/// arrays delegate to `js_object_keys` (which already handles both, #323/#893);
/// other primitives (number/boolean/null/undefined) yield an empty array.
/// Without this, the codegen unboxed the argument to a raw pointer and a string
/// receiver (or an SSO inline value, which isn't a pointer at all) was
/// dereferenced as an `ObjectHeader` → SIGSEGV.
#[no_mangle]
pub extern "C" fn js_object_keys_value(value: f64) -> *mut ArrayHeader {
    let jv = JSValue::from_bits(value.to_bits());
    // #2818: ToObject(null/undefined) throws TypeError, matching Node.
    if jv.is_null() || jv.is_undefined() {
        super::has_own_helpers::throw_to_object_nullish_type_error();
    }
    if jv.is_any_string() {
        let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let len = match crate::string::str_bytes_from_jsvalue(value, &mut scratch) {
            Some((ptr, blen)) if !ptr.is_null() => unsafe {
                crate::string::compute_utf16_len(ptr, blen)
            },
            _ => 0,
        };
        let arr = crate::array::js_array_alloc(len.max(1));
        for i in 0..len {
            let s = i.to_string();
            let k = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
            crate::array::js_array_push(arr, JSValue::string_ptr(k));
        }
        return arr;
    }
    if crate::builtins::boxed_primitive_to_string_tag(value) == Some("String") {
        if let Some((_, payload)) = crate::builtins::boxed_primitive_payload(value) {
            let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
            let len = match crate::string::str_bytes_from_jsvalue(payload, &mut scratch) {
                Some((ptr, blen)) if !ptr.is_null() => unsafe {
                    crate::string::compute_utf16_len(ptr, blen)
                },
                _ => 0,
            };
            let arr = crate::array::js_array_alloc(len.max(1));
            for i in 0..len {
                let s = i.to_string();
                let k = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
                crate::array::js_array_push(arr, JSValue::string_ptr(k));
            }
            if jv.is_pointer() {
                let ptr = jv.as_pointer::<ObjectHeader>();
                let own = js_object_keys(ptr);
                let own_len = crate::array::js_array_length(own);
                for i in 0..own_len {
                    let key_val = crate::array::js_array_get(own, i);
                    crate::array::js_array_push_f64(arr, f64::from_bits(key_val.bits()));
                }
            }
            return arr;
        }
    }
    if jv.is_pointer() {
        let ptr = jv.as_pointer::<u8>() as usize;
        if ptr > 0 && ptr < 0x100000 {
            if let Some(dispatch) = super::class_registry::handle_own_property_names_dispatch() {
                let names = unsafe { dispatch(ptr as i64) };
                if names.to_bits() != crate::value::TAG_UNDEFINED {
                    let bits = names.to_bits();
                    if bits >> 48 == 0x7FFD {
                        let arr = (bits & crate::value::POINTER_MASK) as *mut ArrayHeader;
                        if !arr.is_null() {
                            return arr;
                        }
                    }
                }
            }
            return crate::array::js_array_alloc(0);
        }
        if crate::closure::is_closure_ptr(ptr) {
            return js_closure_dynamic_keys(ptr);
        }
        if ptr >= crate::gc::GC_HEADER_SIZE + 0x1000 {
            unsafe {
                let gc_header =
                    (ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
                if (*gc_header).obj_type == crate::gc::GC_TYPE_ERROR {
                    let props = crate::node_submodules::error_user_props(ptr);
                    let arr = crate::array::js_array_alloc(props.len() as u32);
                    let mut out = arr;
                    for (name, _) in props {
                        let key =
                            crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                        out = crate::array::js_array_push(out, JSValue::string_ptr(key));
                    }
                    return out;
                }
            }
        }
        return js_object_keys(ptr as *const ObjectHeader);
    }
    crate::array::js_array_alloc(0)
}

fn closure_dynamic_enumerable_props(ptr: usize) -> Vec<(String, f64)> {
    let mut props = crate::closure::closure_dynamic_props_snapshot(ptr)
        .into_iter()
        .filter(|(name, _)| {
            get_property_attrs(ptr, name)
                .map(|attrs| attrs.enumerable())
                .unwrap_or(true)
        })
        .collect::<Vec<_>>();
    for name in super::accessor_descriptor_keys_for_obj(ptr) {
        if props.iter().any(|(existing, _)| existing == &name) {
            continue;
        }
        if crate::closure::closure_is_key_deleted(ptr, &name) {
            continue;
        }
        if get_property_attrs(ptr, &name)
            .map(|attrs| attrs.enumerable())
            .unwrap_or(false)
        {
            let value = crate::closure::closure_get_dynamic_prop(ptr, &name);
            props.push((name, value));
        }
    }
    props
}

fn js_closure_dynamic_keys(ptr: usize) -> *mut ArrayHeader {
    let props = closure_dynamic_enumerable_props(ptr);
    let arr = crate::array::js_array_alloc(props.len() as u32);
    let mut out = arr;
    for (name, _) in props {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        out = crate::array::js_array_push(out, JSValue::string_ptr(key));
    }
    out
}

fn js_closure_dynamic_values(ptr: usize) -> *mut ArrayHeader {
    let props = closure_dynamic_enumerable_props(ptr);
    let arr = crate::array::js_array_alloc(props.len() as u32);
    let mut out = arr;
    for (_, value) in props {
        out = crate::array::js_array_push(out, JSValue::from_bits(value.to_bits()));
    }
    out
}

fn js_closure_dynamic_entries(ptr: usize) -> *mut ArrayHeader {
    let props = closure_dynamic_enumerable_props(ptr);
    let arr = crate::array::js_array_alloc(props.len() as u32);
    let mut out = arr;
    for (name, value) in props {
        let pair = crate::array::js_array_alloc(2);
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let pair = crate::array::js_array_push(pair, JSValue::string_ptr(key));
        let pair = crate::array::js_array_push(pair, JSValue::from_bits(value.to_bits()));
        out = crate::array::js_array_push(out, JSValue::array_ptr(pair));
    }
    out
}

/// Iterate a string value's characters, invoking `emit(index, char_str_value)`
/// for each. Returns the character count, or `None` if the value isn't a
/// valid string. Shared by `Object.values`/`Object.entries` on string args.
fn for_each_string_char<F: FnMut(u32, f64)>(value: f64, mut emit: F) -> Option<u32> {
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let (ptr, blen) = crate::string::str_bytes_from_jsvalue(value, &mut scratch)?;
    if ptr.is_null() {
        return Some(0);
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr, blen as usize) };
    let s = std::str::from_utf8(bytes).ok()?;
    let mut i = 0u32;
    for ch in s.chars() {
        let mut buf = [0u8; 4];
        let cs = ch.encode_utf8(&mut buf);
        let k = crate::string::js_string_from_bytes(cs.as_ptr(), cs.len() as u32);
        emit(i, f64::from_bits(JSValue::string_ptr(k).bits()));
        i += 1;
    }
    Some(i)
}

/// Tag-dispatching `Object.values(value)` — see [`js_object_keys_value`].
/// A string yields its characters (`Object.values("hi") === ["h","i"]`);
/// objects/arrays delegate to `js_object_values`; primitives yield `[]`.
#[no_mangle]
pub extern "C" fn js_object_values_value(value: f64) -> *mut ArrayHeader {
    let jv = JSValue::from_bits(value.to_bits());
    // #2818: ToObject(null/undefined) throws TypeError, matching Node.
    if jv.is_null() || jv.is_undefined() {
        super::has_own_helpers::throw_to_object_nullish_type_error();
    }
    if jv.is_any_string() {
        let arr = crate::array::js_array_alloc(1);
        let mut out = arr;
        if for_each_string_char(value, |_, ch| {
            out = crate::array::js_array_push(out, JSValue::from_bits(ch.to_bits()));
        })
        .is_none()
        {
            return crate::array::js_array_alloc(0);
        }
        return out;
    }
    if jv.is_pointer() {
        let ptr = jv.as_pointer::<u8>() as usize;
        if crate::closure::is_closure_ptr(ptr) {
            return js_closure_dynamic_values(ptr);
        }
        return js_object_values(ptr as *const ObjectHeader);
    }
    crate::array::js_array_alloc(0)
}

/// Tag-dispatching `Object.entries(value)` — see [`js_object_keys_value`].
/// A string yields `[[index, char], …]` (`Object.entries("hi") ===
/// [["0","h"],["1","i"]]`); objects/arrays delegate to `js_object_entries`;
/// primitives yield `[]`.
#[no_mangle]
pub extern "C" fn js_object_entries_value(value: f64) -> *mut ArrayHeader {
    let jv = JSValue::from_bits(value.to_bits());
    // #2818: ToObject(null/undefined) throws TypeError, matching Node.
    if jv.is_null() || jv.is_undefined() {
        super::has_own_helpers::throw_to_object_nullish_type_error();
    }
    if jv.is_any_string() {
        let outer = crate::array::js_array_alloc(1);
        let mut out = outer;
        if for_each_string_char(value, |idx, ch| {
            let pair = crate::array::js_array_alloc(2);
            let idx_s = idx.to_string();
            let idx_key = crate::string::js_string_from_bytes(idx_s.as_ptr(), idx_s.len() as u32);
            let p = crate::array::js_array_push(pair, JSValue::string_ptr(idx_key));
            let p = crate::array::js_array_push(p, JSValue::from_bits(ch.to_bits()));
            out = crate::array::js_array_push(out, JSValue::array_ptr(p));
        })
        .is_none()
        {
            return crate::array::js_array_alloc(0);
        }
        return out;
    }
    if jv.is_pointer() {
        let ptr = jv.as_pointer::<u8>() as usize;
        if crate::closure::is_closure_ptr(ptr) {
            return js_closure_dynamic_entries(ptr);
        }
        return js_object_entries(ptr as *const ObjectHeader);
    }
    crate::array::js_array_alloc(0)
}

/// Returns `Some(index)` if `s` is a canonical array-index string per ECMA-262
/// (the decimal form of an integer in `0..=2^32-2`, no leading zeros, no sign),
/// else `None`. These are the keys that `OrdinaryOwnPropertyKeys` enumerates
/// first, in ascending numeric order. (#2438)
pub(crate) fn canonical_array_index(s: &str) -> Option<u32> {
    let b = s.as_bytes();
    if b == b"0" {
        return Some(0);
    }
    // Non-empty, no leading zero, every byte an ASCII digit.
    if b.is_empty() || b[0] == b'0' || !b.iter().all(|c| c.is_ascii_digit()) {
        return None;
    }
    // Array-index range is `0..=2^32-2` (4294967294). 4294967295 is reserved
    // for `.length`, not a valid index; larger values are ordinary string keys.
    match s.parse::<u64>() {
        Ok(n) if n <= 4_294_967_294 => Some(n as u32),
        _ => None,
    }
}

/// Compute the position order that `OrdinaryOwnPropertyKeys` mandates for an
/// object's `keys_array`: array-index keys first in ascending numeric order,
/// then the remaining string keys in insertion order. Each returned `u32` is
/// an index into `keys_array` (which is parallel to the field slots), so a
/// caller can reorder both keys and values with the same permutation. (#2438)
///
/// Returns `None` when no key is an array index — i.e. the keys are already in
/// spec order — so callers keep their zero-extra-allocation insertion-order
/// fast path for the overwhelmingly common case.
pub(crate) unsafe fn ecma_own_key_order(keys: *const ArrayHeader) -> Option<Vec<u32>> {
    // Cheap first pass: bail with zero allocation when no key is an array
    // index — the overwhelmingly common case, where insertion order already
    // satisfies OrdinaryOwnPropertyKeys. (Also covers a null `keys`.)
    if !keys_contain_array_index(keys) {
        return None;
    }
    let len = crate::array::js_array_length(keys);
    let mut int_keys: Vec<(u32, u32)> = Vec::new();
    let mut str_positions: Vec<u32> = Vec::new();
    let mut sso_buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    for i in 0..len {
        let key_val = crate::array::js_array_get(keys, i);
        let idx = crate::string::js_string_key_bytes(key_val, &mut sso_buf)
            .and_then(|b| std::str::from_utf8(b).ok())
            .and_then(canonical_array_index);
        match idx {
            Some(n) => int_keys.push((n, i)),
            None => str_positions.push(i),
        }
    }
    // `int_keys` is non-empty here — `keys_contain_array_index` returned true.
    int_keys.sort_unstable_by_key(|&(n, _)| n);
    let mut out = Vec::with_capacity(len as usize);
    out.extend(int_keys.iter().map(|&(_, pos)| pos));
    out.extend(str_positions);
    Some(out)
}

/// Whether any key in `keys_array` is a canonical array index. Cheap predicate
/// for paths that just need to know whether spec reordering is required (e.g.
/// the JSON.stringify shape-template fast path) without building the full
/// permutation. (#2438)
pub(crate) unsafe fn keys_contain_array_index(keys: *const ArrayHeader) -> bool {
    if keys.is_null() {
        return false;
    }
    let len = crate::array::js_array_length(keys);
    let mut sso_buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    for i in 0..len {
        let key_val = crate::array::js_array_get(keys, i);
        let is_idx = crate::string::js_string_key_bytes(key_val, &mut sso_buf)
            .and_then(|b| std::str::from_utf8(b).ok())
            .and_then(canonical_array_index)
            .is_some();
        if is_idx {
            return true;
        }
    }
    false
}

/// Get the keys of an object as an array of strings.
/// If any key has a per-property descriptor with `enumerable: false`, that key is filtered out.
/// Otherwise (the common case), this returns the stored keys array directly.
#[no_mangle]
pub extern "C" fn js_object_keys(obj: *const ObjectHeader) -> *mut ArrayHeader {
    if obj.is_null() || !is_valid_obj_ptr(obj as *const u8) {
        // Issue #893: defensive sibling of `js_object_entries`'s
        // is_valid_obj_ptr filter — `Object.keys(undefined)` /
        // `Object.keys(ansiStyles)` (cross-module import) previously
        // dereferenced a low-48-bit-of-undefined pointer (~0x1) and
        // segfaulted. Return empty array.
        return crate::array::js_array_alloc(0);
    }
    // Issue #323: arrays land here too (the codegen routes every `Object.keys`
    // call through this entry point, regardless of receiver type). Treating an
    // ArrayHeader as an ObjectHeader read garbage from the slot-0 element bits
    // — `obj_type=length`, `keys_array=elements[1]` — which happened to look
    // null when slots were zero-filled. After the issue #323 init-to-HOLE fix,
    // slot[1] reads as TAG_HOLE which is non-null and segfaulted downstream.
    // Detect arrays by GC type byte and emit string indices for non-HOLE slots.
    let stripped = {
        let bits = obj as u64;
        let top16 = bits >> 48;
        if top16 == 0x7FFD || top16 >= 0x7FF8 {
            (bits & 0x0000_FFFF_FFFF_FFFF) as *const ObjectHeader
        } else {
            obj
        }
    };
    if crate::closure::is_closure_ptr(stripped as usize) {
        let props = crate::closure::closure_dynamic_props_snapshot(stripped as usize);
        let out = crate::array::js_array_alloc(props.len() as u32);
        for (name, _) in props {
            if matches!(name.as_str(), "length" | "name" | "prototype") {
                continue;
            }
            if let Some(attrs) = get_property_attrs(stripped as usize, &name) {
                if !attrs.enumerable() {
                    continue;
                }
            }
            let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            crate::array::js_array_push(out, JSValue::string_ptr(key));
        }
        return out;
    }
    if !stripped.is_null() && (stripped as usize) >= crate::gc::GC_HEADER_SIZE + 0x1000 {
        unsafe {
            let gc_header = (stripped as *const u8).sub(crate::gc::GC_HEADER_SIZE)
                as *const crate::gc::GcHeader;
            if (*gc_header).obj_type == crate::gc::GC_TYPE_ARRAY {
                let arr = stripped as *const crate::array::ArrayHeader;
                let length = (*arr).length;
                if length > 100_000 {
                    return crate::array::js_array_alloc(0);
                }
                let elements = (arr as *const u8)
                    .add(std::mem::size_of::<crate::array::ArrayHeader>())
                    as *const u64;
                let result = crate::array::js_array_alloc(length);
                for i in 0..length {
                    if std::ptr::read(elements.add(i as usize)) == crate::value::TAG_HOLE {
                        continue;
                    }
                    // Format `i` as decimal into a stack buffer; SSO covers
                    // 0..=99999 (≤5 bytes), and a length-100k array hits the
                    // sanity-cap above so we never need a heap StringHeader.
                    let s = i.to_string();
                    let key_box = crate::string::js_string_new_sso(s.as_ptr(), s.len() as u32);
                    crate::array::js_array_push_f64(result, key_box);
                }
                for name in crate::array::array_named_property_names(arr, true) {
                    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                    crate::array::js_array_push(result, JSValue::string_ptr(key));
                }
                return result;
            }
        }
    }
    unsafe {
        if (*obj).class_id == NATIVE_MODULE_CLASS_ID {
            if let Some(module_name) = read_native_module_name(obj) {
                if let Some(keys) = native_module_enumerable_keys(&module_name) {
                    let include_permission = matches!(
                        module_name.as_str(),
                        "process" | "process.namespace" | "process.default"
                    ) && crate::process::process_permission_enabled();
                    let out =
                        crate::array::js_array_alloc(keys.len() as u32 + include_permission as u32);
                    for key_bytes in keys {
                        let key_str = crate::string::js_string_from_bytes(
                            key_bytes.as_ptr(),
                            key_bytes.len() as u32,
                        );
                        crate::array::js_array_push(out, JSValue::string_ptr(key_str));
                    }
                    if include_permission {
                        let key_str = crate::string::js_string_from_bytes(
                            b"permission".as_ptr(),
                            b"permission".len() as u32,
                        );
                        crate::array::js_array_push(out, JSValue::string_ptr(key_str));
                    }
                    return out;
                }
            }
        }
        let keys = (*obj).keys_array;
        if keys.is_null() {
            return crate::array::js_array_alloc(0);
        }
        // Per JS spec, `Object.keys` must return a fresh array — callers
        // can `.sort()`, `.push()`, etc. without mutating the receiver.
        // Pre-fix this fast path returned the object's own internal
        // `keys_array` pointer, so `Object.keys(o).sort()` reordered
        // `o`'s key→slot mapping and subsequent `o.foo` reads returned
        // the wrong slot's value. The slow path below already builds a
        // fresh array; the fast path now mirrors it, just without the
        // per-key descriptor check.
        let has_descriptors =
            PROPERTY_DESCRIPTORS.with(|m| m.borrow().keys().any(|(ptr, _)| *ptr == obj as usize));
        let len = crate::array::js_array_length(keys) as usize;
        // #2438: enumerate in ECMA-262 OrdinaryOwnPropertyKeys order —
        // array-index keys first (ascending numeric), then string keys in
        // insertion order. `None` means no array-index keys, so insertion
        // order already matches spec and we walk `0..len` with no extra alloc.
        let order = ecma_own_key_order(keys);
        let pos = |j: usize| -> u32 {
            match &order {
                Some(ord) => ord[j],
                None => j as u32,
            }
        };
        if !has_descriptors {
            let out = crate::array::js_array_alloc(len as u32);
            for j in 0..len {
                let key_val = crate::array::js_array_get(keys, pos(j));
                crate::array::js_array_push_f64(out, f64::from_bits(key_val.bits()));
            }
            return out;
        }
        // Slow path: filter out non-enumerable keys.
        let filtered = crate::array::js_array_alloc(len as u32);
        let mut sso_buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        for j in 0..len {
            let key_val = crate::array::js_array_get(keys, pos(j));
            // #1781: accept inline SSO short keys (≤5 bytes) — the
            // pre-fix `is_string()` skipped them and Object.keys silently
            // dropped them from the result.
            let name_bytes = match crate::string::js_string_key_bytes(key_val, &mut sso_buf) {
                Some(b) => b,
                None => continue,
            };
            let key_str = match std::str::from_utf8(name_bytes) {
                Ok(s) => s,
                Err(_) => continue,
            };
            // If a descriptor explicitly marks this key non-enumerable, skip it.
            if let Some(attrs) = get_property_attrs(obj as usize, key_str) {
                if !attrs.enumerable() {
                    continue;
                }
            }
            crate::array::js_array_push_f64(filtered, f64::from_bits(key_val.bits()));
        }
        filtered
    }
}

/// Get the values of an object as an array
/// Returns an array of the object's field values
#[no_mangle]
pub extern "C" fn js_object_values(obj: *const ObjectHeader) -> *mut ArrayHeader {
    if obj.is_null() || !is_valid_obj_ptr(obj as *const u8) {
        // Issue #893: defensive sibling of `js_object_entries` —
        // see that function's comment for the rationale.
        return crate::array::js_array_alloc(0);
    }
    unsafe {
        // Iterate up to keys_len (logical property count), not
        // field_count — same fix as Object.entries above. Without
        // this, objects with overflow fields silently returned only
        // their first 8 values.
        let keys = (*obj).keys_array;
        let count = if !keys.is_null() {
            crate::array::js_array_length(keys) as usize
        } else {
            (*obj).field_count as usize
        };
        let result = crate::array::js_array_alloc(count as u32);

        // #2438: walk slots in OrdinaryOwnPropertyKeys order so values line up
        // with the spec key order (and with `Object.keys`/`Object.entries`).
        let order = ecma_own_key_order(keys);
        let pos = |j: usize| -> u32 {
            match &order {
                Some(ord) => ord[j],
                None => j as u32,
            }
        };
        for j in 0..count {
            let value = js_object_get_field(obj as *mut ObjectHeader, pos(j));
            crate::array::js_array_push_f64(result, f64::from_bits(value.bits()));
        }

        result
    }
}

/// Get the entries of an object as an array of [key, value] pairs
/// Returns an array where each element is a 2-element array [key, value]
#[no_mangle]
pub extern "C" fn js_object_entries(obj: *const ObjectHeader) -> *mut ArrayHeader {
    if obj.is_null() || !is_valid_obj_ptr(obj as *const u8) {
        // Issue #893 lineage: chalk's `Object.entries(ansiStyles)` passed a
        // value whose unboxed low-48 bits weren't a real heap pointer
        // (cross-module import where the default-export wrapper hasn't
        // finished initializing). Pre-fix the `(*obj).keys_array` deref
        // SIGSEGV'd at 0x14; now we return an empty array so the user's
        // `for (const [k, v] of Object.entries(undefined)) {}` no-ops the
        // way the spec's "abstract conversion to object" path would for
        // an unrecognized receiver. Real JS throws TypeError here; we
        // prefer the empty-array fallback because Perry doesn't have a
        // clean "throw at codegen-call boundaries" path for these
        // pointer-typed entry points and a segfault is strictly worse
        // for the caller.
        return crate::array::js_array_alloc(0);
    }
    unsafe {
        let keys = (*obj).keys_array;
        // Iterate up to keys_len (the logical property count), not
        // field_count. Parser-built and dict-built objects with ≥9
        // fields cap field_count at the inline alloc_limit (8) and
        // store overflow values in OVERFLOW_FIELDS — for those,
        // field_count under-counts the actual property count by N-8.
        // Without this fix, `Object.entries(obj)` on a 50-key dict
        // returned only the first 8 entries (silent data loss).
        // Mirrors the same fix in `js_object_keys` and the
        // `actual_fields = keys_len` line in `json.rs::stringify_object`.
        let count = if !keys.is_null() {
            crate::array::js_array_length(keys) as usize
        } else {
            (*obj).field_count as usize
        };
        let result = crate::array::js_array_alloc(count as u32);

        // #2438: emit pairs in OrdinaryOwnPropertyKeys order (array-index keys
        // first, ascending; then string keys in insertion order).
        let order = ecma_own_key_order(keys);
        let pos = |j: usize| -> u32 {
            match &order {
                Some(ord) => ord[j],
                None => j as u32,
            }
        };
        for j in 0..count {
            let i = pos(j);
            // Create a pair array [key, value]
            let pair = crate::array::js_array_alloc(2);

            // Get the key (from keys array — already validated non-null
            // when count came from there).
            if !keys.is_null() && i < crate::array::js_array_length(keys) {
                let key = crate::array::js_array_get_f64(keys, i);
                crate::array::js_array_push_f64(pair, key);
            } else {
                crate::array::js_array_push_f64(pair, 0.0);
            }

            // Read the value. `js_object_get_field` handles the
            // inline-vs-overflow split internally (inline if
            // i < field_count, overflow_get otherwise).
            let value = js_object_get_field(obj as *mut ObjectHeader, i);
            crate::array::js_array_push_f64(pair, f64::from_bits(value.bits()));

            // Push the pair to result (NaN-box the array pointer)
            let pair_boxed = crate::value::js_nanbox_pointer(pair as i64);
            crate::array::js_array_push_f64(result, pair_boxed);
        }

        result
    }
}

/// Check if a property exists in an object by its string key name
/// Returns NaN-boxed true if the property exists, NaN-boxed false otherwise
/// This implements the JavaScript 'in' operator: "key" in obj
#[no_mangle]
pub extern "C" fn js_object_has_property(obj: f64, key: f64) -> f64 {
    let nanbox_false = f64::from_bits(0x7FFC_0000_0000_0003u64); // TAG_FALSE
    let nanbox_true = f64::from_bits(0x7FFC_0000_0000_0004u64); // TAG_TRUE

    let obj_val = JSValue::from_bits(obj.to_bits());
    let key_val = JSValue::from_bits(key.to_bits());

    // #1758: a SYMBOL key. The class-ref path below + the keys_array scan
    // (string keys only) can't see a class-object's static `[Sym]` props nor
    // ones inherited from a class-expression parent. Delegate to the symbol
    // resolver (handles INT32 class refs, POINTER class-objects, own +
    // prototype-chain), mirroring the string-key "present-and-not-undefined"
    // semantics. Fixes effect's `Predicate.hasProperty(classObj, TypeId)`
    // (`isSchema` → `dual` → `transformOrFail`) and `Sym in obj` generally.
    if unsafe { crate::symbol::js_is_symbol(key) } != 0 {
        let v = unsafe { crate::symbol::js_object_get_symbol_property(obj, key) };
        return if v.to_bits() != crate::value::TAG_UNDEFINED {
            nanbox_true
        } else {
            nanbox_false
        };
    }

    // Refs #420 / #618: `Symbol in ClassRef` — drizzle's `entityKind in cls`.
    // Class refs are INT32-tagged. Check CLASS_STATIC_SYMBOLS for symbol
    // keys and CLASS_DYNAMIC_PROPS for string keys.
    {
        let bits = obj.to_bits();
        if (bits >> 48) == 0x7FFE {
            let class_id = (bits & 0xFFFF_FFFF) as u32;
            // Symbol key path.
            if let Some(_) = crate::symbol::class_static_symbol_lookup(class_id, key) {
                return nanbox_true;
            }
            // String key path: check CLASS_DYNAMIC_PROPS via the get-by-name fn.
            if !key_val.is_pointer() && key_val.is_string() {
                // is_string covers heap StringHeader. Route through the
                // CLASS_DYNAMIC_PROPS-aware get fn.
            }
            // Fallback: emit false for class refs that aren't in either table.
            return nanbox_false;
        }
    }

    if !obj_val.is_pointer() {
        // Web Streams handles are raw finite f64 ids, not NaN-boxed pointers.
        // Property reads already route these through the stdlib handle
        // dispatcher; mirror that for the `in` operator so `"closed" in reader`
        // observes getter-backed handle properties without dereferencing the id.
        let f = f64::from_bits(obj.to_bits());
        if key_val.is_any_string() && f.is_finite() && f > 0.0 && f.fract() == 0.0 {
            let id = f as usize;
            if (0x100000..0x200000).contains(&id) {
                if let Some(probe) = crate::object::stream_handle_probe() {
                    unsafe {
                        if probe(id) {
                            if let Some(dispatch) =
                                super::class_registry::handle_property_dispatch()
                            {
                                let key_ptr = crate::value::js_get_string_pointer_unified(key)
                                    as *const crate::StringHeader;
                                let name_ptr = (key_ptr as *const u8)
                                    .add(std::mem::size_of::<crate::StringHeader>());
                                let name_len = (*key_ptr).byte_len as usize;
                                let result = dispatch(id as i64, name_ptr, name_len);
                                if result.to_bits() != crate::value::TAG_UNDEFINED {
                                    return nanbox_true;
                                }
                            }
                        }
                    }
                }
            }
        }
        return nanbox_false;
    }

    let obj_addr = obj_val.bits() & 0x0000_FFFF_FFFF_FFFF;
    if obj_addr >= 0x10000 {
        let obj_ptr = obj_addr as *mut ObjectHeader;
        unsafe {
            if !obj_ptr.is_null() && (*obj_ptr).class_id == NATIVE_MODULE_CLASS_ID {
                let key_ptr =
                    crate::value::js_get_string_pointer_unified(key) as *const crate::StringHeader;
                let present = super::native_module::read_native_module_name(obj_ptr)
                    .as_deref()
                    .zip(super::has_own_helpers::str_from_string_header(key_ptr))
                    .map(|(module, key)| {
                        super::native_module::native_module_has_enumerable_key(module, key)
                    })
                    .unwrap_or(false);
                return if present { nanbox_true } else { nanbox_false };
            }
        }
    }
    // Small handle receiver (`"prop" in crypto.createDiffieHellman(...)`,
    // Fastify handles, etc.). The generic object path below would treat the
    // handle id as an ObjectHeader pointer and can crash while reading
    // `keys_array`. Mirror the property-get IC miss path: ask the registered
    // handle property dispatcher whether the property resolves to a real
    // value.
    if obj_addr > 0 && obj_addr < 0x100000 {
        // #1781: accept inline SSO short keys (`"id" in handle`) — is_string()
        // is STRING_TAG-only, so a <=5-char key skipped the handle dispatcher
        // and `in` wrongly returned false. Materialize SSO bytes to a heap
        // header before reading name_ptr/name_len.
        if key_val.is_any_string() {
            unsafe {
                if let Some(dispatch) = super::class_registry::handle_property_dispatch() {
                    let key_ptr = crate::value::js_get_string_pointer_unified(key)
                        as *const crate::StringHeader;
                    let name_ptr =
                        (key_ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                    let name_len = (*key_ptr).byte_len as usize;
                    let result = dispatch(obj_addr as i64, name_ptr, name_len);
                    if result.to_bits() != crate::value::TAG_UNDEFINED {
                        return nanbox_true;
                    }
                }
            }
        }
        return nanbox_false;
    }

    let obj_ptr = obj_val.as_pointer::<ObjectHeader>();
    if obj_ptr.is_null() {
        return nanbox_false;
    }

    if unsafe { (*obj_ptr).class_id == NATIVE_MODULE_CLASS_ID } {
        if !key_val.is_any_string() {
            return nanbox_false;
        }
        let key_str =
            crate::value::js_get_string_pointer_unified(key) as *const crate::StringHeader;
        if key_str.is_null() {
            return nanbox_false;
        }
        let key_name = match unsafe { super::has_own_helpers::str_from_string_header(key_str) } {
            Some(name) => name,
            None => return nanbox_false,
        };
        let present = unsafe { read_native_module_name(obj_ptr) }
            .as_deref()
            .is_some_and(|module_name| native_module_has_enumerable_key(module_name, key_name));
        return if present { nanbox_true } else { nanbox_false };
    }

    // Issue #323: array fast path. `n in arr` with a numeric key was always
    // returning false because the receiver was treated as ObjectHeader and
    // the key-is-string guard below rejected the numeric key. Detect an
    // ArrayHeader by GC type byte; for numeric keys check `index < length`
    // and slot != TAG_HOLE (distinguishes a hole from an explicit
    // `arr[i] = undefined` write, the latter overwrites HOLE with UNDEFINED).
    if (obj_ptr as usize) >= crate::gc::GC_HEADER_SIZE + 0x1000 {
        unsafe {
            let gc_header =
                (obj_ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            if (*gc_header).obj_type == crate::gc::GC_TYPE_ARRAY {
                let arr = obj_ptr as *const crate::array::ArrayHeader;
                let length = (*arr).length;
                if length > 100_000 {
                    return nanbox_false;
                }
                // Numeric key: extract the index. Accept both NaN-boxed i32
                // and plain f64 (e.g. literal `1`) provided it's a
                // non-negative integer in range.
                let idx: Option<u32> = if key_val.is_int32() {
                    let i = key_val.as_int32();
                    if i >= 0 {
                        Some(i as u32)
                    } else {
                        None
                    }
                } else if key_val.is_number() {
                    let f = f64::from_bits(key_val.bits());
                    if f >= 0.0 && f.fract() == 0.0 && f < u32::MAX as f64 {
                        Some(f as u32)
                    } else {
                        None
                    }
                } else {
                    None
                };
                if let Some(idx) = idx {
                    if idx >= length {
                        return nanbox_false;
                    }
                    let elements = (arr as *const u8)
                        .add(std::mem::size_of::<crate::array::ArrayHeader>())
                        as *const u64;
                    if std::ptr::read(elements.add(idx as usize)) == crate::value::TAG_HOLE {
                        return nanbox_false;
                    }
                    return nanbox_true;
                }
                if key_val.is_any_string() {
                    let key_str = crate::value::js_get_string_pointer_unified(key)
                        as *const crate::StringHeader;
                    if !key_str.is_null() {
                        if let Some(key_name) =
                            super::has_own_helpers::str_from_string_header(key_str)
                        {
                            if key_name == "length" {
                                return nanbox_true;
                            }
                            if let Some(index) = super::canonical_array_index(key_name) {
                                if index < length {
                                    let elements = (arr as *const u8)
                                        .add(std::mem::size_of::<crate::array::ArrayHeader>())
                                        as *const u64;
                                    if std::ptr::read(elements.add(index as usize))
                                        != crate::value::TAG_HOLE
                                    {
                                        return nanbox_true;
                                    }
                                }
                                return nanbox_false;
                            }
                            if crate::array::array_named_property_has(arr, key_str) {
                                return nanbox_true;
                            }
                            if array_prototype_property_value(key_name, obj_ptr as usize).is_some()
                            {
                                return nanbox_true;
                            }
                        }
                    }
                }
                return nanbox_false;
            }
            // #1758: a CLOSURE receiver (functions ARE objects in JS, so
            // `key in fn` is valid). Pre-fix this fell through to the
            // keys_array scan below, which read `(*obj_ptr).keys_array` at
            // the closure's capture-slot offset — a NaN-boxed value, not a
            // real *ArrayHeader — and SIGSEGV'd in `js_array_length`. effect's
            // `dual`-wrapped helpers reach here (`<key> in someClosure` deep in
            // the fiber runtime). Mirror the closure read path
            // (`js_object_get_field_by_name`: `length` → arity, others →
            // CLOSURE_DYNAMIC_PROPS): present-and-not-undefined ⇒ true.
            if (*gc_header).obj_type == crate::gc::GC_TYPE_CLOSURE {
                if !key_val.is_any_string() {
                    return nanbox_false;
                }
                let key_str =
                    crate::value::js_get_string_pointer_unified(key) as *const crate::StringHeader;
                if key_str.is_null() {
                    return nanbox_false;
                }
                let v = js_object_get_field_by_name(obj_ptr, key_str);
                return if v.is_undefined() {
                    nanbox_false
                } else {
                    nanbox_true
                };
            }
        }
    }

    // #1781: accept inline SSO short keys here too — `"abc" in obj` for a
    // <=5-char key arrives as a SHORT_STRING_TAG value that is_string()
    // rejects, so `in` wrongly returned false. Materialize to a heap header
    // (stored keys in keys_array are always heap, so js_string_equals works).
    if !key_val.is_any_string() {
        return nanbox_false;
    }

    let key_str = crate::value::js_get_string_pointer_unified(key) as *const crate::StringHeader;

    unsafe {
        let keys = (*obj_ptr).keys_array;
        if keys.is_null() {
            return nanbox_false;
        }

        let key_count = crate::array::js_array_length(keys) as usize;
        for i in 0..key_count {
            let stored_key_val = crate::array::js_array_get(keys, i as u32);
            // #1781: accept inline SSO short keys (the closure-style
            // `key in obj` path previously dropped them too).
            if crate::string::js_string_key_matches(stored_key_val, key_str) {
                // Check if the field was deleted (set to undefined by delete operator)
                let field_val = js_object_get_field(obj_ptr, i as u32);
                if field_val.is_undefined() {
                    return nanbox_false;
                }
                return nanbox_true;
            }
        }

        nanbox_false
    }
}

/// Get a field by its string key name
/// Returns the field value or undefined if the key is not found
unsafe fn closure_dynamic_prop_by_key(obj: usize, key: *const crate::StringHeader) -> Option<f64> {
    if key.is_null() {
        return None;
    }
    let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    let key_len = (*key).byte_len as usize;
    let name = std::str::from_utf8(std::slice::from_raw_parts(key_ptr, key_len)).ok()?;
    let val = crate::closure::closure_get_dynamic_prop(obj, name);
    if val.to_bits() == crate::value::TAG_UNDEFINED {
        None
    } else {
        Some(val)
    }
}

unsafe fn native_module_own_field_by_key(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> Option<JSValue> {
    if key.is_null() {
        return None;
    }
    let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    let key_len = (*key).byte_len as usize;
    let target = std::slice::from_raw_parts(key_ptr, key_len);
    if target == b"__module__" {
        return None;
    }
    let keys = (*obj).keys_array;
    if keys.is_null() {
        return None;
    }
    let key_count = crate::array::js_array_length(keys);
    for i in 0..key_count {
        let stored = crate::array::js_array_get(keys, i);
        let mut sso_buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        if crate::string::js_string_key_bytes(stored, &mut sso_buf) == Some(target) {
            return Some(js_object_get_field(obj, i));
        }
    }
    None
}

#[no_mangle]
pub extern "C" fn js_object_get_field_by_name(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> JSValue {
    // #2846: the receiver may be a Proxy value that arrived through a generic
    // property read (e.g. `rec.proxy.a` where `rec = Proxy.revocable(...)`).
    // Proxies are encoded as small fake pointers; deref-ing one as an
    // ObjectHeader would read unmapped memory. Route to the proxy get dispatch,
    // which forwards to the target (or throws on a revoked proxy) — matching
    // Node. `js_proxy_is_proxy` validates the value is a *registered* proxy so a
    // real heap object whose address happens to be small isn't misrouted.
    {
        // Proxy ids live in [0xF0000, 0x100000); `js_proxy_is_proxy` confirms
        // it is a *registered* proxy before we route to the proxy getter.
        let addr = obj as u64;
        if (0xF0000..0x100000).contains(&addr) && !key.is_null() {
            const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
            let boxed = f64::from_bits(POINTER_TAG | (addr & 0x0000_FFFF_FFFF_FFFF));
            if crate::proxy::js_proxy_is_proxy(boxed) != 0 {
                let key_f64 = f64::from_bits(crate::value::js_nanbox_string(key as i64).to_bits());
                let v = crate::proxy::js_proxy_get(boxed, key_f64);
                return JSValue::from_bits(v.to_bits());
            }
        }
    }
    // #2128: a plain JS number value (a finite double or canonical NaN —
    // anything `JSValue::is_number` returns true for *minus* the raw-I64
    // pointer convention where top16 == 0) reaches this generic property-get
    // when codegen lacks static type info — e.g. drizzle's
    // `buildQueryFromSourceParams` mapping a chunk that happens to be a
    // bound-param number (`1` row-id, `31` age). Without this guard the
    // receiver's f64 bits get bit-cast to a pointer and the first downstream
    // helper that reads a GC header (`is_registered_set` here, `(*obj).field_*`
    // elsewhere) derefs unmapped memory and SIGSEGVs. Spec: property access
    // on a primitive number returns undefined for unknown keys (we don't
    // auto-box to Number.prototype here; that's handled by the method-dispatch
    // path, not this property-getter slow path). Heap pointers stored as raw
    // I64 (module-level objects) have top16 == 0 and are preserved by this
    // check.
    {
        let bits = obj as u64;
        let top16 = bits >> 48;
        // Two shapes of primitive-number receiver reach this generic slow
        // path: (a) a finite double whose top16 is neither a NaN-box tag
        // nor zero — most numbers (1.0 has top16 0x3FF0, -3.14 has
        // 0xC008...), and (b) the f64 +0.0 whose full bit pattern is
        // `0` — distinguishable from a raw heap pointer because real
        // ObjectHeader allocations live above 0x10000 and from null /
        // undefined because both are NaN-boxed with top16 == 0x7FFC.
        let is_primitive_number =
            (top16 != 0 && !(0x7FF9..=0x7FFF).contains(&top16)) || (top16 == 0 && bits == 0);
        if is_primitive_number {
            // #2138: auto-box the primitive number for the inherited
            // `.constructor` read so `n.constructor === Number` (and the
            // duck-type `value.constructor.name === "Number"` lodash/date-fns
            // use to discriminate primitives). Route through the same
            // `js_get_global_this_builtin_value` helper that backs bare-`Number`
            // identifier resolution so identity comparison holds. Other unknown
            // keys still return undefined per #2128 (was SIGSEGV pre-#2128).
            if !key.is_null() {
                unsafe {
                    let key_ptr =
                        (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                    let key_len = (*key).byte_len as usize;
                    let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                    if let Ok(name) = std::str::from_utf8(key_bytes) {
                        if let Some(v) =
                            primitive_object_prototype_accessor(name, f64::from_bits(bits))
                        {
                            return v;
                        }
                    }
                    if let Some(v) =
                        primitive_builtin_prototype_property(b"Number", key, f64::from_bits(bits))
                    {
                        return v;
                    }
                }
            }
            return JSValue::undefined();
        }
    }
    // Native module registry handles can arrive here either as raw small
    // integers or as POINTER_TAG-boxed small integers. Route them before any
    // GC-header probes such as Date/Promise checks.
    {
        let bits = obj as u64;
        let top16 = bits >> 48;
        let raw = if top16 == 0 {
            bits as usize
        } else if top16 == 0x7FFD {
            (bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else {
            0
        };
        if raw > 0 && raw < 0x100000 {
            if !key.is_null() {
                unsafe {
                    let key_ptr =
                        (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                    let key_len = (*key).byte_len as usize;
                    let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                    if is_timer_handle_method_key(key_bytes)
                        && crate::timer::is_known_timer_id(raw as i64)
                    {
                        let this_f64 =
                            f64::from_bits(crate::value::js_nanbox_pointer(raw as i64).to_bits());
                        let result = super::js_class_method_bind(this_f64, key_ptr, key_len);
                        return JSValue::from_bits(result.to_bits());
                    }
                    if key_bytes == b"constructor" {
                        let null_obj_ptr = &NULL_OBJECT_BYTES as *const NullObjectBytes as *mut u8;
                        return JSValue::from_bits(JSValue::pointer(null_obj_ptr).bits());
                    }
                    if let Some(dispatch) = handle_property_dispatch() {
                        let bits = dispatch(raw as i64, key_ptr, key_len);
                        return JSValue::from_bits(bits.to_bits());
                    }
                }
            }
            return JSValue::undefined();
        }
    }
    // #2089: a `Date` is a NaN-boxed pointer to an 8-byte `DateCell`. A
    // generic property read on it (`date.constructor`, `date[k]`, a method
    // read as a value) must NOT fall through to the object-deref path below —
    // the cell is far smaller than an `ObjectHeader`, so reading its
    // `keys_array`/field slots would deref unmapped memory. Resolve the few
    // meaningful reads here and return `undefined` for everything else
    // (matching property reads on the old value-type Date). `obj` may arrive
    // NaN-boxed (top16 == 0x7FFD) or as a raw-I64 pointer (top16 == 0).
    {
        let bits = obj as u64;
        let top16 = bits >> 48;
        let addr = if top16 == 0x7FFD {
            (bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else if top16 == 0 {
            bits as usize
        } else {
            0
        };
        if addr != 0 && crate::date::is_date_cell_addr(addr) {
            if !key.is_null() {
                unsafe {
                    let key_ptr =
                        (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                    let key_len = (*key).byte_len as usize;
                    let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                    if key_bytes == b"constructor" {
                        let v = js_get_global_this_builtin_value(b"Date".as_ptr(), 4);
                        return JSValue::from_bits(v.to_bits());
                    }
                }
            }
            return JSValue::undefined();
        }
    }
    // Issue #818 (Effect class-instance pattern): a V8 handle (JS_HANDLE_TAG
    // = 0x7FFB) reaches here when codegen routes a generic `PropertyGet`
    // through this slow path — e.g. `Effect.succeed(42).value` where the
    // call return was a JS handle but the HIR `js_transform` pass didn't
    // rewrite the consumer-side `.value` into `JsGetProperty` (because the
    // call lowered as a `StaticMethodCall`, not as a `JsCallMethod`). The
    // method-call counterpart in `js_call_method` already routes
    // JS_HANDLE_TAG values to V8 via JS_HANDLE_CALL_METHOD; do the same
    // here via JS_HANDLE_OBJECT_GET_PROPERTY so subsequent property reads
    // on a returned class instance reach the live V8 object instead of
    // falling to the small-handle dispatch (which only knows about
    // Fastify/axios/sqlite, not generic V8 handles).
    {
        let bits = obj as u64;
        if (bits >> 48) == 0x7FFB && !key.is_null() {
            let func_ptr = crate::value::JS_HANDLE_OBJECT_GET_PROPERTY
                .load(std::sync::atomic::Ordering::SeqCst);
            if !func_ptr.is_null() {
                let func: unsafe extern "C" fn(f64, *const i8, usize) -> f64 =
                    unsafe { std::mem::transmute(func_ptr) };
                unsafe {
                    let key_ptr =
                        (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                    let key_len = (*key).byte_len as usize;
                    let result = func(f64::from_bits(bits), key_ptr as *const i8, key_len);
                    return JSValue::from_bits(result.to_bits());
                }
            }
            return JSValue::undefined();
        }
    }
    // Issue #618-followup: read INT32-tagged class ref's dynamic property
    // from the side-table (mirror of the set-side intercept). For drizzle's
    // `SQL.Aliased` lookup pattern.
    {
        let bits = obj as u64;
        if (bits >> 48) == 0x7FFE && !key.is_null() {
            let class_id = (bits & 0xFFFF_FFFF) as u32;
            let class_value = f64::from_bits(bits);
            let is_prototype_ref = super::class_prototype_ref_id(class_value).is_some();
            unsafe {
                let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let name_len = (*key).byte_len as usize;
                let name = std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len))
                    .unwrap_or("");
                // v0.5.752: class_ref.constructor synthesizes back to the
                // same class ref so drizzle's
                // `Object.getPrototypeOf(value).constructor === Class` chain
                // collapses correctly (with v0.5.751's getPrototypeOf
                // returning the class ref for instance receivers). Refs
                // #420 / #618 followup.
                if is_prototype_ref
                    && name == "constructor"
                    && class_id != 0
                    && class_has_own_method(class_id, name)
                {
                    let value = class_prototype_method_value_for_name(class_id, name);
                    return JSValue::from_bits(value.to_bits());
                }
                if name == "constructor" && class_id != 0 && is_class_id_registered(class_id) {
                    let value = if is_prototype_ref {
                        super::class_constructor_ref_value(class_id)
                    } else {
                        class_value
                    };
                    return JSValue::from_bits(value.to_bits());
                }
                if name == "prototype"
                    && class_id != 0
                    && is_class_id_registered(class_id)
                    && !is_prototype_ref
                {
                    let value = super::class_prototype_ref_value(class_id);
                    return JSValue::from_bits(value.to_bits());
                }
                if class_id != 0 && class_has_own_method(class_id, name) {
                    let value = class_prototype_method_value_for_name(class_id, name);
                    return JSValue::from_bits(value.to_bits());
                }
                if is_prototype_ref {
                    if let Ok(registry) = CLASS_VTABLE_REGISTRY.read() {
                        if let Some(ref reg) = *registry {
                            let mut cid = class_id;
                            let mut depth = 0usize;
                            while depth < 32 {
                                if let Some(vtable) = reg.get(&cid) {
                                    if let Some(&getter_ptr) = vtable.getters.get(name) {
                                        let f: extern "C" fn(f64) -> f64 =
                                            std::mem::transmute(getter_ptr);
                                        return JSValue::from_bits(f(class_value).to_bits());
                                    }
                                }
                                match get_parent_class_id(cid) {
                                    Some(p) if p != 0 && p != cid => {
                                        cid = p;
                                        depth += 1;
                                    }
                                    _ => break,
                                }
                            }
                        }
                    }
                    return JSValue::undefined();
                }
                if !name.is_empty() {
                    if super::class_registry::class_is_key_deleted(class_id, name) {
                        return JSValue::undefined();
                    }
                    let result = CLASS_DYNAMIC_PROPS.with(|m| {
                        m.borrow()
                            .get(&class_id)
                            .and_then(|props| props.get(name).copied())
                    });
                    if let Some(v) = result {
                        return JSValue::from_bits(v.to_bits());
                    }
                    if super::class_registry::lookup_static_method_in_chain(class_id, name)
                        .is_some()
                    {
                        let heap_name = {
                            let layout =
                                std::alloc::Layout::from_size_align(name_len.max(1), 1).unwrap();
                            let ptr = std::alloc::alloc(layout);
                            std::ptr::copy_nonoverlapping(name_ptr, ptr, name_len);
                            ptr
                        };
                        let result = js_class_method_bind(class_value, heap_name, name_len);
                        return JSValue::from_bits(result.to_bits());
                    }
                    if let Some(v) = super::class_registry::class_static_accessor_getter_value(
                        class_id,
                        name,
                        class_value,
                    ) {
                        return JSValue::from_bits(v.to_bits());
                    }
                    // #1788: a subclass of a class-expression value
                    // (`class Sub extends make("A") {}`) inherits the parent
                    // class OBJECT's OWN per-evaluation static fields. The
                    // parent object was recorded as `class_id`'s static
                    // prototype at `extends` time; walk that chain (also
                    // covering multi-level `class Leaf extends Mid {}`).
                    if let Some(v) = super::class_registry::resolve_proto_chain_field(class_id, key)
                    {
                        if !v.is_undefined() && !v.is_null() {
                            return v;
                        }
                    }
                    // #36 / #321: the subclass extends a FUNCTION value
                    // (`class Svc extends Context.Tag(id)<...>() {}`). Read the
                    // named static off the parent closure — its OWN props
                    // (`Svc.key` → "Svc") plus, via the closure getter, its
                    // static prototype (`Svc._op` → "Tag" on TagProto).
                    if let Some(closure_ptr) = super::class_registry::class_parent_closure(class_id)
                    {
                        let v = crate::closure::closure_get_dynamic_prop(closure_ptr, name);
                        let vb = JSValue::from_bits(v.to_bits());
                        if !vb.is_undefined() && !vb.is_null() {
                            return vb;
                        }
                    }
                    // #2059: the constructor's built-in `name` own property —
                    // the class name. Checked last so an explicit static
                    // `name` member (method/field, handled above) still wins.
                    // This is what `assert.throws` reads via
                    // `thrown.constructor.name` to label the thrown error.
                    if name == "name" && class_id != 0 {
                        if !super::class_registry::class_is_key_deleted(class_id, name) {
                            if let Some(cname) = super::class_registry::class_name_for_id(class_id)
                            {
                                let s = crate::string::js_string_from_bytes(
                                    cname.as_ptr(),
                                    cname.len() as u32,
                                );
                                return JSValue::from_bits(
                                    crate::js_nanbox_string(s as i64).to_bits(),
                                );
                            }
                        }
                    }
                }
            }
            return JSValue::undefined();
        }
    }
    // #1545: Promise `then`/`catch`/`finally` value-reads return a bound
    // function so `typeof p.then === "function"`, `const f = p.then`, and
    // passing `p.then` as a deferred callback all work. (The call form
    // `p.then(cb)` is lowered directly to `js_promise_then` by codegen.)
    // `obj` arrives NaN-boxed POINTER-tagged here; mask to the raw promise
    // pointer and confirm via the GC header before treating it as a promise.
    {
        let bits = obj as u64;
        let top16 = bits >> 48;
        // Callers reach this helper with either a NaN-boxed POINTER-tagged
        // value (0x7FFD, e.g. the `_f64` wrapper) or an already-masked raw
        // heap pointer (top16 == 0, e.g. the PIC miss handler), so accept both.
        let raw = if top16 == 0x7FFD {
            (bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else if top16 == 0 {
            bits as usize
        } else {
            0
        };
        // Native-module registry handles live below 0x100000 and can also be
        // POINTER_TAG-boxed; do not walk back to a GcHeader for those.
        if raw >= 0x100000 && !key.is_null() && is_valid_obj_ptr(raw as *const u8) {
            {
                unsafe {
                    let gc_header = (raw - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
                    if (*gc_header).obj_type == crate::gc::GC_TYPE_PROMISE {
                        let name_ptr =
                            (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                        let name_len = (*key).byte_len as usize;
                        let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
                        if matches!(name_bytes, b"then" | b"catch" | b"finally") {
                            let prop = std::str::from_utf8_unchecked(name_bytes);
                            if let Some(v) = crate::promise::js_promise_bound_method(
                                raw as *mut crate::promise::Promise,
                                prop,
                            ) {
                                return JSValue::from_bits(v.to_bits());
                            }
                        }
                    }
                }
            }
        }
    }
    // SSO property access (v0.5.213 Step 1 gate). The codegen inline
    // `.length` path routes SHORT_STRING_TAG receivers here because
    // it doesn't yet know about the SSO tag. Handle `.length` by
    // reading the length byte directly from the NaN-box payload.
    // Other property accesses on an SSO string (e.g. `.charAt` via
    // `[0]`, `.slice`) aren't yet routed here — handled by the
    // string method dispatch in a future migration step; today they
    // fall through to "undefined" which matches the behavior for
    // string-valued property access on untyped locals in general.
    {
        let obj_bits = obj as u64;
        if (obj_bits & crate::value::TAG_MASK) == crate::value::SHORT_STRING_TAG {
            if !key.is_null() {
                unsafe {
                    let key_ptr =
                        (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                    let key_len = (*key).byte_len as usize;
                    let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                    if key_bytes == b"length" {
                        let len = (obj_bits & crate::value::SHORT_STRING_LEN_MASK)
                            >> crate::value::SHORT_STRING_LEN_SHIFT;
                        return JSValue::number(len as f64);
                    }
                }
            }
            return JSValue::undefined();
        }
    }
    // #1670: Web Streams handles are returned as `id as f64` (a normal
    // float, NOT NaN-boxed) just above the pointer-tagged small-handle band, so
    // an inline `res.body.locked` reaches this generic field-get with `obj`
    // carrying the IEEE-754 bits of the stream id.
    // The NaN-box-strip + small-handle branches below don't recognise it
    // (top16 is an ordinary exponent, not a tag; the value as a pointer is
    // far above 0x100000), so it would be dereferenced as a heap pointer →
    // segfault. Decode the float; when the stdlib probe confirms a live
    // stream handle, route the property read through the handle property
    // dispatcher (which carries the #1670 stream getter/method arms).
    // Mirrors the method-dispatch path in `native_call_method.rs` (#1545).
    // The typed-local path (`const b = res.body; b.locked`) lowers as a
    // 0-arg NativeMethodCall getter and never reaches here.
    {
        let f = f64::from_bits(obj as u64);
        if !key.is_null() && f.is_finite() && f > 0.0 && f.fract() == 0.0 {
            let id = f as usize;
            if (0x100000..0x200000).contains(&id) {
                if let Some(probe) = crate::object::stream_handle_probe() {
                    unsafe {
                        if probe(id) {
                            if let Some(dispatch) = handle_property_dispatch() {
                                let key_ptr = (key as *const u8)
                                    .add(std::mem::size_of::<crate::StringHeader>());
                                let key_len = (*key).byte_len as usize;
                                let bits = dispatch(id as i64, key_ptr, key_len);
                                return JSValue::from_bits(bits.to_bits());
                            }
                        }
                    }
                }
            }
        }
    }
    // #2058: a raw, unboxed finite f64 NUMBER receiver (e.g. `(5).toString`,
    // or `n.isPrototypeOf` where `n: number`) reaches here with its float
    // bits intact — numbers are NOT NaN-boxed in Perry, so `5.0` arrives as
    // 0x4014_0000_0000_0000. That is neither a NaN-box tag (top16 >= 0x7FF8)
    // nor a masked heap pointer (those have top16 == 0), so the generic
    // pointer logic below would dereference the float bits as an
    // `ObjectHeader` → SIGSEGV. Detect the primitive number first: return a
    // bound-method closure for the inherited Number/Object prototype methods
    // (so `typeof n.toString === "function"` holds and the value is
    // callable), and `undefined` for any other key (matching property reads
    // on primitives). Date timestamps and Web-Stream handles are raw f64 too,
    // but both are special-cased above, so they never reach this branch.
    {
        let bits = obj as u64;
        let f = f64::from_bits(bits);
        // A Date is now a NaN-boxed `DateCell` pointer (non-finite bit
        // pattern), intercepted earlier in this function, so it never reaches
        // this finite-number branch.
        if !key.is_null() && f.is_finite() && (bits >> 48) != 0 {
            unsafe {
                let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let name_len = (*key).byte_len as usize;
                let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
                if let Ok(name) = std::str::from_utf8(name_bytes) {
                    if let Some(v) = primitive_object_prototype_accessor(name, f) {
                        return v;
                    }
                }
                if let Some(v) = primitive_builtin_prototype_property(b"Number", key, f) {
                    return v;
                }
                if is_primitive_proto_method(name_bytes) {
                    let result = super::js_class_method_bind(f, name_ptr, name_len);
                    return JSValue::from_bits(result.to_bits());
                }
            }
            return JSValue::undefined();
        }
    }
    // Strip NaN-boxing tags if present (defensive: handle POINTER_TAG, UNDEFINED, NULL, etc.)
    let obj = {
        let bits = obj as u64;
        let top16 = bits >> 48;
        if top16 == 0x7FFD || top16 >= 0x7FF8 {
            // NaN-boxed value — extract lower 48 bits as pointer
            let raw = (bits & 0x0000_FFFF_FFFF_FFFF) as *const ObjectHeader;
            if raw.is_null() || top16 == 0x7FFC {
                // undefined/null tag or null pointer — return undefined
                return JSValue::undefined();
            }
            // Issue #340: small-handle receivers (raw < 0x100000) come
            // from native modules (axios, fastify, ioredis, ...) that
            // store objects in registries and expose integer ids. The
            // handle property dispatcher (registered by stdlib via
            // `js_register_handle_property_dispatch`) routes the
            // property name to the per-module accessor (e.g. axios
            // status/data, fastify req query/params/...). Without
            // this, every property access on those handles silently
            // returned undefined.
            if (raw as usize) > 0 && (raw as usize) < 0x100000 {
                if !key.is_null() {
                    unsafe {
                        let key_ptr =
                            (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                        let key_len = (*key).byte_len as usize;
                        let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                        if is_timer_handle_method_key(key_bytes)
                            && crate::timer::is_known_timer_id(raw as i64)
                        {
                            let this_f64 = f64::from_bits(
                                crate::value::js_nanbox_pointer(raw as i64).to_bits(),
                            );
                            let result = super::js_class_method_bind(this_f64, key_ptr, key_len);
                            return JSValue::from_bits(result.to_bits());
                        }
                    }
                    // Drizzle-sqlite blocker: synth `data.constructor` for
                    // small-handle native instances so drizzle's
                    // `isConfig(data)` duck-type via
                    // `data.constructor.name !== "Object"` doesn't crash on
                    // `(undefined).name` under #648's strict catch-all.
                    // Returning the existing NULL_OBJECT_BYTES stub (a real
                    // ObjectHeader-shape with no fields) makes `(stub).name`
                    // return undefined safely, and `undefined !== "Object"`
                    // makes isConfig return false at the first gate. Refs
                    // #645 deeper followup.
                    unsafe {
                        let key_ptr =
                            (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                        let key_len = (*key).byte_len as usize;
                        let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                        if key_bytes == b"constructor" {
                            if let Some(dispatch) = handle_property_dispatch() {
                                let bits = dispatch(raw as i64, key_ptr, key_len);
                                let value = JSValue::from_bits(bits.to_bits());
                                if !value.is_undefined() {
                                    return value;
                                }
                            }
                            let null_obj_ptr =
                                &NULL_OBJECT_BYTES as *const NullObjectBytes as *mut u8;
                            return JSValue::from_bits(JSValue::pointer(null_obj_ptr).bits());
                        }
                    }
                    if let Some(dispatch) = handle_property_dispatch() {
                        unsafe {
                            let key_ptr =
                                (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                            let key_len = (*key).byte_len as usize;
                            let bits = dispatch(raw as i64, key_ptr, key_len);
                            return JSValue::from_bits(bits.to_bits());
                        }
                    }
                }
                return JSValue::undefined();
            }
            raw
        } else {
            obj
        }
    };
    if obj.is_null() {
        return JSValue::undefined();
    }
    // Same handle-receiver path for already-stripped pointers — happens
    // when the codegen passes a raw i64 handle through the slow path.
    if (obj as usize) < 0x100000 {
        if !key.is_null() {
            unsafe {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                if is_timer_handle_method_key(key_bytes)
                    && crate::timer::is_known_timer_id(obj as i64)
                {
                    let this_f64 =
                        f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
                    let result = super::js_class_method_bind(this_f64, key_ptr, key_len);
                    return JSValue::from_bits(result.to_bits());
                }
            }
            if let Some(dispatch) = handle_property_dispatch() {
                unsafe {
                    let key_ptr =
                        (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                    let key_len = (*key).byte_len as usize;
                    let bits = dispatch(obj as i64, key_ptr, key_len);
                    return JSValue::from_bits(bits.to_bits());
                }
            }
        }
        return JSValue::undefined();
    }
    if (obj as usize) < 0x10000 {
        return JSValue::undefined();
    }
    unsafe {
        if crate::closure::is_closure_ptr(obj as usize) {
            if key.is_null() {
                return JSValue::undefined();
            }
            let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let key_len = (*key).byte_len as usize;
            let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
            if let Ok(name_str) = std::str::from_utf8(key_bytes) {
                if crate::closure::closure_is_key_deleted(obj as usize, name_str) {
                    return JSValue::undefined();
                }
                if matches!(name_str, "caller" | "arguments")
                    && crate::closure::closure_is_arrow(obj as *const crate::closure::ClosureHeader)
                {
                    crate::fs::validate::throw_type_error_with_code(
                        "Restricted function property access",
                        "ERR_INVALID_ARG_TYPE",
                    );
                }
                let val = crate::closure::closure_get_dynamic_prop(obj as usize, name_str);
                if val.to_bits() != crate::value::TAG_UNDEFINED {
                    return JSValue::from_bits(val.to_bits());
                }
                if name_str == "length" {
                    let closure_value = crate::value::js_nanbox_pointer(obj as i64);
                    if let Some(arity) =
                        super::native_module::bound_native_callable_value_arity(closure_value)
                    {
                        return JSValue::number(arity as f64);
                    }
                    if let Some(len) = super::native_module::builtin_closure_length(obj as usize) {
                        return JSValue::number(len as f64);
                    }
                    let length =
                        crate::closure::closure_length(obj as *const crate::closure::ClosureHeader);
                    return JSValue::number(length.unwrap_or(0) as f64);
                }
                if name_str == "name" {
                    let func_ptr =
                        (*(obj as *const crate::closure::ClosureHeader)).func_ptr as usize;
                    let fname =
                        crate::builtins::function_name_for_ptr(func_ptr).unwrap_or_default();
                    let s = crate::string::js_string_from_bytes(fname.as_ptr(), fname.len() as u32);
                    return JSValue::from_bits(crate::js_nanbox_string(s as i64).to_bits());
                }
            }
            return JSValue::undefined();
        }
        if let Some(val) = closure_dynamic_prop_by_key(obj as usize, key) {
            return JSValue::from_bits(val.to_bits());
        }
        // Buffers: BufferHeader is allocated via raw `alloc()` (no GcHeader)
        // and tracked in BUFFER_REGISTRY. Detect first so the GC header check
        // below doesn't read garbage one word before the BufferHeader.
        // Route `.length` to `js_buffer_length` (matches the codegen path that
        // routes through PropertyGet for chained `Buffer.from(...).length`
        // expressions where the static type isn't recognized as Buffer).
        if crate::buffer::is_registered_buffer(obj as usize) {
            if !key.is_null() {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                if let Some(value) = crypto_key_property_value(obj as usize, key_bytes) {
                    return value;
                }
                if key_bytes == b"length" || key_bytes == b"byteLength" {
                    let b = obj as *const crate::buffer::BufferHeader;
                    return JSValue::number(crate::buffer::js_buffer_length(b) as f64);
                }
                // ArrayBuffer.prototype `resizable` / `maxByteLength` getters.
                // Perry has no resizable ArrayBuffers, so `resizable` is always
                // false and `maxByteLength` equals `byteLength`. These live only
                // on ArrayBuffer (not DataView/SharedArrayBuffer/typed arrays),
                // which return `undefined` for them in Node — so scope to a
                // plain registered ArrayBuffer.
                if (key_bytes == b"resizable" || key_bytes == b"maxByteLength")
                    && crate::buffer::is_array_buffer(obj as usize)
                    && !crate::buffer::is_data_view(obj as usize)
                    && !crate::buffer::is_shared_array_buffer(obj as usize)
                {
                    if key_bytes == b"resizable" {
                        return JSValue::bool(false);
                    }
                    let b = obj as *const crate::buffer::BufferHeader;
                    return JSValue::number(crate::buffer::js_buffer_length(b) as f64);
                }
                if key_bytes == b"constructor" {
                    if crate::buffer::crypto_key_meta(obj as usize).is_some() {
                        let ctor =
                            super::js_get_global_this_builtin_value(b"CryptoKey".as_ptr(), 9);
                        return JSValue::from_bits(ctor.to_bits());
                    }
                    // #3657: a DataView's `.constructor` is the global
                    // `DataView`, not `Buffer` — checked before the
                    // Uint8Array/Buffer arms since a DataView slice is also a
                    // registered buffer.
                    if crate::buffer::is_data_view(obj as usize) {
                        let ctor = super::js_get_global_this_builtin_value(b"DataView".as_ptr(), 8);
                        return JSValue::from_bits(ctor.to_bits());
                    }
                    if crate::buffer::is_uint8array_buffer(obj as usize) {
                        let ctor =
                            super::js_get_global_this_builtin_value(b"Uint8Array".as_ptr(), 10);
                        return JSValue::from_bits(ctor.to_bits());
                    }
                    let module = b"buffer.Buffer";
                    return JSValue::from_bits(
                        js_create_native_module_namespace(module.as_ptr(), module.len()).to_bits(),
                    );
                }
                if crate::buffer::is_secret_key(obj as usize) {
                    if key_bytes == b"type" {
                        let s = crate::string::js_string_from_bytes(b"secret".as_ptr(), 6);
                        return JSValue::from_bits(JSValue::string_ptr(s).bits());
                    }
                    if key_bytes == b"symmetricKeySize" {
                        let b = obj as *const crate::buffer::BufferHeader;
                        return JSValue::number(crate::buffer::js_buffer_length(b) as f64);
                    }
                    if key_bytes == b"asymmetricKeyType" || key_bytes == b"asymmetricKeyDetails" {
                        return JSValue::undefined();
                    }
                }
                if key_bytes == b"buffer" || key_bytes == b"parent" {
                    let alias = crate::buffer::buffer_backing_array_buffer(obj as usize);
                    return JSValue::from_bits(
                        crate::value::js_nanbox_pointer(alias as i64).to_bits(),
                    );
                }
                if key_bytes == b"byteOffset" || key_bytes == b"offset" {
                    let offset = crate::buffer::buffer_byte_offset(obj as usize);
                    return JSValue::number(offset as f64);
                }
                // Issue #639 followup: method-as-value reads on a Buffer
                // (e.g. duck-type tests like `typeof v.readUInt8 === "function"`
                // in @perryts/mysql's `isBufferLike`) need to return a
                // bound-method closure so `typeof` reports `"function"` and
                // a subsequent call routes through `js_native_call_method`'s
                // existing `dispatch_buffer_method` arm. Pre-fix every
                // non-length read returned undefined, so duck tests failed
                // and the encoder fell through to its `String(buf)` fallback —
                // BLOB params got encoded as VAR_STRING and the INSERT
                // silently corrupted the binary column.
                if let Ok(name) = std::str::from_utf8(key_bytes) {
                    if is_buffer_method_name(name) {
                        let heap_name = {
                            let layout =
                                std::alloc::Layout::from_size_align(key_bytes.len().max(1), 1)
                                    .unwrap();
                            let ptr = std::alloc::alloc(layout);
                            std::ptr::copy_nonoverlapping(key_bytes.as_ptr(), ptr, key_bytes.len());
                            ptr
                        };
                        // Buffers are stored as raw f64-bitcast pointers
                        // (NOT NaN-boxed) per CLAUDE.md "Module-level
                        // variables" — but `js_native_call_method`'s
                        // buffer arm at line ~5031 strips both raw and
                        // NaN-boxed payloads via `(bits >> 48) >= 0x7FF8`,
                        // so wrapping in POINTER_TAG here is equally
                        // valid and matches `js_class_method_bind`.
                        let this_f64 =
                            f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
                        let result = js_class_method_bind(this_f64, heap_name, key_bytes.len());
                        return JSValue::from_bits(result.to_bits());
                    }
                }
            }
            return JSValue::undefined();
        }
        // Typed arrays (Int32Array/Float64Array/...): the `TypedArrayHeader` is
        // `std::alloc`'d (small) or GC-old-allocated (large), but in both cases
        // tracked in TYPED_ARRAY_REGISTRY, so detect via the side table before
        // the GC-header read below (which would read garbage for the small
        // `std::alloc` case). `.length`, `.byteLength`, `.byteOffset`, and
        // `.BYTES_PER_ELEMENT` lower as generic PropertyGet for multi-byte
        // numeric-length views whose static type the codegen doesn't recognize;
        // pre-fix, only Uint8Array worked (it's a registered buffer) so
        // multi-byte `.byteLength` returned undefined.
        if let Some(kind) = crate::typedarray::lookup_typed_array_kind(obj as usize) {
            if !key.is_null() {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                let ta = obj as *const crate::typedarray::TypedArrayHeader;
                let elem_size = crate::typedarray::elem_size_for_kind(kind);
                match key_bytes {
                    b"length" => {
                        let len = crate::typedarray::js_typed_array_length(ta);
                        return JSValue::number(len as f64);
                    }
                    b"byteLength" => {
                        let len = crate::typedarray::js_typed_array_length(ta);
                        return JSValue::number((len as usize * elem_size) as f64);
                    }
                    b"buffer" => {
                        let buf = crate::typedarray::typed_array_to_array_buffer(ta);
                        if buf.is_null() {
                            return JSValue::undefined();
                        }
                        return JSValue::from_bits(
                            crate::value::js_nanbox_pointer(buf as i64).to_bits(),
                        );
                    }
                    b"byteOffset" => return JSValue::number(0.0),
                    b"BYTES_PER_ELEMENT" => return JSValue::number(elem_size as f64),
                    _ => {}
                }
            }
            return JSValue::undefined();
        }
        // Sets: SetHeader is allocated via raw `alloc()` (no GcHeader),
        // so we can't safely read the byte preceding the pointer to
        // determine its type. Detect via the SET_REGISTRY first. Route
        // `.size` to `js_set_size` and synthesize method values for
        // prototype functions such as `.has`, which Node exposes through
        // ordinary property reads.
        if crate::set::is_registered_set(obj as usize) {
            if !key.is_null() {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                if key_bytes == b"size" {
                    let s = obj as *const crate::set::SetHeader;
                    return JSValue::number(crate::set::js_set_size(s) as f64);
                }
                if let Some(name) = set_method_value_name(key_bytes) {
                    // Return the SAME brand-checking thunk installed on
                    // Set.prototype so `const m = s.forEach; m.call(badThis)`
                    // throws a TypeError (and `m === Set.prototype.forEach`).
                    // Falls back to the legacy instance-bound closure if the
                    // prototype thunk isn't available.
                    if let Ok(method_name) = std::str::from_utf8(name) {
                        if let Some(v) =
                            super::collection_proto_thunks::collection_proto_method_value(
                                "Set",
                                method_name,
                            )
                        {
                            return JSValue::from_bits(v.to_bits());
                        }
                    }
                    let this_f64 =
                        f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
                    let result = js_class_method_bind(this_f64, name.as_ptr(), name.len());
                    return JSValue::from_bits(result.to_bits());
                }
            }
            return JSValue::undefined();
        }
        // Symbols: registered in SYMBOL_POINTERS by symbol.rs. Symbols
        // allocated via Symbol.for(...) are Box-leaked (no GcHeader), so
        // reading the byte before would be UB. Detect via the side table.
        if crate::symbol::is_registered_symbol(obj as usize) {
            if !key.is_null() {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                let sym_f64 =
                    f64::from_bits(0x7FFD_0000_0000_0000u64 | (obj as u64 & 0x0000_FFFF_FFFF_FFFF));
                if key_bytes == b"description" {
                    return JSValue::from_bits(
                        crate::symbol::js_symbol_description(sym_f64).to_bits(),
                    );
                }
            }
            return JSValue::undefined();
        }
        // Validate this is an ObjectHeader, not some other heap type.
        // Check GcHeader first (reliable for heap objects), then fallback to ObjectHeader.object_type
        // for static/const objects that don't have GcHeaders.
        // Guard: ensure we can safely read GC_HEADER_SIZE bytes before obj
        if (obj as usize) < crate::gc::GC_HEADER_SIZE + 0x1000
            || !is_valid_obj_ptr(obj as *const u8)
        {
            return JSValue::undefined();
        }
        let gc_header =
            (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        let gc_type = (*gc_header).obj_type;
        if gc_type != crate::gc::GC_TYPE_ARRAY && !is_valid_obj_ptr(obj as *const u8) {
            return JSValue::undefined();
        }
        // Issue #618: closures have their own GC type (GC_TYPE_CLOSURE=4)
        // distinct from GC_TYPE_OBJECT, but support dynamic-property storage
        // via the `CLOSURE_DYNAMIC_PROPS` side-table. `js_object_set_field_by_name`
        // routes writes there for the IIFE-namespace pattern
        // (`((sql2) => { sql2.identifier = ...; })(sql)`); mirror the read
        // path here so the companion get fires. Pre-fix the
        // `gc_type != GC_TYPE_OBJECT` arm below would early-return undefined
        // for any closure receiver, masking the dynamic-prop side-table.
        if gc_type == crate::gc::GC_TYPE_CLOSURE {
            if !key.is_null() {
                let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let name_len = (*key).byte_len as usize;
                let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
                // #3655: a `delete`d slot (`delete fn.name`, configurable:true)
                // reads back `undefined`, even though `name`/`length` are
                // otherwise synthesized from the registries below.
                if let Ok(name_str) = std::str::from_utf8(name_bytes) {
                    if crate::closure::closure_is_key_deleted(obj as usize, name_str) {
                        return JSValue::undefined();
                    }
                    if matches!(name_str, "caller" | "arguments")
                        && crate::closure::closure_is_arrow(
                            obj as *const crate::closure::ClosureHeader,
                        )
                    {
                        crate::fs::validate::throw_type_error_with_code(
                            "Restricted function property access",
                            "ERR_INVALID_ARG_TYPE",
                        );
                    }
                }
                // `fn.length` — return the registered ECMAScript-visible
                // length for the underlying function. Ramda's
                // `converge` / `useWith` / `addIndex` chain feeds
                // `pluck('length', fns)` through
                // `reduce(max, 0, …)` → `curryN(N, …)` → `_arity(N, …)`;
                // without a real number here that pipeline produces
                // `NaN`, and `_arity` throws
                // `First argument to _arity must be a non-negative
                // integer no greater than ten` at module init.
                if name_bytes == b"length" {
                    let closure_value = crate::value::js_nanbox_pointer(obj as i64);
                    if let Some(arity) =
                        super::native_module::bound_native_callable_value_arity(closure_value)
                    {
                        return JSValue::number(arity as f64);
                    }
                    // #3143: built-in proto methods share one func_ptr, so the
                    // func-ptr arity registry can't tell `map` (1) from `slice`
                    // (2) — read the per-closure recorded spec length first.
                    if let Some(len) = super::native_module::builtin_closure_length(obj as usize) {
                        return JSValue::number(len as f64);
                    }
                    let length =
                        crate::closure::closure_length(obj as *const crate::closure::ClosureHeader);
                    return JSValue::number(length.unwrap_or(0) as f64);
                }
                // #2145: `fn.__proto__` is the closure's [[Prototype]]
                // — `Int8Array.__proto__ === %TypedArray%` after
                // `populate_global_this_builtins` wired the static-proto
                // side-table. Spec models `__proto__` as a
                // `Object.prototype` accessor that returns
                // `[[GetPrototypeOf]](this)`; for closures Perry resolves
                // that off the same side-table `Object.setPrototypeOf`
                // writes to. Walking `closure_get_dynamic_prop` would
                // instead look for a `__proto__` own-prop on the parent,
                // which is the wrong thing — the proto IS the answer.
                // Returns undefined (not null) when no proto is recorded,
                // matching the closure-receiver `getPrototypeOf` arm
                // semantics for non-wired closures.
                if name_bytes == b"__proto__" {
                    if let Some(proto_bits) = crate::closure::closure_static_prototype(obj as usize)
                    {
                        return JSValue::from_bits(proto_bits);
                    }
                    return JSValue::undefined();
                }
                if let Ok(name_str) = std::str::from_utf8(name_bytes) {
                    // User-attached own property (`fn.x = 1`) takes precedence.
                    let val = crate::closure::closure_get_dynamic_prop(obj as usize, name_str);
                    if val.to_bits() != crate::value::TAG_UNDEFINED {
                        return JSValue::from_bits(val.to_bits());
                    }
                    // #3664: `g.constructor` for a generator/async-generator
                    // function resolves through its [[Prototype]] (`%Generator%`)
                    // to `%GeneratorFunction%` / `%AsyncGeneratorFunction%`.
                    // Other functions have no `constructor` own-prop in Perry's
                    // model (they fall through to `undefined`, as before).
                    if name_str == "constructor" {
                        if let Some(ctor) =
                            crate::object::generator_function_constructor_of(obj as usize)
                        {
                            return JSValue::from_bits(ctor.to_bits());
                        }
                    }
                    // #3664: `g.prototype` for a generator/async-generator
                    // function is a lazily-created object whose [[Prototype]] is
                    // `%Generator.prototype%`. Non-generator functions fall
                    // through (unchanged). The dynamic-prop check above already
                    // returned any cached/user-assigned `prototype`.
                    if name_str == "prototype" {
                        if let Some(proto) =
                            crate::object::generator_function_prototype_of(obj as usize)
                        {
                            return JSValue::from_bits(proto.to_bits());
                        }
                        let func_value = crate::value::js_nanbox_pointer(obj as i64);
                        if let Some(proto) =
                            super::ordinary_function_prototype_value_for_read(func_value)
                        {
                            return JSValue::from_bits(proto.to_bits());
                        }
                    }
                    // #2059: `fn.name` — every function carries a built-in own
                    // `name` data property. Resolve the codegen-registered name
                    // (keyed by the wrapper func_ptr, the same registry the
                    // `[Function: <name>]` formatter uses); anonymous functions
                    // read back `""`, matching Node, not `undefined`.
                    if name_str == "name" {
                        let func_ptr =
                            (*(obj as *const crate::closure::ClosureHeader)).func_ptr as usize;
                        let fname =
                            crate::builtins::function_name_for_ptr(func_ptr).unwrap_or_default();
                        let s =
                            crate::string::js_string_from_bytes(fname.as_ptr(), fname.len() as u32);
                        return JSValue::from_bits(crate::js_nanbox_string(s as i64).to_bits());
                    }
                    // #3716: reading `f.bind` / `f.call` / `f.apply` *as a value*
                    // off any function must yield a real callable, not
                    // `undefined`. Reify it into a BOUND_METHOD closure bound to
                    // this function as receiver; invoking it routes back through
                    // `js_native_call_method(f, "<method>", …)`. This is what makes
                    // the "uncurry-this" idiom
                    // `Function.prototype.call.bind(method)` work — reading `.bind`
                    // off the reified `Function.prototype.call` previously read
                    // back `undefined`, so the bound function was never produced.
                    let reified: Option<&'static [u8]> = match name_str {
                        "bind" => Some(b"bind"),
                        "call" => Some(b"call"),
                        "apply" => Some(b"apply"),
                        _ => None,
                    };
                    if let Some(method) = reified {
                        let receiver =
                            f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
                        return JSValue::from_bits(
                            crate::closure::reify_function_method_value(receiver, method).to_bits(),
                        );
                    }
                    return JSValue::from_bits(val.to_bits());
                }
            }
            return JSValue::undefined();
        }
        // Error objects: route the common instance properties (message,
        // name, stack, cause) through the dedicated error accessors.
        // `js_object_get_field_by_name_f64` is the codegen's default
        // property dispatch for caught exceptions, so this is the only
        // sensible place to wire Error access.
        if gc_type == crate::gc::GC_TYPE_ERROR {
            if !key.is_null() {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                let err_ptr = obj as *mut crate::error::ErrorHeader;
                // User-assigned own properties (`err.code = "X"`,
                // `err.errno = -2`, custom fields) take precedence over the
                // built-in accessors below — they were recorded in the
                // per-error side table by the setter (#2014).
                if let Ok(key_str) = std::str::from_utf8(key_bytes) {
                    if let Some(v) =
                        crate::node_submodules::error_user_prop(err_ptr as usize, key_str)
                    {
                        return JSValue::from_bits(v.to_bits());
                    }
                }
                match key_bytes {
                    b"message" => {
                        let s = crate::error::js_error_get_message(err_ptr);
                        return JSValue::from_bits(crate::js_nanbox_string(s as i64).to_bits());
                    }
                    b"name" => {
                        let s = crate::error::js_error_get_name(err_ptr);
                        return JSValue::from_bits(crate::js_nanbox_string(s as i64).to_bits());
                    }
                    b"stack" => {
                        let s = crate::error::js_error_get_stack(err_ptr);
                        return JSValue::from_bits(crate::js_nanbox_string(s as i64).to_bits());
                    }
                    b"cause" => {
                        let v = crate::error::js_error_get_cause(err_ptr);
                        return JSValue::from_bits(v.to_bits());
                    }
                    b"toString" => {
                        let this_f64 =
                            f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
                        let result = js_class_method_bind(this_f64, b"toString".as_ptr(), 8);
                        return JSValue::from_bits(result.to_bits());
                    }
                    b"constructor" => {
                        let name = crate::error::error_kind_constructor_name((*err_ptr).error_kind);
                        let name = name.as_bytes();
                        let v = js_get_global_this_builtin_value(name.as_ptr(), name.len());
                        return JSValue::from_bits(v.to_bits());
                    }
                    b"code" => {
                        // Errors thrown by runtime validation paths (e.g.
                        // diagnostics_channel argument checks) register
                        // their `ERR_*` code in a side table keyed on the
                        // message StringHeader pointer. This avoids the
                        // earlier substring-match shim that incorrectly
                        // applied `ERR_INVALID_ARG_TYPE` to any user
                        // TypeError whose `.message` happened to equal
                        // the placeholder text.
                        let msg = crate::error::js_error_get_message(err_ptr);
                        if let Some(code) = crate::node_submodules::error_code_for_message(msg) {
                            let s = crate::string::js_string_from_bytes(
                                code.as_ptr(),
                                code.len() as u32,
                            );
                            return JSValue::from_bits(crate::js_nanbox_string(s as i64).to_bits());
                        }
                        return JSValue::undefined();
                    }
                    b"errors" => {
                        // AggregateError.errors — return the errors array
                        // NaN-boxed with POINTER_TAG so callers can index
                        // into it. (The LLVM backend also has a direct
                        // `js_error_get_errors` fast path in expr.rs but
                        // this covers dynamic dispatch on caught errors.)
                        let errs = crate::error::js_error_get_errors(err_ptr);
                        if errs.is_null() {
                            return JSValue::undefined();
                        }
                        return JSValue::from_bits(crate::js_nanbox_pointer(errs as i64).to_bits());
                    }
                    b"syscall" => {
                        // Node attaches `syscall` to system-call errors
                        // (open/stat/access/…). Perry's fs helpers register
                        // the value in a side table keyed by the message
                        // StringHeader (parallel to the `.code` path).
                        let msg = crate::error::js_error_get_message(err_ptr);
                        if let Some(syscall) =
                            crate::node_submodules::error_syscall_for_message(msg)
                        {
                            let s = crate::string::js_string_from_bytes(
                                syscall.as_ptr(),
                                syscall.len() as u32,
                            );
                            return JSValue::from_bits(crate::js_nanbox_string(s as i64).to_bits());
                        }
                        return JSValue::undefined();
                    }
                    b"errno" => {
                        let msg = crate::error::js_error_get_message(err_ptr);
                        if let Some(errno) = crate::node_submodules::error_errno_for_message(msg) {
                            return JSValue::number(errno as f64);
                        }
                        return JSValue::undefined();
                    }
                    b"path" => {
                        let msg = crate::error::js_error_get_message(err_ptr);
                        if let Some(path) = crate::node_submodules::error_path_for_message(msg) {
                            let s = crate::string::js_string_from_bytes(
                                path.as_ptr(),
                                path.len() as u32,
                            );
                            return JSValue::from_bits(crate::js_nanbox_string(s as i64).to_bits());
                        }
                        return JSValue::undefined();
                    }
                    b"dest" => {
                        // Node attaches `dest` to two-path fs errors
                        // (rename/copyFile/link/symlink). Mirrors `.path`.
                        let msg = crate::error::js_error_get_message(err_ptr);
                        if let Some(dest) = crate::node_submodules::error_dest_for_message(msg) {
                            let s = crate::string::js_string_from_bytes(
                                dest.as_ptr(),
                                dest.len() as u32,
                            );
                            return JSValue::from_bits(crate::js_nanbox_string(s as i64).to_bits());
                        }
                        return JSValue::undefined();
                    }
                    _ => return JSValue::undefined(),
                }
            }
            return JSValue::undefined();
        }
        // Arrays: handle `.length` so dynamic property access on a
        // typed-Any local returned from `JSON.parse("[1,2,3]")` picks
        // up the real length instead of falling through to object
        // field lookup and returning undefined. The array-length
        // inline fast path in codegen fires only when the type is
        // statically known, so this branch catches the dynamic case.
        if gc_type == crate::gc::GC_TYPE_ARRAY {
            if !key.is_null() {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                let arr = obj as *const crate::array::ArrayHeader;
                if key_bytes == b"length" {
                    return JSValue::number(crate::array::js_array_length(arr) as f64);
                }
                // date-fns / drizzle / lodash duck-typing path:
                // `arr.constructor === Array`, `new arr.constructor(...)`,
                // etc. expect a non-undefined function-typed value that
                // refers back to the global `Array` constructor. Resolve
                // through the singleton so this returns the same closure
                // pointer as the bare `Array` identifier.
                if key_bytes == b"constructor" {
                    let v = js_get_global_this_builtin_value(b"Array".as_ptr(), 5);
                    return JSValue::from_bits(v.to_bits());
                }
                if let Ok(name) = std::str::from_utf8(key_bytes) {
                    if let Some(index) = super::canonical_array_index(name) {
                        return JSValue::from_bits(
                            crate::array::js_array_get_f64(arr, index).to_bits(),
                        );
                    }
                    if let Some(v) = own_data_field_by_name(obj, key) {
                        return v;
                    }
                    if let Some(v) = crate::array::array_named_property_get(arr, key) {
                        return JSValue::from_bits(v.to_bits());
                    }
                    if let Some(v) = array_prototype_property_value(name, obj as usize) {
                        return v;
                    }
                }
                if is_array_method_value_name(key_bytes) {
                    if let Ok(name) = std::str::from_utf8(key_bytes) {
                        if let Some(v) = array_prototype_property_value(name, obj as usize) {
                            return v;
                        }
                    }
                }
            }
            return JSValue::undefined();
        }
        // Issue #179 Phase 2: lazy array dispatch. `.length` returns
        // cached_length without materializing; any other property
        // access force-materializes (via the call into the generic
        // array path, which goes through `clean_arr_ptr` and hits
        // the lazy branch there).
        if gc_type == crate::gc::GC_TYPE_LAZY_ARRAY {
            if !key.is_null() {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                if key_bytes == b"length" {
                    let arr = obj as *const crate::array::ArrayHeader;
                    return JSValue::number(crate::array::js_array_length(arr) as f64);
                }
                if key_bytes == b"constructor" {
                    let v = js_get_global_this_builtin_value(b"Array".as_ptr(), 5);
                    return JSValue::from_bits(v.to_bits());
                }
            }
            // Any other property access force-materializes, then
            // re-enters via the materialized ArrayHeader pointer.
            let materialized = crate::json_tape::force_materialize_lazy(
                obj as *mut crate::json_tape::LazyArrayHeader,
            );
            return js_object_get_field_by_name(materialized as *const ObjectHeader, key);
        }
        // Strings: handle `.length` so `(x as string).length` on an
        // unknown-typed local (TypeScript `as` casts are erased in
        // HIR) produces the real UTF-16 code-unit length.
        if gc_type == crate::gc::GC_TYPE_STRING {
            if !key.is_null() {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                if key_bytes == b"length" {
                    let s = obj as *const crate::StringHeader;
                    return JSValue::number((*s).utf16_len as f64);
                }
                if let Some((kind, asym_type)) = crate::buffer::asymmetric_key_meta(obj as usize) {
                    if key_bytes == b"type" {
                        let label = if kind == 1 {
                            b"public".as_slice()
                        } else {
                            b"private".as_slice()
                        };
                        let s =
                            crate::string::js_string_from_bytes(label.as_ptr(), label.len() as u32);
                        return JSValue::from_bits(JSValue::string_ptr(s).bits());
                    }
                    if key_bytes == b"asymmetricKeyType" {
                        let label = match asym_type {
                            1 => b"rsa".as_slice(),
                            2 => b"ec".as_slice(),
                            3 => b"ed25519".as_slice(),
                            4 => b"x25519".as_slice(),
                            _ => b"".as_slice(),
                        };
                        if !label.is_empty() {
                            let s = crate::string::js_string_from_bytes(
                                label.as_ptr(),
                                label.len() as u32,
                            );
                            return JSValue::from_bits(JSValue::string_ptr(s).bits());
                        }
                    }
                    if key_bytes == b"asymmetricKeyDetails" {
                        let details = js_object_alloc(0, if asym_type == 2 { 1 } else { 0 });
                        if asym_type == 2 {
                            let name =
                                crate::string::js_string_from_bytes(b"namedCurve".as_ptr(), 10);
                            let val =
                                crate::string::js_string_from_bytes(b"prime256v1".as_ptr(), 10);
                            js_object_set_field_by_name(
                                details,
                                name,
                                f64::from_bits(JSValue::string_ptr(val).bits()),
                            );
                        }
                        return JSValue::from_bits(JSValue::pointer(details as *mut u8).bits());
                    }
                    // `js_class_method_bind` only needs a pointer that stays
                    // valid for the closure's lifetime — the static byte
                    // literals satisfy that without per-read allocation.
                    let static_name: Option<&'static [u8]> = match key_bytes {
                        b"export" => Some(b"export"),
                        b"equals" => Some(b"equals"),
                        _ => None,
                    };
                    if let Some(name) = static_name {
                        let this_f64 =
                            f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
                        let result = js_class_method_bind(this_f64, name.as_ptr(), name.len());
                        return JSValue::from_bits(result.to_bits());
                    }
                }
            }
            return JSValue::undefined();
        }
        // Maps: handle `.size` for `obj.m.size` style access where m is
        // a Map field stored in a plain object literal. Without this
        // the dynamic property dispatch returns undefined.
        if gc_type == crate::gc::GC_TYPE_MAP {
            if !key.is_null() {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                if key_bytes == b"size" {
                    let m = obj as *const crate::map::MapHeader;
                    return JSValue::number(crate::map::js_map_size(m) as f64);
                }
            }
            return JSValue::undefined();
        }
        // RegExp: RegExpHeader is allocated via GC_TYPE_OBJECT but tracked
        // in REGEX_POINTERS. Detect and route `.source`, `.flags`,
        // `.lastIndex`, `.global`, `.ignoreCase`, `.multiline`, `.sticky`,
        // `.unicode`, `.dotAll` to the regex header fields. Must run
        // before the generic object-field path so the keys_array lookup
        // doesn't try to read the regex header bytes as ObjectHeader.
        if gc_type == crate::gc::GC_TYPE_OBJECT && crate::regex::is_regex_pointer(obj as *const u8)
        {
            if !key.is_null() {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                let re = obj as *const crate::regex::RegExpHeader;
                match key_bytes {
                    b"source" => {
                        let s = crate::regex::js_regexp_get_source(re);
                        return JSValue::from_bits(crate::js_nanbox_string(s as i64).to_bits());
                    }
                    b"flags" => {
                        let s = crate::regex::js_regexp_get_flags(re);
                        return JSValue::from_bits(crate::js_nanbox_string(s as i64).to_bits());
                    }
                    b"lastIndex" => {
                        return JSValue::number((*re).last_index as f64);
                    }
                    b"global" => {
                        return JSValue::bool((*re).global);
                    }
                    b"ignoreCase" => {
                        return JSValue::bool((*re).case_insensitive);
                    }
                    b"multiline" => {
                        return JSValue::bool((*re).multiline);
                    }
                    // #2828: route the remaining observable flags to the
                    // header fields populated by `js_regexp_new` instead of
                    // unconditionally returning `false`.
                    b"sticky" => {
                        return JSValue::bool((*re).sticky);
                    }
                    b"unicode" => {
                        return JSValue::bool((*re).unicode);
                    }
                    b"dotAll" => {
                        return JSValue::bool((*re).dot_all);
                    }
                    b"hasIndices" => {
                        return JSValue::bool((*re).has_indices);
                    }
                    _ => return JSValue::undefined(),
                }
            }
            return JSValue::undefined();
        }
        if gc_type != crate::gc::GC_TYPE_OBJECT {
            let object_type = (*obj).object_type;
            if object_type != crate::error::OBJECT_TYPE_REGULAR {
                return JSValue::undefined();
            }
        }
        if super::is_arguments_object(obj) {
            if let Some(value) = super::arguments_object_get_field(obj, key) {
                return value;
            }
        }

        // #1387: `PerformanceEntry#toJSON` is a synthesized (non-enumerable)
        // method — entry objects are plain shaped objects with no stored
        // `toJSON` field, so a `entry.toJSON` read (e.g. `typeof entry.toJSON`)
        // would otherwise miss the keys_array and return undefined. Return a
        // bound-method closure; the call lands in `js_native_call_method`'s
        // toJSON arm via `dispatch_bound_method`. Gated on the key bytes first
        // so non-toJSON reads pay only a length+compare, not the identity
        // check.
        if !key.is_null() {
            let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let key_len = (*key).byte_len as usize;
            let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
            if key_bytes == b"toJSON" && crate::perf_hooks::is_perf_entry_object(obj) {
                let this_f64 =
                    f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
                let result = js_class_method_bind(this_f64, b"toJSON".as_ptr(), 6);
                return JSValue::from_bits(result.to_bits());
            }
        }

        // #2856: a property READ (not a call) of `next` on a Map/Set
        // iterator object must yield a callable (so `typeof it.next ===
        // "function"` and `const n = it.next; n()` work). The iterators
        // dispatch via class id and store no `next` field, so bind the
        // method to the receiver. Also bind the self-iterator methods.
        if !key.is_null()
            && ((*obj).class_id == crate::collection_iter_object::MAP_ITERATOR_CLASS_ID
                || (*obj).class_id == crate::collection_iter_object::SET_ITERATOR_CLASS_ID)
        {
            let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let key_len = (*key).byte_len as usize;
            let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
            let bind_name: Option<&'static [u8]> = match key_bytes {
                b"next" => Some(b"next"),
                b"return" => Some(b"return"),
                b"throw" => Some(b"throw"),
                b"@@iterator" => Some(b"@@iterator"),
                _ => None,
            };
            if let Some(name) = bind_name {
                let this_f64 =
                    f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
                let result = js_class_method_bind(this_f64, name.as_ptr(), name.len());
                return JSValue::from_bits(result.to_bits());
            }
            return JSValue::undefined();
        }

        // Issue #649: native-module sub-namespace property access.
        // `fs.constants.F_OK` lowers to `PropertyGet { PropertyGet { fs,
        // "constants" }, "F_OK" }` — the inner expression's runtime value
        // is a NATIVE_MODULE_CLASS_ID-tagged ObjectHeader produced by
        // `js_create_native_module_namespace`; the outer PropertyGet then
        // arrives here with the sub-namespace as receiver. Pre-fix the
        // lookup fell through to the field-bag scan (which only stores
        // `__module__`) and returned undefined. Now we route through
        // `get_native_module_constant` directly.
        if (*obj).class_id == NATIVE_MODULE_CLASS_ID && !key.is_null() {
            let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let key_len = (*key).byte_len as usize;
            let nb_ptr = crate::value::js_nanbox_pointer(obj as i64);
            let module_name = get_module_name_from_namespace(nb_ptr);
            if !module_name.is_empty() {
                let property_name =
                    std::str::from_utf8(std::slice::from_raw_parts(key_ptr, key_len)).unwrap_or("");
                if matches!(
                    module_name,
                    "process" | "process.namespace" | "process.default"
                ) {
                    if let Some(value) = crate::process::process_ipc_property(property_name) {
                        return JSValue::from_bits(value.to_bits());
                    }
                }
                if let Some(value) = native_module_own_field_by_key(obj, key) {
                    return value;
                }
                // #3687: node:cluster default-import EventEmitter methods on the
                // distinct `cluster.default` namespace. Mirror the
                // NativeModuleRef fast path (`js_native_module_property_by_name`)
                // — this dynamic `obj[key]` read must resolve `on`/`emit`/… to
                // bound methods *before* `get_native_module_constant` (which
                // normalizes to `cluster` and returns `undefined` for `on`).
                if module_name == "cluster.default"
                    && super::is_cluster_emitter_method(property_name)
                {
                    return JSValue::from_bits(
                        super::bound_native_callable_export_value(module_name, property_name)
                            .to_bits(),
                    );
                }
                if let Some(val) = get_native_module_constant(module_name, property_name, nb_ptr) {
                    return JSValue::from_bits(val.to_bits());
                }
                if module_name == "crypto.webcrypto" {
                    if let Some(value) = super::global_this::webcrypto_method_value(property_name) {
                        return JSValue::from_bits(value.to_bits());
                    }
                }
                // Issue #894: parity with the direct-NativeModuleRef
                // fast path (`js_native_module_property_by_name`). For
                // (module, prop) pairs whose property-read should
                // produce a callable handle — e.g.
                // `("events", "EventEmitter")` — synthesize the same
                // BOUND_METHOD_FUNC_PTR closure so the require-then-
                // member-access shape (`const { EventEmitter } =
                // require("node:events")`) matches the direct
                // namespace-import shape (`import { EventEmitter } from
                // "node:events"`). Pre-fix the slow path returned
                // undefined here, and the downstream
                // `EventEmitter.prototype` read tripped the spec
                // "Cannot read properties of undefined" throw.
                if is_native_module_callable_export(module_name, property_name) {
                    return JSValue::from_bits(
                        super::bound_native_callable_export_value(module_name, property_name)
                            .to_bits(),
                    );
                }
                return JSValue::undefined();
            }
        }

        if (*obj).class_id == crate::tty::CLASS_ID_TTY_WRITE_STREAM && !key.is_null() {
            let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let key_len = (*key).byte_len as usize;
            let property_name =
                std::str::from_utf8(std::slice::from_raw_parts(key_ptr, key_len)).unwrap_or("");
            if let Some(value) = crate::tty::tty_write_stream_dimension(property_name) {
                return JSValue::from_bits(value.to_bits());
            }
        }

        // Refs #420 / #618 followup: `instance.constructor` returns the
        // class ref. Pre-fix this fell through to the keys_array lookup
        // which never finds "constructor" (the class itself isn't stored
        // as a field on the instance), and the chain returned undefined.
        // Drizzle's `is(value, type)` walks `value.constructor[entityKind]`
        // which depends on this. Spec: every instance's `__proto__.constructor`
        // points back to the class function. We materialize that lookup
        // by reading the ObjectHeader's class_id and returning the
        // INT32-tagged class ref if registered. Unregistered class_id
        // (e.g. `class C {}` with no methods) still returns undefined
        // here; pure object literals have class_id=0 and also return
        // undefined (matches Node behavior — bare object literals don't
        // get a custom constructor; their .constructor would be Object).
        if !key.is_null() {
            let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let key_len = (*key).byte_len as usize;
            let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
            if (*obj).class_id == CLASS_ID_BOXED_STRING {
                if let Some((_, payload)) = crate::builtins::boxed_primitive_payload(
                    f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits()),
                ) {
                    if let Some(value) = string_index_value(payload, key) {
                        return value;
                    }
                }
            }
            if key_bytes == b"constructor" {
                if let Some(v) = own_data_field_by_name(obj, key) {
                    return v;
                }
                let class_id = (*obj).class_id;
                if class_id != 0 && class_has_own_method(class_id, "constructor") {
                    let value = class_prototype_method_value_for_name(class_id, "constructor");
                    return JSValue::from_bits(value.to_bits());
                }
                if matches!(
                    class_id,
                    CLASS_ID_BOXED_NUMBER
                        | CLASS_ID_BOXED_STRING
                        | CLASS_ID_BOXED_BOOLEAN
                        | CLASS_ID_BOXED_BIGINT
                        | CLASS_ID_BOXED_SYMBOL
                ) {
                    let name = match class_id {
                        CLASS_ID_BOXED_NUMBER => b"Number".as_slice(),
                        CLASS_ID_BOXED_STRING => b"String".as_slice(),
                        CLASS_ID_BOXED_BOOLEAN => b"Boolean".as_slice(),
                        CLASS_ID_BOXED_BIGINT => b"BigInt".as_slice(),
                        CLASS_ID_BOXED_SYMBOL => b"Symbol".as_slice(),
                        _ => unreachable!(),
                    };
                    let v = js_get_global_this_builtin_value(name.as_ptr(), name.len());
                    return JSValue::from_bits(v.to_bits());
                }
                // Object-literal instances (`{ x: 1 }`) carry a synthetic
                // `__AnonShape_*` class id. Spec says their `.constructor`
                // is the global `Object`, not the synthetic class — so
                // resolve through the globalThis singleton so the value
                // matches the bare `Object` identifier (`x.constructor
                // === Object`, date-fns `constructFrom`, drizzle's
                // `isPlainObject` duck check).
                if class_id != 0 && is_anon_shape_class_id(class_id) {
                    let v = js_get_global_this_builtin_value(b"Object".as_ptr(), 6);
                    return JSValue::from_bits(v.to_bits());
                }
                if let Some(func_value) =
                    super::class_registry::function_value_for_class_id(class_id)
                {
                    return JSValue::from_bits(func_value.to_bits());
                }
                if class_id != 0 && is_class_id_registered(class_id) {
                    let bits = 0x7FFE_0000_0000_0000u64 | (class_id as u64);
                    return JSValue::from_bits(bits);
                }
                // class_id == 0 fallback: plain ObjectHeader allocated
                // without an HIR shape (Object.create(null) hybrids, raw
                // empty `{}` produced by JSON.parse, etc.). Report
                // `Object` so duck-type tests don't trip undefined.
                if class_id == 0 {
                    let v = js_get_global_this_builtin_value(b"Object".as_ptr(), 6);
                    return JSValue::from_bits(v.to_bits());
                }
            }
        }

        let keys = (*obj).keys_array;

        if keys.is_null() {
            // #809: an object with no own keys (e.g. an `Object.create(proto)`
            // result, or a `Function.prototype = obj` instance) still has to
            // resolve inherited props/methods. Pre-fix this returned undefined
            // here — BEFORE the `class_id` prototype-walk below — so
            // `Object.create(P).m()` threw `TypeError: m is not a function`.
            let class_id = (*obj).class_id;
            if class_id != 0 {
                let receiver =
                    f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
                if let Some(v) = super::class_registry::resolve_proto_chain_field_with_receiver(
                    class_id, key, receiver,
                ) {
                    return v;
                }
                let key_bytes = std::slice::from_raw_parts(
                    (key as *const u8).add(std::mem::size_of::<crate::StringHeader>()),
                    (*key).byte_len as usize,
                );
                // Issue #838 followup (b): same keyless-receiver gap for
                // JS-classic prototype methods. An instance allocated via
                // `js_new_function_construct` (no constructor-body write
                // yet, or a constructor that runs the closures' own
                // capture writes but never `this.<own field> = …`)
                // starts with `keys_array == null`. Without this arm
                // dayjs's `(new _(cfg)).format` returned undefined
                // because the keyless branch skipped the regular
                // `CLASS_PROTOTYPE_METHODS` walk reached further down
                // — see the matching arm at line ~4083.
                if let Ok(name) = std::str::from_utf8(key_bytes) {
                    if let Some(v) = lookup_prototype_method(class_id, name) {
                        return JSValue::from_bits(v.to_bits());
                    }
                    // Native class vtable accessors and methods are exposed
                    // from the class, not from own fields, so keyless
                    // receivers need the same fallback as shaped receivers.
                    if let Ok(registry) = CLASS_VTABLE_REGISTRY.read() {
                        if let Some(ref reg) = *registry {
                            let mut cid = class_id;
                            let mut depth = 0usize;
                            while depth < 32 {
                                if let Some(vtable) = reg.get(&cid) {
                                    if let Some(&getter_ptr) = vtable.getters.get(name) {
                                        let this_f64 = f64::from_bits(
                                            crate::value::js_nanbox_pointer(obj as i64).to_bits(),
                                        );
                                        let f: extern "C" fn(f64) -> f64 =
                                            std::mem::transmute(getter_ptr);
                                        return JSValue::from_bits(f(this_f64).to_bits());
                                    }
                                }
                                match get_parent_class_id(cid) {
                                    Some(p) if p != 0 && p != cid => {
                                        cid = p;
                                        depth += 1;
                                    }
                                    _ => break,
                                }
                            }
                        }
                    }
                    if lookup_class_method_in_chain(class_id, name).is_some() {
                        let heap_name = {
                            let layout =
                                std::alloc::Layout::from_size_align(key_bytes.len().max(1), 1)
                                    .unwrap();
                            let ptr = std::alloc::alloc(layout);
                            std::ptr::copy_nonoverlapping(key_bytes.as_ptr(), ptr, key_bytes.len());
                            ptr
                        };
                        let this_f64 =
                            f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
                        let result = js_class_method_bind(this_f64, heap_name, key_bytes.len());
                        return JSValue::from_bits(result.to_bits());
                    }
                }
            }
            if class_id == crate::builtins::CONSOLE_INSTANCE_CLASS_ID {
                let key_bytes = std::slice::from_raw_parts(
                    (key as *const u8).add(std::mem::size_of::<crate::StringHeader>()),
                    (*key).byte_len as usize,
                );
                if let Ok(name) = std::str::from_utf8(key_bytes) {
                    if crate::builtins::is_console_instance_method_name(name) {
                        let heap_name = {
                            let layout =
                                std::alloc::Layout::from_size_align(key_bytes.len().max(1), 1)
                                    .unwrap();
                            let ptr = std::alloc::alloc(layout);
                            std::ptr::copy_nonoverlapping(key_bytes.as_ptr(), ptr, key_bytes.len());
                            ptr
                        };
                        let this_f64 =
                            f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
                        let result = js_class_method_bind(this_f64, heap_name, key_bytes.len());
                        return JSValue::from_bits(result.to_bits());
                    }
                }
            }
            // #2820: a keyless object (`{}`, `Object.create(...)`) may still
            // carry an explicit `Object.setPrototypeOf` prototype — walk it so
            // inherited reads resolve.
            if !key.is_null() {
                if let Some(v) = super::prototype_chain::resolve_inherited_field(obj as usize, key)
                {
                    return v;
                }
                if let Some(v) = ordinary_object_prototype_method_value(obj, key) {
                    return v;
                }
            }
            return JSValue::undefined();
        }

        // Validate keys_array is a real heap pointer (upper 16 bits must be 0 for ARM64/x86-64 user space).
        // If the object is actually a non-Object type (closure, array, map, etc.), keys_array at offset
        // 16 may contain garbage. An invalid upper 16-bit value catches this case defensively.
        let keys_ptr = keys as usize;
        if (keys_ptr as u64) >> 48 != 0 || keys_ptr < 0x10000 {
            // #2820: an object with no own keys (`{}`) may still have an
            // explicit `Object.setPrototypeOf` prototype — walk it before
            // giving up so inherited reads resolve.
            if !key.is_null() {
                if let Some(v) = super::prototype_chain::resolve_inherited_field(obj as usize, key)
                {
                    return v;
                }
                if let Some(v) = ordinary_object_prototype_method_value(obj, key) {
                    return v;
                }
            }
            return JSValue::undefined();
        }

        // Issue #62 phase B: the previous "ASCII-like pointer value" heuristic
        // assumed macOS mmap always returns arena pointers with `top_byte < 0x20`.
        // That stopped holding once strings started arena-allocating (more blocks,
        // mimalloc mapping into higher ranges): valid 0x000_04355_a033_* pointers
        // triggered false positives, the heuristic returned `undefined`, and tests
        // like `Object.defineProperty` flapped. The GcHeader `obj_type ==
        // GC_TYPE_ARRAY` check immediately below is a real content-level validation
        // (can't be faked by an address in any range) and fully supersedes this
        // address-sniffing heuristic.

        // Cross-platform safety: validate keys_array has a valid GcHeader.
        // If the keys_array pointer is corrupt (e.g., due to a stale reference after GC,
        // or a func_addr relocation issue on x86_64), the GcHeader check catches it
        // before we dereference the array contents.
        {
            let keys_gc =
                (keys as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            let keys_gc_type = (*keys_gc).obj_type;
            // keys_array must be GC_TYPE_ARRAY (arena-allocated array)
            if keys_gc_type != crate::gc::GC_TYPE_ARRAY {
                return JSValue::undefined();
            }
        }

        // Fast path: check field index cache (keys_array_ptr + key_hash → field_index)
        // Objects with the same shape share the same keys_array, so we cache per-shape lookups.
        let key_bytes = std::slice::from_raw_parts(
            (key as *const u8).add(std::mem::size_of::<crate::StringHeader>()),
            (*key).byte_len as usize,
        );
        // #4140: builtin reflection-only accessors (e.g. the
        // `%TypedArray%.prototype` getters) don't flip `ACCESSORS_IN_USE`, so the
        // gated short-circuits below skip them on a plain value read. Handle the
        // hosting prototype object here — a cheap pointer compare for everything
        // else — before the slot scan returns the empty backing field.
        if let Some(v) = builtin_reflection_accessor_read(obj, key_bytes) {
            return v;
        }
        let key_hash = {
            let mut h: u32 = 0x811c9dc5;
            for &b in key_bytes {
                h ^= b as u32;
                h = h.wrapping_mul(0x01000193);
            }
            h
        };
        let keys_id = keys as usize;

        let key_count = crate::array::js_array_length(keys) as usize;

        // Thread-local inline cache: fixed-size direct-mapped cache (no allocation, no HashMap)
        // Each entry stores (keys_ptr, key_hash, field_index). Copied-minor
        // nursery reset can reuse a keys-array address, so cache hits still
        // validate the key slot before returning a field.
        const FIELD_CACHE_SIZE: usize = 1024;
        thread_local! {
            static FIELD_CACHE: std::cell::UnsafeCell<[(usize, u32, u32); FIELD_CACHE_SIZE]> =
                const { std::cell::UnsafeCell::new([(0usize, 0u32, 0u32); FIELD_CACHE_SIZE]) };
        }
        let cache_idx = (keys_id.wrapping_add(key_hash as usize)) % FIELD_CACHE_SIZE;
        let cached = FIELD_CACHE.with(|c| {
            let cache = &*c.get();
            let entry = cache[cache_idx];
            if entry.0 == keys_id && entry.1 == key_hash {
                Some(entry.2)
            } else {
                None
            }
        });
        if let Some(field_idx) = cached {
            let idx = field_idx as usize;
            let cache_hit_valid = if idx < key_count {
                let key_val = crate::array::js_array_get(keys, field_idx);
                // #1781: SSO-aware match — pre-fix the `is_string()` here
                // false-invalidated cache hits for ≤5-byte keys stored
                // as SHORT_STRING_TAG values.
                crate::string::js_string_key_matches(key_val, key)
            } else {
                false
            };
            if !cache_hit_valid {
                FIELD_CACHE.with(|c| {
                    let cache = &mut *c.get();
                    cache[cache_idx] = (0, 0, 0);
                });
            } else {
                // Accessor short-circuit: if this (obj, key) has a getter installed,
                // invoke it instead of reading the slot. The `ACCESSORS_IN_USE`
                // thread-local gate keeps this off the hot path in the common case.
                if ACCESSORS_IN_USE.with(|c| c.get()) {
                    if let Ok(name) = std::str::from_utf8(key_bytes) {
                        if let Some(acc) = get_accessor_descriptor(obj as usize, name) {
                            if acc.get != 0 {
                                let receiver = crate::value::js_nanbox_pointer(obj as i64);
                                return invoke_accessor_getter(acc.get, receiver);
                            }
                            // Has accessor but no getter → undefined.
                            return JSValue::undefined();
                        }
                    }
                }
                return js_object_get_field(obj, field_idx);
            }
        }

        // Slow path: linear scan through keys array
        let _field_count = (*obj).field_count as usize;

        if key_count > 65536 {
            return JSValue::undefined();
        }

        let alloc_limit = std::cmp::max((*obj).field_count, 8) as usize;

        for i in 0..key_count {
            let key_val = crate::array::js_array_get(keys, i as u32);
            // #1781: accept inline SSO short keys here too — the
            // slow-path lookup is what backs `obj[k]` for ≤5-byte
            // keys after a field-cache miss.
            if crate::string::js_string_key_matches(key_val, key) {
                // Cache this lookup for next time
                FIELD_CACHE.with(|c| {
                    let cache = &mut *c.get();
                    cache[cache_idx] = (keys_id, key_hash, i as u32);
                });
                // Accessor short-circuit (see fast path above).
                if ACCESSORS_IN_USE.with(|c| c.get()) {
                    if let Ok(name) = std::str::from_utf8(key_bytes) {
                        if let Some(acc) = get_accessor_descriptor(obj as usize, name) {
                            if acc.get != 0 {
                                let receiver = crate::value::js_nanbox_pointer(obj as i64);
                                return invoke_accessor_getter(acc.get, receiver);
                            }
                            return JSValue::undefined();
                        }
                    }
                }
                if i < alloc_limit {
                    return js_object_get_field(obj, i as u32);
                } else {
                    return match overflow_get(obj as usize, i) {
                        Some(bits) => JSValue::from_bits(bits),
                        None => JSValue::undefined(),
                    };
                }
            }
        }

        // Key not found in the keys_array — fall back to the class
        // vtable's getter map. Refs #486 (hono): cross-module class
        // getters (e.g. hono Context's `get req()` defined in
        // `hono/dist/context.js` and read from a user `c.req.url`
        // expression in main.ts) reach this point because the field
        // dispatcher only looks for stored fields, not getter accessors.
        // The getter is registered in `CLASS_VTABLE_REGISTRY` via
        // `js_register_class_getter` at module init by codegen — invoke
        // it with the same NaN-boxed `this` the codegen passes for
        // method dispatch.
        let class_id = (*obj).class_id;
        if class_id != 0 {
            if let Ok(registry) = CLASS_VTABLE_REGISTRY.read() {
                if let Some(ref reg) = *registry {
                    // Walk the class -> parent chain so a getter declared
                    // on a base class is also found when the receiver is
                    // a subclass instance. `get_parent_class_id` reads
                    // CLASS_REGISTRY (populated by `js_register_class_parent`).
                    let mut cid = class_id;
                    let mut depth = 0usize;
                    while depth < 32 {
                        if let Some(vtable) = reg.get(&cid) {
                            if let Ok(name) = std::str::from_utf8(key_bytes) {
                                if let Some(&getter_ptr) = vtable.getters.get(name) {
                                    // Getters take `this` as f64 (NaN-boxed
                                    // POINTER_TAG), matching the codegen
                                    // calling convention for class methods.
                                    let this_f64: f64 = f64::from_bits(
                                        crate::value::js_nanbox_pointer(obj as i64).to_bits(),
                                    );
                                    let f: extern "C" fn(f64) -> f64 =
                                        std::mem::transmute(getter_ptr);
                                    return JSValue::from_bits(f(this_f64).to_bits());
                                }
                            }
                        }
                        match get_parent_class_id(cid) {
                            Some(p) if p != 0 && p != cid => {
                                cid = p;
                                depth += 1;
                            }
                            _ => break,
                        }
                    }
                }
            }

            // Issue #711 part 2: walk the class chain for a registered
            // prototype object (from `Function.prototype = X`). When
            // found, the method is an own-property of the proto
            // object — return its value directly. `pipe`, `[Equal.symbol]`,
            // etc. on Effect's EffectPrototype reach here.
            {
                let receiver =
                    f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
                if let Some(v) = resolve_proto_chain_field_with_receiver(class_id, key, receiver) {
                    return v;
                }
            }

            // Issue #838: JS-classic `Class.prototype.method = fn`
            // assignment registered via `js_register_prototype_method`.
            // Read returns the stored closure value directly, mirroring
            // Node's `Object.getPrototypeOf(inst).method` lookup. The
            // bound-method-closure fallback below handles vtable methods;
            // this arm covers methods that only exist as prototype
            // assignments (never declared inside the `class` block).
            if let Ok(name) = std::str::from_utf8(key_bytes) {
                if let Some(v) = lookup_prototype_method(class_id, name) {
                    return JSValue::from_bits(v.to_bits());
                }
                if class_id == crate::builtins::CONSOLE_INSTANCE_CLASS_ID
                    && crate::builtins::is_console_instance_method_name(name)
                {
                    let heap_name = {
                        let layout =
                            std::alloc::Layout::from_size_align(key_bytes.len().max(1), 1).unwrap();
                        let ptr = std::alloc::alloc(layout);
                        std::ptr::copy_nonoverlapping(key_bytes.as_ptr(), ptr, key_bytes.len());
                        ptr
                    };
                    let this_f64 =
                        f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
                    let result = js_class_method_bind(this_f64, heap_name, key_bytes.len());
                    return JSValue::from_bits(result.to_bits());
                }
            }

            // v0.5.756: method-as-value fallback. If `obj.method` reads via
            // the runtime path (Any-typed receiver, so the codegen #446
            // arm at expr.rs:3596 didn't fire), look up the method in the
            // class vtable chain and return a bound-method closure
            // (BOUND_METHOD_FUNC_PTR sentinel + (this, name_ptr, name_len)
            // captures). This makes both `typeof obj.method === "function"`
            // and `obj.method(args)` work for class methods on Any-typed
            // receivers — the closure-call dispatch routes through
            // `js_native_call_method` which walks the same vtable chain.
            // Refs #446 / drizzle's `(ins as any)._prepare()` chain.
            if let Ok(name) = std::str::from_utf8(key_bytes) {
                if lookup_class_method_in_chain(class_id, name).is_some() {
                    // Allocate a fresh i8 buffer for the method name owned
                    // by the closure. The keys_array's StringHeader bytes
                    // could in theory be GC'd if the keys_array is not
                    // pinned for the closure's lifetime.
                    let heap_name = {
                        let layout =
                            std::alloc::Layout::from_size_align(key_bytes.len().max(1), 1).unwrap();
                        let ptr = std::alloc::alloc(layout);
                        std::ptr::copy_nonoverlapping(key_bytes.as_ptr(), ptr, key_bytes.len());
                        ptr
                    };
                    let this_f64 =
                        f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
                    let result = js_class_method_bind(this_f64, heap_name, key_bytes.len());
                    return JSValue::from_bits(result.to_bits());
                }
            }
        }

        // #2820: before giving up, walk an explicit `Object.setPrototypeOf`
        // prototype chain recorded for this object so inherited property reads
        // (`obj.x` where `x` is an own property of the set prototype) resolve.
        if !key.is_null() {
            if let Some(v) = super::prototype_chain::resolve_inherited_field(obj as usize, key) {
                return v;
            }
            if let Some(v) = ordinary_object_prototype_method_value(obj, key) {
                return v;
            }
        }

        // Key not found
        JSValue::undefined()
    }
}

/// Get a field by its string key name, returned as f64 (raw JSValue bits)
/// This preserves the NaN-boxing for strings and other pointer types
#[no_mangle]
pub extern "C" fn js_object_get_field_by_name_f64(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> f64 {
    if (obj as usize) > 0 && (obj as usize) < 0x10000 && !key.is_null() {
        if let Some(name) = unsafe { super::has_own_helpers::str_from_string_header(key) } {
            let class_id = obj as usize as u32;
            if name == "name" && !super::class_registry::class_is_key_deleted(class_id, name) {
                if let Some(cname) = super::class_registry::class_name_for_id(class_id) {
                    let s = crate::string::js_string_from_bytes(cname.as_ptr(), cname.len() as u32);
                    return crate::js_nanbox_string(s as i64);
                }
            }
        }
    }
    // date-fns `constructFrom`: `new date.constructor(value)`. A Date is a
    // NaN-boxed `DateCell` pointer (#2089); `js_object_get_field_by_name`
    // routes `.constructor` to the global Date constructor closure and every
    // other key to `undefined` without derefing the small cell as an object.
    let value = js_object_get_field_by_name(obj, key);
    f64::from_bits(value.bits())
}

/// #2058: the universal `Object.prototype` methods inherited by every value,
/// including primitive numbers. Read as a property *value* (e.g.
/// `const f = n.toString`, `typeof n.isPrototypeOf`), these resolve to real
/// callable functions in Node — Perry binds them lazily via
/// `js_class_method_bind` so the value is both `typeof "function"` and
/// dispatchable through `js_native_call_method` (every name here has a
/// corresponding dispatch arm). `constructor` is excluded: it is a property
/// holding the `Number` function, not a bound method.
fn is_primitive_proto_method(key: &[u8]) -> bool {
    matches!(
        key,
        b"toString"
            | b"valueOf"
            | b"hasOwnProperty"
            | b"isPrototypeOf"
            | b"propertyIsEnumerable"
            | b"toLocaleString"
    )
}

fn is_array_method_value_name(key: &[u8]) -> bool {
    matches!(
        key,
        b"pop" | b"push" | b"shift" | b"unshift" | b"splice" | b"slice"
    )
}

fn set_method_value_name(key: &[u8]) -> Option<&'static [u8]> {
    match key {
        b"add" => Some(b"add"),
        b"clear" => Some(b"clear"),
        b"delete" => Some(b"delete"),
        b"entries" => Some(b"entries"),
        b"forEach" => Some(b"forEach"),
        b"has" => Some(b"has"),
        b"keys" => Some(b"keys"),
        b"values" => Some(b"values"),
        b"union" => Some(b"union"),
        b"intersection" => Some(b"intersection"),
        b"difference" => Some(b"difference"),
        b"symmetricDifference" => Some(b"symmetricDifference"),
        b"isSubsetOf" => Some(b"isSubsetOf"),
        b"isSupersetOf" => Some(b"isSupersetOf"),
        b"isDisjointFrom" => Some(b"isDisjointFrom"),
        b"@@iterator" => Some(b"@@iterator"),
        _ => None,
    }
}

fn is_timer_handle_method_key(key: &[u8]) -> bool {
    matches!(
        key,
        b"ref"
            | b"unref"
            | b"hasRef"
            | b"refresh"
            | b"close"
            | b"__perry_dispose__"
            // `using t = setTimeout(...)` / `t[Symbol.dispose]` — the
            // well-known dispose symbol lowers to this key. (#1213)
            | b"@@__perry_wk_dispose"
            | b"@@__perry_wk_toPrimitive"
    )
}

/// Monomorphic inline cache miss handler (issue #51).
///
/// Called when the codegen-emitted shape check (`obj->keys_array == cache[0]`)
/// fails. Performs the full field lookup via `js_object_get_field_by_name`,
/// then populates the per-site cache so subsequent calls with the same shape
/// hit the inline fast path (no function call, direct field load).
///
/// `cache` layout: `[keys_array_ptr: i64, field_slot_index: i64]`
///
/// Only caches when:
/// - obj is a valid ObjectHeader (not null, not handle, not string/array/etc.)
/// - field exists and its slot index < 8 (inline allocation limit)
///
/// Overflow fields (slot >= alloc_limit) are NOT cached and fall through to
/// the slow path — the fast path loads from `obj_ptr + 24 + slot*8` which
/// would read past the inline allocation.
#[no_mangle]
pub extern "C" fn js_object_get_field_ic_miss(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
    cache: *mut [i64; 2],
) -> f64 {
    // SSO receiver — never cacheable. Route through the SSO-aware
    // `js_object_get_field_by_name` which handles `.length` inline
    // and returns undefined for other keys.
    if !key.is_null() {
        let obj_bits = obj as u64;
        if (obj_bits & crate::value::TAG_MASK) == crate::value::SHORT_STRING_TAG {
            let v = js_object_get_field_by_name(obj, key);
            return f64::from_bits(v.bits());
        }
    }
    if obj.is_null() || key.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    unsafe {
        if let Some(val) = closure_dynamic_prop_by_key(obj as usize, key) {
            return val;
        }
        // Buffers have no GcHeader. The generic IC-miss object path below may
        // inspect GC/object metadata, so mirror js_object_get_field_by_name's
        // buffer-first dispatch here.
        if crate::buffer::is_registered_buffer(obj as usize) {
            let value = js_object_get_field_by_name(obj, key);
            return f64::from_bits(value.bits());
        }
    }
    // Issue #340: small-handle receivers (axios, fastify, ioredis,
    // ...) are passed here from the codegen IC miss path with the
    // lower-48 of the NaN-box stripped — `obj as usize` is the
    // raw handle id (1, 2, 3, ...). Route to HANDLE_PROPERTY_DISPATCH
    // (registered by stdlib via js_register_handle_property_dispatch)
    // so `r.status` / `r.data` and similar handle-property accesses
    // dispatch to the per-module accessor instead of silently
    // returning undefined.
    if (obj as usize) > 0 && (obj as usize) < 0x100000 {
        // #2846: a revocable Proxy is encoded as a small fake pointer in the
        // proxy-id range (also `< 0x100000`). A generic `proxy.key` read funnels
        // here via the IC-miss path; route it to the proxy get dispatch (which
        // forwards to the target, or throws on a revoked proxy) before the
        // handle-dispatch fallback. `js_proxy_is_proxy` validates the value is a
        // registered proxy so real small handles aren't misrouted.
        {
            const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
            let boxed = f64::from_bits(POINTER_TAG | ((obj as u64) & 0x0000_FFFF_FFFF_FFFF));
            if crate::proxy::js_proxy_is_proxy(boxed) != 0 {
                let key_f64 = f64::from_bits(crate::value::js_nanbox_string(key as i64).to_bits());
                return crate::proxy::js_proxy_get(boxed, key_f64);
            }
        }
        // #1213: Timeout/Immediate handle methods (ref/unref/hasRef/refresh/
        // close) read as bound-method function values so `typeof t.ref ===
        // "function"` holds (the call form already works via
        // js_native_call_method). The IC fast path funnels small handles here,
        // bypassing the identical block in `js_object_get_field_by_name`, so it
        // must be mirrored.
        unsafe {
            let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let key_len = (*key).byte_len as usize;
            let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
            if is_timer_handle_method_key(key_bytes) && crate::timer::is_known_timer_id(obj as i64)
            {
                let this_f64 =
                    f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
                return super::js_class_method_bind(this_f64, key_ptr, key_len);
            }
        }
        // Drizzle-sqlite blocker: synth `data.constructor` for small-handle
        // receivers — IC-miss path mirror of the constructor intercept in
        // `js_object_get_field_by_name`. Refs #645 deeper followup.
        unsafe {
            let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let key_len = (*key).byte_len as usize;
            let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
            if key_bytes == b"constructor" {
                if let Some(dispatch) = handle_property_dispatch() {
                    let bits = dispatch(obj as i64, key_ptr, key_len);
                    if bits.to_bits() != crate::value::TAG_UNDEFINED {
                        return bits;
                    }
                }
                let null_obj_ptr = &NULL_OBJECT_BYTES as *const NullObjectBytes as *mut u8;
                return f64::from_bits(JSValue::pointer(null_obj_ptr).bits());
            }
        }
        if let Some(dispatch) = handle_property_dispatch() {
            unsafe {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                return dispatch(obj as i64, key_ptr, key_len);
            }
        }
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if (obj as usize) < 0x10000 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    // When accessors are active anywhere in the program, skip the cache
    // entirely: the PIC fast path does a direct field load that bypasses
    // getter dispatch, so any object that uses defineProperty / get / set
    // would silently return the raw slot value instead of calling the
    // getter. The slow path through js_object_get_field_by_name handles
    // accessors correctly.
    let can_cache = !ACCESSORS_IN_USE.with(|c| c.get());
    unsafe {
        // Issue #72: validate this really is a GC_TYPE_OBJECT before reading
        // (*obj).keys_array — otherwise an Array/String/Buffer/etc. receiver
        // (whose `object_type` byte at offset 0 happens to be 1, matching
        // OBJECT_TYPE_REGULAR for a length-1 array) would be treated as
        // cacheable and seed the per-site PIC with garbage from element[1].
        // The codegen guard funnels non-OBJECT receivers here too, so this
        // belt-and-braces check keeps the cache from being primed with
        // values that would survive into the inline hot path.
        let is_object = (obj as usize) >= crate::gc::GC_HEADER_SIZE + 0x1000
            && is_valid_obj_ptr(obj as *const u8)
            && {
                let gc_header =
                    (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
                (*gc_header).obj_type == crate::gc::GC_TYPE_OBJECT
            };
        let is_regular = is_object && (*obj).object_type == crate::error::OBJECT_TYPE_REGULAR;
        if can_cache && is_regular {
            let keys = (*obj).keys_array;
            if keys.is_null() || (keys as usize) <= 0x10000 {
                let value = js_object_get_field_by_name(obj, key);
                return f64::from_bits(value.bits());
            }
            let key_count = *(keys as *const u32) as usize;
            let keys_data = (keys as *const u8).add(8) as *const f64;
            let alloc_limit = std::cmp::max((*obj).field_count, 8) as usize;
            for i in 0..key_count {
                let k_bits = (*keys_data.add(i)).to_bits();
                let k_ptr = (k_bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::StringHeader;
                if !k_ptr.is_null() && crate::string::js_string_equals(k_ptr, key) != 0 {
                    if i >= alloc_limit {
                        // Field is in the overflow map — fall through to the
                        // slow path which handles overflow correctly.
                        break;
                    }
                    // The codegen IC fast path computes `obj + 24 + slot*8`
                    // and does a direct load. Any inline slot (`i <
                    // alloc_limit`) is reachable via that path, so cache
                    // every inline slot — including the ones at index >= 8
                    // for classes whose `field_count` exceeds the
                    // MIN_FIELD_SLOTS=8 baseline (e.g. World.commandBuffer
                    // sits at slot 12). Pre-fix this branch capped the cache
                    // at `i < 8` which left every >8-slot field permanently
                    // missing the cache: every access fell through to a
                    // fresh keys_array walk + js_string_equals chain. On
                    // perf-comprehensive's hot loops that path was hit
                    // ~900k times per run (40% inclusive samples per
                    // perfcomp.profile).
                    (*cache)[0] = keys as i64;
                    (*cache)[1] = i as i64;
                    let field_ptr = (obj as *const u8)
                        .add(std::mem::size_of::<ObjectHeader>() + i * 8)
                        as *const f64;
                    return *field_ptr;
                }
            }
        }
    }
    let value = js_object_get_field_by_name(obj, key);
    f64::from_bits(value.bits())
}

// Polymorphic numeric-key get/set (`js_object_get_index_polymorphic` /
// `js_object_set_index_polymorphic`) live in `polymorphic_index.rs`:
// they dispatch by GC type (array vs object vs closure vs buffer) rather
// than touching object field storage directly, so they were split out
// of this module. See `polymorphic_index.rs` for the implementations
// and the #471 fix notes.

#[cfg(test)]
mod sso_tests_1781 {
    use super::*;

    #[test]
    fn object_keys_values_entries_on_string_do_not_crash() {
        // Regression: Object.keys/values/entries on a string segfaulted
        // (the value was deref'd as an ObjectHeader; SSO strings aren't even
        // pointers). Now they yield index keys / chars / [index,char].
        let heap = crate::string::js_string_from_bytes(b"abc".as_ptr(), 3);
        let v = crate::value::js_nanbox_string(heap as i64);
        assert_eq!(crate::array::js_array_length(js_object_keys_value(v)), 3);
        assert_eq!(crate::array::js_array_length(js_object_values_value(v)), 3);
        assert_eq!(crate::array::js_array_length(js_object_entries_value(v)), 3);
        // SSO string (<= 5 bytes) — the non-pointer case that crashed hardest.
        let sso = crate::value::JSValue::try_short_string(b"hi").unwrap();
        assert_eq!(
            crate::array::js_array_length(js_object_keys_value(f64::from_bits(sso.bits()))),
            2
        );
        // Number / boolean primitives → empty array (no own enumerable keys).
        assert_eq!(crate::array::js_array_length(js_object_keys_value(42.0)), 0);
    }

    /// #1781: `"id" in obj` for a key <= 5 bytes — the lookup key arrives as
    /// an inline SSO value (tag 0x7FF9). `is_string()` (STRING_TAG-only)
    /// rejected it, so `js_object_has_property` returned false even though the
    /// object had the key (stored keys are always heap, so materializing the
    /// SSO lookup key lets js_string_equals match).
    #[test]
    fn in_operator_finds_object_key_via_sso_lookup() {
        unsafe {
            let obj = crate::object::js_object_alloc(0, 0);
            let key = crate::string::js_string_from_bytes(b"id".as_ptr(), 2);
            crate::object::js_object_set_field_by_name(obj, key, 42.0);

            let obj_box = crate::value::js_nanbox_pointer(obj as i64);
            let sso = crate::value::JSValue::try_short_string(b"id").unwrap();
            assert!(sso.is_short_string());
            let present = js_object_has_property(obj_box, f64::from_bits(sso.bits()));
            assert_ne!(
                crate::value::js_is_truthy(present),
                0,
                "SSO key 'id' should be found via `in`"
            );

            let missing = crate::value::JSValue::try_short_string(b"zz").unwrap();
            let absent = js_object_has_property(obj_box, f64::from_bits(missing.bits()));
            assert_eq!(
                crate::value::js_is_truthy(absent),
                0,
                "absent SSO key 'zz' should not be found"
            );
        }
    }
}

#[no_mangle]
pub extern "C" fn js_private_brand_check(
    obj: f64,
    declaring_class_id: u32,
    field_name_ptr: *const u8,
    field_name_len: u32,
) -> f64 {
    let false_value = f64::from_bits(crate::value::TAG_FALSE);
    let true_value = f64::from_bits(crate::value::TAG_TRUE);
    if declaring_class_id == 0 || field_name_ptr.is_null() || field_name_len == 0 {
        return false_value;
    }

    let value = JSValue::from_bits(obj.to_bits());
    if !value.is_pointer() {
        return false_value;
    }
    let obj_ptr = value.as_pointer::<ObjectHeader>();
    if obj_ptr.is_null() {
        return false_value;
    }

    let obj_class_id = js_object_get_class_id(obj_ptr);
    if obj_class_id == 0 {
        return false_value;
    }

    let mut cur = obj_class_id;
    let mut has_declaring_brand = false;
    for _ in 0..32 {
        if cur == declaring_class_id {
            has_declaring_brand = true;
            break;
        }
        match super::class_registry::get_parent_class_id(cur) {
            Some(parent) if parent != 0 && parent != cur => cur = parent,
            _ => break,
        }
    }
    if !has_declaring_brand {
        return false_value;
    }

    true_value
}

#[cfg(test)]
mod buffer_ic_miss_tests {
    use super::*;

    unsafe fn key(bytes: &[u8]) -> *const crate::StringHeader {
        crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
    }

    unsafe fn string_value_bytes(value: f64) -> Vec<u8> {
        let bits = value.to_bits();
        assert_eq!((bits >> 48) as u16, 0x7fff);
        let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::StringHeader;
        let data = (ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        std::slice::from_raw_parts(data, (*ptr).byte_len as usize).to_vec()
    }

    unsafe fn secret_buffer(len: usize) -> *mut crate::buffer::BufferHeader {
        let buf = crate::buffer::buffer_alloc(len as u32);
        (*buf).length = len as u32;
        crate::buffer::mark_as_uint8array(buf as usize);
        crate::buffer::mark_as_secret_key(buf as usize);
        buf
    }

    #[test]
    fn secret_key_buffer_metadata_survives_ic_miss_for_aes_sizes() {
        unsafe {
            for len in [16usize, 24, 32] {
                let buf = secret_buffer(len);
                let mut cache = [0i64; 2];

                let ty = js_object_get_field_ic_miss(
                    buf as *const ObjectHeader,
                    key(b"type"),
                    &mut cache,
                );
                assert_eq!(string_value_bytes(ty), b"secret");

                let size = js_object_get_field_ic_miss(
                    buf as *const ObjectHeader,
                    key(b"symmetricKeySize"),
                    &mut cache,
                );
                assert_eq!(size, len as f64);

                let raw = dispatch_buffer_method(buf as usize, "export", std::ptr::null(), 0);
                let raw_addr = (raw.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *const ObjectHeader;
                let raw_len = js_object_get_field_ic_miss(raw_addr, key(b"length"), &mut cache);
                assert_eq!(raw_len, len as f64);
            }
        }
    }
}
