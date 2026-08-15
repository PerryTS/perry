//! Indexed field get + accessor/prototype-property helpers.
//! Pure relocation out of field_get_set.rs (issue #1103 split).

use super::*;

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
        let fc = crate::object::object_live_slot_count(obj);
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
                fc
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
    let alloc_limit = std::cmp::max(
        crate::object::object_live_slot_count(obj),
        crate::object::INLINE_SLOT_FLOOR as u32,
    ) as usize;
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

crate::perry_thread_local! {
    static OBJECT_PROTOTYPE_LOOKUP_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

struct ObjectPrototypeLookupGuard;

impl Drop for ObjectPrototypeLookupGuard {
    fn drop(&mut self) {
        OBJECT_PROTOTYPE_LOOKUP_DEPTH.with(|depth| {
            depth.set(depth.get().saturating_sub(1));
        });
    }
}

fn object_prototype_lookup_guard() -> Option<ObjectPrototypeLookupGuard> {
    OBJECT_PROTOTYPE_LOOKUP_DEPTH.with(|depth| {
        if depth.get() != 0 {
            None
        } else {
            depth.set(1);
            Some(ObjectPrototypeLookupGuard)
        }
    })
}

unsafe fn default_object_prototype_property_value(
    receiver_addr: usize,
    key: *const crate::StringHeader,
) -> Option<JSValue> {
    let _guard = object_prototype_lookup_guard()?;
    // #7498: THIS IS THE FRAME `PERRY_GC_PROTECT_FROMSPACE=1` FAULTS IN on the
    // `[...obj.arr]` path — a 56-byte from-space `GC_TYPE_STRING`, i.e. `key`.
    // Both arguments are GC-managed and both are live across the call below
    // before their first use: the recursive `js_object_get_field_by_name` on
    // `Object.prototype` can run an accessor, which is user code, and user code
    // allocates. A copying minor there moves the key string and the receiver and
    // rewrites only the slots it can see; a bare argument is not one.
    // (#7795 removed the two resolution calls that used to allocate here as
    // well — the rooting is still required for the prototype read itself.)
    //
    // Root both before the first of those calls and read each back at its
    // point of use. NaN-boxed handles only, so this module adds no bare
    // `get_raw_*_ptr` to `scripts/raw_handle_debt.py`.
    let scope = crate::gc::RuntimeHandleScope::new();
    let key_h = scope.root_nanbox_f64(crate::value::nanbox_string_key(key));
    let receiver_h = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(receiver_addr as i64));
    let key = || {
        crate::value::js_nanbox_get_pointer(key_h.get_nanbox_f64()) as *const crate::StringHeader
    };
    let receiver_addr =
        || crate::value::js_nanbox_get_pointer(receiver_h.get_nanbox_f64()) as usize;

    // #7795: resolve `Object.prototype` from the memoized, GC-healed cache
    // instead of re-running `globalThis.Object` (which interns an `"Object"`
    // key string) plus a `closure_get_dynamic_prop("prototype")` on EVERY
    // ordinary-object property MISS. `object_prototype_addr` performs exactly
    // this resolution, caches only a successful one, heals the address through
    // the forwarding chain, and is itself a registered GC root
    // (`scan_prototype_addr_cache_roots_mut`) — the array index-read fast path
    // already depends on it. `Object.prototype` is non-writable and
    // non-configurable per spec, so the memo cannot go stale.
    let proto_addr = crate::array::object_prototype_addr();
    if proto_addr == 0 {
        return None;
    }
    let proto_ptr = proto_addr as *mut ObjectHeader;
    if proto_ptr as usize == receiver_addr() {
        return None;
    }
    let receiver = crate::value::js_nanbox_pointer(receiver_addr() as i64);
    let previous_this = super::super::js_implicit_this_set(receiver);
    // The DISPLACED `this` and the displaced accessor receiver both ride
    // through `js_object_get_field_by_name` — which can run a getter — before
    // being republished. Rooting the ACCESSOR_RECEIVER_OVERRIDE cell (#7231)
    // protects the armed value, not these saved ones; that residual is what
    // these two handles close.
    let previous_this_h = scope.root_nanbox_f64(previous_this);
    let prev_override = accessor_receiver_override_begin(receiver);
    let prev_override_h = prev_override.map(|v| scope.root_nanbox_f64(v));
    let property = js_object_get_field_by_name(proto_ptr, key());
    accessor_receiver_override_end(prev_override_h.map(|h| h.get_nanbox_f64()));
    super::super::js_implicit_this_set(previous_this_h.get_nanbox_f64());
    if property.is_undefined() {
        None
    } else {
        Some(property)
    }
}

