use super::*;
use crate::object::*;
use crate::{ArrayHeader, JSValue};
use std::cell::{Cell, RefCell, UnsafeCell};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicU8, Ordering};
use std::sync::RwLock;

/// Register a class id so `js_value_typeof` can distinguish class refs
/// (INT32-tagged with class_id payload) from real int32 numeric values.
#[no_mangle]
pub unsafe extern "C" fn js_register_class_id(class_id: u32) {
    if class_id == 0 {
        return;
    }
    let mut guard = REGISTERED_CLASS_IDS.write().unwrap();
    if guard.is_none() {
        *guard = Some(std::collections::HashSet::new());
    }
    guard.as_mut().unwrap().insert(class_id);
}

/// Maps `class_id → user-visible class name`. Populated by codegen via
/// `js_register_class_name`. Read back by V8-bridge code when surfacing a
/// Perry class to JS — NestJS's `ModuleTokenFactory.create()` reads
/// `metatype.name` to build the module token, so the empty default name
/// from `v8::Function::builder(...)` would collide every module under the
/// same token. (#1021.)
pub static CLASS_NAMES: RwLock<Option<HashMap<u32, String>>> = RwLock::new(None);

/// Register the user-visible name of a class so the V8 bridge can label
/// the V8-side wrapper for nice `metatype.name` reads. Idempotent.
///
/// A zero-length name is a legitimate registration, not a no-op: an anonymous
/// class expression's `.name` IS the empty string per spec (issue #5952 —
/// `const M = Mixin(Base)` over `function Mixin(B) { return class extends B {} }`
/// binds a class whose `name` is `""`). Membership in `CLASS_NAMES` means "this
/// id is a real class", which stays true for an anonymous one; only the null
/// pointer and the reserved id 0 are rejected.
#[no_mangle]
pub unsafe extern "C" fn js_register_class_name(class_id: u32, name_ptr: *const u8, name_len: u32) {
    if class_id == 0 || name_ptr.is_null() {
        return;
    }
    let slice = std::slice::from_raw_parts(name_ptr, name_len as usize);
    let name = match std::str::from_utf8(slice) {
        Ok(s) => s.to_string(),
        Err(_) => return,
    };
    let mut guard = CLASS_NAMES.write().unwrap();
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    guard.as_mut().unwrap().insert(class_id, name);
}

/// Look up the user-visible name of a registered class. Returns `None`
/// when the class id was never registered with `js_register_class_name`.
pub fn class_name_for_id(class_id: u32) -> Option<String> {
    let guard = CLASS_NAMES.read().ok()?;
    guard.as_ref()?.get(&class_id).cloned()
}

/// Whether dynamic-dispatch miss diagnostics are enabled (`PERRY_DISPATCH_DIAG`,
/// any non-empty/non-falsey value). Cached on first read.
///
/// When a dynamic dispatch falls through every resolution tower (vtable,
/// static-method, static-field, prototype, field-scan, namespace, symbol), the
/// runtime returns a *silent placeholder* — the receiver class ref, an empty
/// object, `undefined`, etc. — rather than throwing, because some of those
/// placeholders are load-bearing (effect's `.pipe()` chains yield the class ref
/// during module init, #687). The upside is no spurious crashes; the downside
/// is a typo'd / unsupported member surfaces far downstream as a stray
/// `{}`/`1`/`[]`/function, turning each one into a multi-hour localization.
///
/// This flag doesn't change behavior — it just prints a located, typed report
/// at the moment of the miss, so the bug surfaces at its true call site.
pub(crate) fn dispatch_diag_enabled() -> bool {
    use std::sync::OnceLock;
    static EN: OnceLock<bool> = OnceLock::new();
    *EN.get_or_init(|| {
        std::env::var("PERRY_DISPATCH_DIAG")
            .map(|v| !v.is_empty() && v != "0" && v != "off" && v != "false")
            .unwrap_or(false)
    })
}