pub(crate) unsafe fn ordinary_object_prototype_property_value(
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
    if super::super::prototype_chain::object_static_prototype(obj as usize).is_some() {
        return None;
    }
    let class_id = (*obj).class_id;
    if class_id != 0 && !is_anon_shape_class_id(class_id) {
        return None;
    }
    default_object_prototype_property_value(obj as usize, key)
}

crate::perry_thread_local! {
    /// Receiver to bind when an accessor getter is reached by walking a
    /// prototype chain. `js_object_get_field_by_name(proto, key)` re-derives the
    /// accessor receiver from its `obj` argument — which is the PROTOTYPE during
    /// an inherited read, not the original instance. `resolve_inherited_field`
    /// stashes the real receiver here for the duration of the walk; the getter
    /// invocation consumes it so `this` is the instance, matching the spec's
    /// `[[Get]](P, Receiver)`. (object-literal getters on a `Object.create`
    /// prototype — e.g. @hono/node-server's request prototype reading
    /// `this[incomingKey].method`.)
    ///
    /// **This is a GC root, and must stay one (#7231).** The stashed receiver
    /// is a NaN-boxed heap value that stays armed for the whole prototype
    /// walk, and a walk can reach a Proxy `get` trap — arbitrary user code
    /// that allocates. Nothing else refers to it while it sits here, so
    /// without `scan_accessor_receiver_override_root_mut` the getter is
    /// invoked with a `this` naming from-space.
    ///
    /// RESIDUAL, same shape as `CURRENT_NEW_TARGET`: the displaced value that
    /// `accessor_receiver_override_begin` returns rides a bare Rust local
    /// through the walk and is republished by `_end`. Rooting the cell
    /// protects the ARMED value, not the saved one.
    static ACCESSOR_RECEIVER_OVERRIDE: std::cell::Cell<Option<f64>>
        = const { std::cell::Cell::new(None) };
}

/// Root + rewrite the in-flight inherited-accessor receiver.
pub(crate) fn scan_accessor_receiver_override_root_mut(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
) {
    ACCESSOR_RECEIVER_OVERRIDE.with(|cell| {
        if let Some(mut value) = cell.get() {
            if visitor.visit_nanbox_f64_slot(&mut value) {
                cell.set(Some(value));
            }
        }
    });
}

pub(crate) fn accessor_receiver_override_begin(receiver: f64) -> Option<f64> {
    ACCESSOR_RECEIVER_OVERRIDE.with(|c| {
        // Keep the OUTERMOST receiver across multi-hop prototype walks.
        let to_set = c.get().or(Some(receiver));
        c.replace(to_set)
    })
}

pub(crate) fn accessor_receiver_override_end(prev: Option<f64>) {
    ACCESSOR_RECEIVER_OVERRIDE.with(|c| c.set(prev));
}

/// `this` to pass to a class getter (vtable `getters`) found while resolving a
/// property. When the getter was reached by walking a prototype chain, `obj` is
/// the PROTOTYPE the getter lives on — bind the original instance stashed by
/// `resolve_inherited_field` instead. Take() consumes it so the getter body
/// runs with a clean override.
pub(crate) unsafe fn class_getter_this(obj: *const ObjectHeader) -> f64 {
    ACCESSOR_RECEIVER_OVERRIDE
        .with(|c| c.take())
        .unwrap_or_else(|| f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits()))
}

pub(crate) unsafe fn invoke_accessor_getter(get_bits: u64, receiver: f64) -> JSValue {
    let closure = (get_bits & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader;
    if closure.is_null() {
        return JSValue::undefined();
    }
    // Consume any inherited-receiver override: the getter's `this` must be the
    // original instance, not the prototype the accessor lives on. Take() clears
    // it so the getter BODY runs with a fresh override (a nested inherited read
    // inside the getter gets its own).
    let eff_receiver = ACCESSOR_RECEIVER_OVERRIDE
        .with(|c| c.take())
        .unwrap_or(receiver);
    // OrdinaryCallBindThis: a primitive receiver (accessor inherited from
    // Number.prototype / Object.prototype etc.) is boxed ONCE up front for a
    // sloppy getter; a strict getter observes the raw primitive.
    let eff_receiver = crate::closure::coerce_call_this(f64::from_bits(get_bits), eff_receiver);
    let call_bits = crate::closure::clone_closure_rebind_this(get_bits, eff_receiver);
    let closure = (call_bits & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader;
    if closure.is_null() {
        return JSValue::undefined();
    }
    let prev = super::super::js_implicit_this_set(eff_receiver);
    let result_f64 = crate::closure::js_closure_call0(closure);
    super::super::js_implicit_this_set(prev);
    JSValue::from_bits(result_f64.to_bits())
}

/// Setter analog of [`invoke_accessor_getter`]: rebinds `this` to the
/// receiver and invokes the setter closure with the assigned value.
pub(crate) unsafe fn invoke_accessor_setter(set_bits: u64, receiver: f64, value: f64) {
    let closure = (set_bits & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader;
    if closure.is_null() {
        return;
    }
    // Strict/sloppy receiver coercion — see invoke_accessor_getter.
    let receiver = crate::closure::coerce_call_this(f64::from_bits(set_bits), receiver);
    let call_bits = crate::closure::clone_closure_rebind_this(set_bits, receiver);
    let closure = (call_bits & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader;
    if closure.is_null() {
        return;
    }
    let prev = super::super::js_implicit_this_set(receiver);
    let _ = crate::closure::js_closure_call1(closure, value);
    super::super::js_implicit_this_set(prev);
}

/// Invoke an accessor owned by a descriptor-marked object before its empty
/// backing slot is read. Gate-neutral builtin installs deliberately leave the
/// process-wide `ACCESSORS_IN_USE` flag clear, but stamp their owner with
/// `OBJ_FLAG_HAS_DESCRIPTORS`; the caller checks that bit before entering this
/// helper, so ordinary object reads pay only the already-loaded header-bit
/// test. This also makes direct reads of builtin prototype accessors preserve
/// their real behavior (`Set.prototype.size` throws, `RegExp.prototype.source`
/// returns `"(?:)"`, and so on) once startup is descriptor-gate-free.
pub(crate) unsafe fn builtin_reflection_accessor_read(
    obj: *const ObjectHeader,
    key_bytes: &[u8],
) -> Option<JSValue> {
    let name = std::str::from_utf8(key_bytes).ok()?;
    let acc = get_accessor_descriptor(obj as usize, name)?;
    if acc.get == 0 {
        return Some(JSValue::undefined());
    }
    let receiver = crate::value::js_nanbox_pointer(obj as i64);
    Some(invoke_accessor_getter(acc.get, receiver))
}

/// True when `addr` is the shared `%TypedArray%.prototype` intrinsic or one of
/// the per-kind typed-array prototypes (`Int8Array.prototype`, …). These objects
/// host the `%TypedArray%.prototype` methods/getters but are NOT themselves
/// typed arrays, so a method invoked directly on them (e.g.
/// `Int8Array.prototype.entries()`) must fail `ValidateTypedArray` and throw a
/// `TypeError`. Mirrors the per-kind/intrinsic detection in
/// the builtin-accessor read path.
pub(crate) unsafe fn is_typed_array_prototype(addr: usize) -> bool {
    if addr == 0 || (addr as u64) >> 48 != 0 || !super::super::is_valid_obj_ptr(addr as *const u8) {
        return false;
    }
    let intrinsic_proto =
        super::super::TYPED_ARRAY_INTRINSIC_PROTO_PTR.load(std::sync::atomic::Ordering::Relaxed);
    if intrinsic_proto != 0 && addr as i64 == intrinsic_proto {
        return true;
    }
    // Per-kind protos are plain `GC_TYPE_OBJECT`s carrying the proto flag in the
    // shared `_reserved` word; gate the flag read on the object type so a
    // regular array whose `_reserved` happens to collide isn't misclassified.
    let gc = (addr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    (*gc).obj_type == crate::gc::GC_TYPE_OBJECT
        && ((*gc)._reserved & crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO) != 0
}

pub(crate) unsafe fn primitive_object_prototype_accessor(
    name: &str,
    receiver: f64,
) -> Option<JSValue> {
    if !crate::state::state().descriptors.accessors_in_use.get() {
        return None;
    }
    let object_ctor = super::super::js_get_global_this_builtin_value(b"Object".as_ptr(), 6);
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

pub(crate) unsafe fn primitive_builtin_prototype_property(
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
    // An ACCESSOR installed on the builtin prototype
    // (`Object.defineProperty(Number.prototype, "x", { get(){…} })`) must run
    // with the ORIGINAL primitive receiver — boxed/raw per getter strictness
    // inside `invoke_accessor_getter` — not the prototype object the accessor
    // happens to live on (which a plain field read below would hand it).
    if crate::state::state().descriptors.accessors_in_use.get() {
        let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let key_len = (*key).byte_len as usize;
        if let Ok(name) = std::str::from_utf8(std::slice::from_raw_parts(key_ptr, key_len)) {
            if let Some(acc) = get_accessor_descriptor(proto_ptr as usize, name) {
                if acc.get == 0 {
                    return Some(JSValue::undefined());
                }
                return Some(invoke_accessor_getter(acc.get, receiver));
            }
        }
    }
    let value = js_object_get_field_by_name(proto_ptr, key);
    if value.is_undefined() {
        return None;
    }
    Some(bind_closure_value_to_receiver(value, receiver))
}

pub(crate) unsafe fn string_index_value(
    str_value: f64,
    key: *const crate::StringHeader,
) -> Option<JSValue> {
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

pub(crate) unsafe fn array_prototype_property_value(
    name: &str,
    receiver_addr: usize,
) -> Option<JSValue> {
    // #7498 — THE FAULT `PERRY_GC_PROTECT_FROMSPACE=1` REPORTS FOR
    // `[...obj.arr]`, measured with lldb: `EXC_BAD_ACCESS` on the `ldrsb` of
    // the UTF-8 scan inside `js_string_from_bytes` below, reading a 56-byte
    // retired-from-space `GC_TYPE_STRING`.
    //
    // `name` is not an owned string. `get_field_by_name_object_tail` slices it
    // straight out of the key `StringHeader`'s payload
    // (`slice::from_raw_parts(key_ptr, key_len)`), so it is a BORROW OF THE GC
    // HEAP — and a borrow is exactly the thing the collector cannot see or
    // rewrite. Every call below allocates: `js_get_global_this_builtin_value`
    // interns `"Array"`, `closure_get_dynamic_prop` can run an accessor, and
    // `js_string_from_bytes` reads its SOURCE bytes *after* its own
    // `string_storage_alloc`. Any one of those can move the key out from under
    // `name`.
    //
    // A `RuntimeHandleScope` cannot fix this: rooting the key would keep the
    // object alive and rewrite the slot, but `name`'s pointer is a `&str`, not
    // a slot. The only sound shape is to stop borrowing the heap — see
    // [`HeapKeyBytes`].
    let name_copy = super::HeapKeyBytes::copy_of(name.as_bytes());
    let name: &str = std::str::from_utf8_unchecked(name_copy.as_bytes());

    let ctor = super::super::js_get_global_this_builtin_value(b"Array".as_ptr(), 5);
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
    // #7498: `js_string_from_bytes` ALLOCATES, so `Array.prototype` and the
    // receiver cannot be carried across it as bare `usize`s — and the key it
    // produces is itself a fresh heap string this function then hands to two
    // more calls that can collect (`js_object_get_field_by_name` runs getters;
    // `default_object_prototype_property_value` interns another key). Root all
    // three and read each back at its point of use.
    let scope = crate::gc::RuntimeHandleScope::new();
    let proto_h = scope.root_nanbox_f64(proto);
    let receiver_h = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(receiver_addr as i64));
    let key_h = scope.root_nanbox_f64(crate::value::nanbox_string_key(
        crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32),
    ));
    let proto_ptr = || crate::value::js_nanbox_get_pointer(proto_h.get_nanbox_f64()) as usize;
    let receiver_addr =
        || crate::value::js_nanbox_get_pointer(receiver_h.get_nanbox_f64()) as usize;
    let key = || {
        crate::value::js_nanbox_get_pointer(key_h.get_nanbox_f64()) as *const crate::StringHeader
    };

    if let Some(v) = own_data_field_by_name(proto_ptr() as *const ObjectHeader, key()) {
        return Some(v);
    }
    if let Some(v) = crate::array::array_named_property_get_by_name(
        proto_ptr() as *const crate::array::ArrayHeader,
        name,
    ) {
        return Some(JSValue::from_bits(v.to_bits()));
    }
    if proto_ptr() == receiver_addr() {
        return default_object_prototype_property_value(receiver_addr(), key());
    }
    let receiver = crate::value::js_nanbox_pointer(receiver_addr() as i64);
    let prev_override = accessor_receiver_override_begin(receiver);
    let prev_override_h = prev_override.map(|v| scope.root_nanbox_f64(v));
    let v = js_object_get_field_by_name(proto_ptr() as *const ObjectHeader, key());
    accessor_receiver_override_end(prev_override_h.map(|h| h.get_nanbox_f64()));
    if v.is_undefined() {
        default_object_prototype_property_value(receiver_addr(), key())
    } else {
        Some(v)
    }
}