/// Best-effort one-line description of a dispatch receiver for diagnostics:
/// class refs resolve to their registered name, pointers/primitives to a tag.
fn describe_dispatch_receiver(recv: f64) -> String {
    let bits = recv.to_bits();
    let top16 = bits >> 48;
    if top16 == 0x7FFE {
        let cid = (bits & 0xFFFF_FFFF) as u32;
        return match class_name_for_id(cid) {
            Some(n) => format!("class-ref `{}` (id {})", n, cid),
            None => format!("class-ref (id {})", cid),
        };
    }
    if top16 == 0x7FFF || top16 == 0x7FF9 {
        return "string".to_string();
    }
    if top16 == 0x7FFD {
        return "object/pointer".to_string();
    }
    match bits {
        x if x == crate::value::TAG_UNDEFINED => "undefined".to_string(),
        0x7FFC_0000_0000_0002 => "null".to_string(),
        0x7FFC_0000_0000_0003 => "false".to_string(),
        0x7FFC_0000_0000_0004 => "true".to_string(),
        _ if !recv.is_nan() => format!("number {}", recv),
        _ => "value".to_string(),
    }
}

/// Report a true dynamic-dispatch miss to stderr (only when
/// `PERRY_DISPATCH_DIAG` is set). `tower` names which resolution path fell
/// through; `returning` is the silent placeholder the runtime is about to hand
/// back. No-op (and near-zero cost) when the flag is off.
pub(crate) fn report_dispatch_miss(tower: &str, recv: f64, name: &str, returning: &str) {
    if !dispatch_diag_enabled() {
        return;
    }
    eprintln!(
        "[perry dispatch-miss] {tower}: {}.{:?} did not resolve \u{2192} returning {returning}. \
         A dynamic dispatch fell through every tower; downstream this usually surfaces as a stray \
         {{}}/1/[]/function. Check the call site for {:?}.",
        describe_dispatch_receiver(recv),
        name,
        name
    );
}

/// Resolve a closure-typed JSValue back to a built-in constructor name
/// (`"Date"`/`"Array"`/`"Object"`/...) when it matches one of the
/// singleton-installed thunks. Returns `None` for closures that aren't
/// the globalThis built-in constructors. Used by
/// `js_new_function_construct` to dispatch `new <inst.constructor>(...)`
/// shapes (date-fns `constructFrom`, lodash-style `Array` cloning, ...)
/// to the right runtime factory.
pub(crate) fn identify_global_builtin_constructor(func_value: f64) -> Option<&'static str> {
    use crate::value::JSValue;
    let jv = JSValue::from_bits(func_value.to_bits());
    if !jv.is_pointer() {
        return None;
    }
    let ptr = jv.as_pointer() as *const crate::closure::ClosureHeader;
    if ptr.is_null() {
        return None;
    }
    if !(ptr as usize).is_multiple_of(std::mem::align_of::<crate::closure::ClosureHeader>()) {
        return None;
    }
    if !is_valid_obj_ptr(ptr as *const u8) {
        return None;
    }
    // Identify by the closure's read-only `func_ptr` rather than the
    // GC-movable ClosureHeader address. Both the date-fns ctor closure
    // and the (later-evacuated) ctor closure carry the same
    // `global_this_builtin_noop_thunk` function pointer, so this match
    // survives GC moves. The per-name lookup must then walk the
    // globalThis singleton's keys to recover the constructor name —
    // accept the extra hop only when the func_ptr matches.
    unsafe {
        if (*ptr).type_tag != crate::closure::CLOSURE_MAGIC {
            return None;
        }
        let func_ptr = (*ptr).func_ptr as usize;
        let is_global_builtin_func = func_ptr
            == global_this_builtin_noop_thunk as *const u8 as usize
            || func_ptr == typed_array_constructor_call_thunk as *const u8 as usize
            // #4102: `Array`/`Object`/`Date` constructor *values* carry their own
            // coercion thunks (not the shared noop thunk), so the dynamic
            // `instanceof` / reflective `@@hasInstance` path could not recover
            // their name. Accept those thunks too; the singleton walk below maps
            // each back to "Array"/"Object"/"Date".
            || func_ptr == global_this_array_thunk as *const u8 as usize
            || func_ptr == global_this_object_thunk as *const u8 as usize
            || func_ptr == global_this_date_thunk as *const u8 as usize
            || func_ptr == global_this_blob_thunk as *const u8 as usize
            || func_ptr == global_this_file_thunk as *const u8 as usize
            || func_ptr == global_this_headers_thunk as *const u8 as usize
            || func_ptr == global_this_request_thunk as *const u8 as usize
            || func_ptr == global_this_response_thunk as *const u8 as usize
            || func_ptr == global_this_string_thunk as *const u8 as usize
            || func_ptr == global_this_number_thunk as *const u8 as usize
            || func_ptr == global_this_boolean_thunk as *const u8 as usize
            || func_ptr == error_constructor_call_thunk as *const u8 as usize
            || func_ptr == type_error_constructor_call_thunk as *const u8 as usize
            || func_ptr == range_error_constructor_call_thunk as *const u8 as usize
            || func_ptr == reference_error_constructor_call_thunk as *const u8 as usize
            || func_ptr == syntax_error_constructor_call_thunk as *const u8 as usize
            || func_ptr == eval_error_constructor_call_thunk as *const u8 as usize
            || func_ptr == uri_error_constructor_call_thunk as *const u8 as usize
            || func_ptr == webcrypto_illegal_constructor_thunk as *const u8 as usize
            // Map/Set/WeakMap/WeakSet/WeakRef constructor *values* carry their
            // own "requires 'new'" thunks (global_this.rs). When obtained as a
            // value and constructed via `new $WeakMap()` (e.g. qs's
            // `side-channel`/`get-intrinsic` reads `%WeakMap%` into a variable),
            // the call lands here, not the static codegen path. Accept the
            // thunks so the singleton walk recovers the name and the match arms
            // below dispatch into the real factory instead of invoking the
            // bare-call thunk (which throws "Constructor WeakMap requires 'new'").
            || func_ptr == map_constructor_call_thunk as *const u8 as usize
            || func_ptr == set_constructor_call_thunk as *const u8 as usize
            || func_ptr == weak_map_constructor_call_thunk as *const u8 as usize
            || func_ptr == weak_set_constructor_call_thunk as *const u8 as usize
            || func_ptr == weak_ref_constructor_call_thunk as *const u8 as usize
            // `class X extends Promise` needs its parent VALUE recognized as the
            // Promise constructor (for the runtime `new Subclass` /
            // `NewPromiseCapability(Subclass)` path). The Promise ctor value
            // carries `promise_constructor_call_thunk`.
            || func_ptr == promise_constructor_call_thunk as *const u8 as usize
            || func_ptr
                == crate::messaging::js_message_channel_constructor_call_error as *const u8
                    as usize
            || func_ptr
                == crate::messaging::js_message_port_constructor_call_error as *const u8 as usize
            || func_ptr
                == crate::messaging::js_broadcast_channel_constructor_call_error as *const u8
                    as usize;
        if !is_global_builtin_func {
            return None;
        }
        // #5989: dedicated per-builtin thunks map to their name DIRECTLY —
        // without consulting globalThis. The name-record/singleton-walk
        // fallbacks below break the moment user code REASSIGNS the global
        // binding (Next.js 16's cacheComponents extensions install a `Date`
        // wrapper via `Date = createDate(Date)`; the wrapper's captured
        // ORIGINAL constructor then no longer matches any globalThis key,
        // identification returned None, and `Reflect.construct(original,
        // args, newTarget)` fell to the generic tail → unbranded "Invalid
        // Date" instances). Only the SHARED thunks (noop, typed-array)
        // still need the walk.
        let direct: Option<&'static str> =
            if func_ptr == global_this_date_thunk as *const u8 as usize {
                Some("Date")
            } else if func_ptr == global_this_array_thunk as *const u8 as usize {
                Some("Array")
            } else if func_ptr == global_this_object_thunk as *const u8 as usize {
                Some("Object")
            } else if func_ptr == global_this_string_thunk as *const u8 as usize {
                Some("String")
            } else if func_ptr == global_this_number_thunk as *const u8 as usize {
                Some("Number")
            } else if func_ptr == global_this_boolean_thunk as *const u8 as usize {
                Some("Boolean")
            } else if func_ptr == global_this_blob_thunk as *const u8 as usize {
                Some("Blob")
            } else if func_ptr == global_this_file_thunk as *const u8 as usize {
                Some("File")
            } else if func_ptr == global_this_headers_thunk as *const u8 as usize {
                Some("Headers")
            } else if func_ptr == global_this_request_thunk as *const u8 as usize {
                Some("Request")
            } else if func_ptr == global_this_response_thunk as *const u8 as usize {
                Some("Response")
            } else if func_ptr == error_constructor_call_thunk as *const u8 as usize {
                Some("Error")
            } else if func_ptr == type_error_constructor_call_thunk as *const u8 as usize {
                Some("TypeError")
            } else if func_ptr == range_error_constructor_call_thunk as *const u8 as usize {
                Some("RangeError")
            } else if func_ptr == reference_error_constructor_call_thunk as *const u8 as usize {
                Some("ReferenceError")
            } else if func_ptr == syntax_error_constructor_call_thunk as *const u8 as usize {
                Some("SyntaxError")
            } else if func_ptr == eval_error_constructor_call_thunk as *const u8 as usize {
                Some("EvalError")
            } else if func_ptr == uri_error_constructor_call_thunk as *const u8 as usize {
                Some("URIError")
            } else if func_ptr == map_constructor_call_thunk as *const u8 as usize {
                Some("Map")
            } else if func_ptr == set_constructor_call_thunk as *const u8 as usize {
                Some("Set")
            } else if func_ptr == weak_map_constructor_call_thunk as *const u8 as usize {
                Some("WeakMap")
            } else if func_ptr == weak_set_constructor_call_thunk as *const u8 as usize {
                Some("WeakSet")
            } else if func_ptr == weak_ref_constructor_call_thunk as *const u8 as usize {
                Some("WeakRef")
            } else if func_ptr == promise_constructor_call_thunk as *const u8 as usize {
                Some("Promise")
            } else {
                None
            };
        if direct.is_some() {
            return direct;
        }
    }
    // Prefer the per-closure built-in `.name` record. Full-suite Rust tests
    // temporarily seed GLOBAL_THIS_PTR with GC fixture pointers; relying only
    // on the singleton walk below makes unrelated tests race with constructor
    // identity for globals such as TextEncoderStream.
    let name_value = crate::value::JSValue::from_bits(
        crate::closure::closure_get_dynamic_prop(ptr as usize, "name").to_bits(),
    );
    if name_value.is_string() {
        let name_ptr = name_value.as_string_ptr();
        if !name_ptr.is_null() {
            let name_bytes = unsafe {
                let data = (name_ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                std::slice::from_raw_parts(data, (*name_ptr).byte_len as usize)
            };
            if let Ok(name) = std::str::from_utf8(name_bytes) {
                for builtin in GLOBAL_THIS_BUILTIN_CONSTRUCTORS.iter().copied() {
                    if builtin == name {
                        return Some(builtin);
                    }
                }
            }
        }
    }
    // Find which builtin name maps to this exact closure header on the
    // singleton. Walk via the existing
    // `js_get_global_this_builtin_value` helper — short loop (≤ ~50
    // entries), only fires on the constructFrom hot path.
    let global_this_f64 = js_get_global_this();
    let global_obj = crate::value::js_nanbox_get_pointer(global_this_f64) as *const ObjectHeader;
    if global_obj.is_null() {
        return None;
    }
    for name in GLOBAL_THIS_BUILTIN_CONSTRUCTORS.iter().copied() {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let v = unsafe { js_object_get_field_by_name(global_obj, key) };
        if v.bits() == jv.bits() {
            return Some(name);
        }
    }
    None
}

pub(crate) fn text_decoder_bool_option(options: f64, name: &str) -> f64 {
    let jsval = crate::value::JSValue::from_bits(options.to_bits());
    if !jsval.is_pointer() {
        return f64::from_bits(crate::value::TAG_FALSE);
    }
    let obj = jsval.as_pointer::<ObjectHeader>();
    if obj.is_null() {
        return f64::from_bits(crate::value::TAG_FALSE);
    }
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let value = js_object_get_field_by_name(obj, key);
    let value_f64 = f64::from_bits(value.bits());
    f64::from_bits(crate::value::JSValue::bool(crate::value::js_is_truthy(value_f64) != 0).bits())
}

pub(crate) unsafe fn validate_web_compression_stream_format(format: f64) {
    let ptr = crate::builtins::js_string_coerce(format) as *const crate::StringHeader;
    if ptr.is_null() {
        crate::fs::validate::throw_type_error_with_code(
            "The argument 'format' is invalid.",
            "ERR_INVALID_ARG_VALUE",
        );
    }
    let len = (*ptr).byte_len as usize;
    let data = (ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    let bytes = std::slice::from_raw_parts(data, len);
    if matches!(bytes, b"gzip" | b"deflate" | b"deflate-raw" | b"brotli") {
        return;
    }
    let received = String::from_utf8_lossy(bytes);
    let message = format!("The argument 'format' is invalid. Received '{received}'");
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_VALUE");
}

pub(crate) const CLASS_ID_TEXT_ENCODER_STREAM: u32 = 0x7FFF_FF30;
pub(crate) const CLASS_ID_TEXT_DECODER_STREAM: u32 = 0x7FFF_FF31;
pub(crate) const CLASS_ID_COMPRESSION_STREAM: u32 = 0x7FFF_FF32;
pub(crate) const CLASS_ID_DECOMPRESSION_STREAM: u32 = 0x7FFF_FF33;

pub(crate) unsafe fn text_encoding_stream_new_with_constructor(
    constructor: f64,
    class_id: u32,
) -> f64 {
    let stream = js_object_alloc(class_id, 0);
    if stream.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }

    for key_bytes in [b"readable".as_slice(), b"writable".as_slice()] {
        let key = crate::string::js_string_from_bytes(key_bytes.as_ptr(), key_bytes.len() as u32);
        let endpoint = js_object_alloc(0, 0);
        let value = if endpoint.is_null() {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        } else {
            crate::value::js_nanbox_pointer(endpoint as i64)
        };
        js_object_set_field_by_name(stream, key, value);
    }

    let ctor_key = crate::string::js_string_from_bytes(b"constructor".as_ptr(), 11);
    js_object_set_field_by_name(stream, ctor_key, constructor);

    crate::value::js_nanbox_pointer(stream as i64)
}

unsafe fn text_encoding_stream_new(constructor_name: &[u8], class_id: u32) -> f64 {
    let ctor = js_get_global_this_builtin_value(constructor_name.as_ptr(), constructor_name.len());
    text_encoding_stream_new_with_constructor(ctor, class_id)
}

#[cfg(test)]
pub(crate) unsafe fn test_text_encoding_stream_new_with_constructor(
    constructor: f64,
    class_id: u32,
) -> f64 {
    text_encoding_stream_new_with_constructor(constructor, class_id)
}

#[no_mangle]
pub unsafe extern "C" fn js_text_encoder_stream_new() -> f64 {
    text_encoding_stream_new(b"TextEncoderStream", CLASS_ID_TEXT_ENCODER_STREAM)
}

#[no_mangle]
pub unsafe extern "C" fn js_text_decoder_stream_new() -> f64 {
    text_encoding_stream_new(b"TextDecoderStream", CLASS_ID_TEXT_DECODER_STREAM)
}

#[no_mangle]
pub unsafe extern "C" fn js_compression_stream_new() -> f64 {
    text_encoding_stream_new(b"CompressionStream", CLASS_ID_COMPRESSION_STREAM)
}

#[no_mangle]
pub unsafe extern "C" fn js_decompression_stream_new() -> f64 {
    text_encoding_stream_new(b"DecompressionStream", CLASS_ID_DECOMPRESSION_STREAM)
}

#[no_mangle]
pub unsafe extern "C" fn js_text_encoding_stream_new() -> f64 {
    js_text_encoder_stream_new()
}

/// Synthetic-anonymous-shape class IDs: classes the HIR generates for
/// bare object literals (`{ x: 1 }` → `__AnonShape_<hash>`). Instances
/// of these shapes should report `Object` from `.constructor`, not the
/// synthetic class itself, so date-fns's `new value.constructor(...)`,
/// drizzle's `value.constructor === Object` duck checks, and the standard
/// `({}).constructor === Object` semantics all match Node. The HIR
/// lowering registers each anon shape's id here at module init.
pub static ANON_SHAPE_CLASS_IDS: RwLock<Option<std::collections::HashSet<u32>>> = RwLock::new(None);

/// Mark `class_id` as a synthetic anon-shape class so `.constructor`
/// reads on instances of that class return the global `Object`
/// constructor rather than the synthetic class ref.
#[no_mangle]
pub unsafe extern "C" fn js_register_anon_shape_class_id(class_id: u32) {
    if class_id == 0 {
        return;
    }
    let mut guard = ANON_SHAPE_CLASS_IDS.write().unwrap();
    if guard.is_none() {
        *guard = Some(std::collections::HashSet::new());
    }
    guard.as_mut().unwrap().insert(class_id);
}

/// True if `class_id` was registered via `js_register_anon_shape_class_id`.
pub fn is_anon_shape_class_id(class_id: u32) -> bool {
    if class_id == 0 {
        return false;
    }
    if let Ok(guard) = ANON_SHAPE_CLASS_IDS.read() {
        if let Some(set) = guard.as_ref() {
            return set.contains(&class_id);
        }
    }
    false
}
