//! keys/values/entries + for-in enumeration.
//! Pure relocation out of field_get_set.rs (issue #1103 split).

use super::*;

/// Map/Set receivers: the collection's DATA lives in internal slots (never
/// own enumerable properties — Node: `Object.keys(new Map([...])) === []`),
/// but user EXPANDOS (`cache.custom = x`) live in the exotic side table
/// (`ExoticKind::Map`/`Set`). Shared by the keys/values/entries guards.
enum MapSetEnum {
    Keys,
    Values,
    Entries,
}

fn map_set_exotic_enum(stripped: *const ObjectHeader, what: MapSetEnum) -> *mut ArrayHeader {
    let addr = stripped as usize;
    let kind = if crate::map::is_registered_map(addr) {
        super::super::exotic_expando::ExoticKind::Map
    } else {
        super::super::exotic_expando::ExoticKind::Set
    };
    let keys = super::super::exotic_expando::exotic_own_keys(kind, addr, true);
    let arr = crate::array::js_array_alloc(keys.len().max(1) as u32);
    let mut out = arr;
    let receiver = f64::from_bits(JSValue::pointer(addr as *const u8).bits());
    for name in keys {
        let value = || unsafe {
            super::super::exotic_expando::exotic_get_own_property(addr, kind, &name, receiver)
                .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED))
        };
        match what {
            MapSetEnum::Keys => {
                let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                out = crate::array::js_array_push(out, JSValue::string_ptr(key));
            }
            MapSetEnum::Values => {
                out = crate::array::js_array_push_f64(out, value());
            }
            MapSetEnum::Entries => {
                let pair = crate::array::js_array_alloc(2);
                let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                crate::array::js_array_push(pair, JSValue::string_ptr(key));
                crate::array::js_array_push_f64(pair, value());
                out = crate::array::js_array_push(
                    out,
                    JSValue::from_bits(JSValue::pointer(pair as *const u8).bits()),
                );
            }
        }
    }
    out
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
        super::super::has_own_helpers::throw_to_object_nullish_type_error();
    }
    // A Proxy is a small registered id — route through the `ownKeys` trap +
    // enumerability filter rather than the handle-dispatch fallback below.
    if crate::proxy::js_proxy_is_proxy(value) != 0 {
        let arr = crate::proxy::proxy_enum_own_keys(value);
        return (arr.to_bits() & crate::value::POINTER_MASK) as *mut ArrayHeader;
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
                    // The wrapper's character indices are installed as REAL
                    // own fields at construction (install_string_wrapper_
                    // indices), so they come back from `js_object_keys` too —
                    // skip them here or `Object.keys(Object("abc"))` lists
                    // every index twice. Only canonical indices below the
                    // string length are virtual; expando keys pass through.
                    let key_ptr =
                        (key_val.bits() & crate::value::POINTER_MASK) as *const crate::StringHeader;
                    if let Some(name) =
                        unsafe { super::super::has_own_helpers::str_from_string_header(key_ptr) }
                    {
                        if let Ok(idx) = name.parse::<u32>() {
                            if idx.to_string() == name && (idx as usize) < len as usize {
                                continue;
                            }
                        }
                    }
                    crate::array::js_array_push_f64(arr, f64::from_bits(key_val.bits()));
                }
            }
            return arr;
        }
    }
    if let Some(addr) = crate::typedarray_props::typed_array_addr_from_value(value) {
        return unsafe {
            crate::typedarray_props::typed_array_own_property_names(
                addr as *const crate::typedarray::TypedArrayHeader,
                true,
            )
        };
    }
    // A class constructor ref `C` is an INT32-tagged value (not a pointer), so it
    // would otherwise fall through to the empty-array tail below. Its enumerable
    // own keys are the static fields registered in CLASS_DYNAMIC_PROPS — built-in
    // `length`/`name`/`prototype` and static methods are non-enumerable. Backs
    // `Object.keys(C)` / `for (k in C)` (test262 class/elements static-field-*).
    if let Some(class_id) = super::super::class_ref_id(value) {
        if super::super::class_prototype_ref_id(value).is_none() {
            let mut names =
                super::super::class_registry::class_own_enumerable_field_names(class_id);
            super::super::descriptors::sort_property_names_ecma(&mut names);
            let arr = crate::array::js_array_alloc(names.len().max(1) as u32);
            let mut out = arr;
            for name in names {
                let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                out = crate::array::js_array_push(out, JSValue::string_ptr(key));
            }
            return out;
        }
    }
    if jv.is_pointer() {
        let ptr = jv.as_pointer::<u8>() as usize;
        // A POINTER_TAG registry handle (zlib stream, fetch Request/Response/
        // Headers/Blob, …) is not an address. It exposes no own enumerable
        // properties — its surface lives on the prototype as accessors — so
        // return empty instead of dereferencing unmapped low memory.
        if crate::value::addr_class::is_handle_band(ptr) {
            return crate::array::js_array_alloc(0);
        }
        if crate::value::addr_class::is_small_handle(ptr) {
            if let Some(dispatch) =
                super::super::class_registry::handle_own_property_names_dispatch()
            {
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
        if crate::typedarray::lookup_typed_array_kind(ptr).is_some() {
            return unsafe {
                crate::typedarray_props::typed_array_own_property_names(
                    ptr as *const crate::typedarray::TypedArrayHeader,
                    true,
                )
            };
        }
        if crate::closure::is_closure_ptr(ptr) {
            return js_closure_dynamic_keys(ptr);
        }
        // Date / RegExp / Error exotic instances: enumerable own expando
        // keys from the side tables (the cell is not an `ObjectHeader`).
        if let Some(kind) = super::super::exotic_expando::exotic_expando_kind(ptr) {
            let keys = super::super::exotic_expando::exotic_own_keys(kind, ptr, true);
            let arr = crate::array::js_array_alloc(keys.len().max(1) as u32);
            let mut out = arr;
            for name in keys {
                let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                out = crate::array::js_array_push(out, JSValue::string_ptr(key));
            }
            return out;
        }
        return js_object_keys(ptr as *const ObjectHeader);
    }
    crate::array::js_array_alloc(0)
}

/// `for (key in value)` enumeration key set. Differs from
/// [`js_object_keys_value`] (which backs `Object.keys`) in two ways
/// mandated by ECMA-262 §14.7.5 / EnumerateObjectProperties:
///
///   * null / undefined enumerate NOTHING and must NOT throw — `Object.keys`
///     throws `TypeError`, but `for (k in undefined) {}` is a no-op
///     (language/statements/for-in/S12.6.4_A1, A2).
///   * inherited enumerable string-keyed properties on the prototype chain
///     are visited too, with shadowed/duplicate names emitted only once
///     (S12.6.4_A6 / A6.1 — `FACTORY.prototype = {feat,hint}`).
///
/// Enumerable own keys at each level come from `js_object_keys_value` so every
/// existing tag-dispatch case (arrays → index keys, strings → index keys, typed
/// arrays, proxies, plain objects, class instances) is reused unchanged. Class /
/// built-in prototype methods are non-enumerable, so they are correctly skipped.
///
/// Shadowing follows the spec exactly: a name that appears as an OWN property at
/// a closer level — even a non-enumerable one — hides the same name on the rest
/// of the chain (language/statements/for-in/12.6.4-2). So at each level we mark
/// ALL own property names (`js_object_get_own_property_names`, incl
/// non-enumerable) as "seen" after emitting that level's enumerable subset.
#[no_mangle]
pub extern "C" fn js_for_in_keys_value(value: f64) -> *mut ArrayHeader {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_null() || jv.is_undefined() {
        return crate::array::js_array_alloc(0);
    }
    let mut out = crate::array::js_array_alloc(8);
    // Non-pointer primitives (number/boolean, boxed string) have only their own
    // enumerable keys; every prototype property they inherit is non-enumerable.
    if !jv.is_pointer() {
        let own = js_object_keys_value(value);
        let n = crate::array::js_array_length(own);
        for i in 0..n {
            let kv = crate::array::js_array_get(own, i);
            out = crate::array::js_array_push_f64(out, f64::from_bits(kv.bits()));
        }
        return out;
    }
    let key_string = |kv: JSValue, scratch: &mut [u8; crate::value::SHORT_STRING_MAX_LEN]| {
        unsafe { crate::string::js_string_key_bytes(kv, scratch) }
            .and_then(|b| std::str::from_utf8(b).ok().map(|s| s.to_string()))
    };
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let mut current = value;
    // Depth cap guards against pathological / cyclic prototype graphs.
    for _ in 0..1000 {
        let cv = JSValue::from_bits(current.to_bits());
        if cv.is_null() || cv.is_undefined() || !cv.is_pointer() {
            break;
        }
        // Emit this level's enumerable own keys (OrdinaryOwnPropertyKeys order),
        // skipping any name already shadowed by a closer level.
        let enum_arr = js_object_keys_value(current);
        let en = crate::array::js_array_length(enum_arr);
        for i in 0..en {
            let kv = crate::array::js_array_get(enum_arr, i);
            let name = match key_string(kv, &mut scratch) {
                Some(s) => s,
                None => continue,
            };
            if seen.insert(name) {
                out = crate::array::js_array_push_f64(out, f64::from_bits(kv.bits()));
            }
        }
        // Mark ALL own names (incl non-enumerable) seen so they shadow the
        // remainder of the chain.
        let all_f64 = super::super::descriptors::js_object_get_own_property_names(current);
        let all_arr = (all_f64.to_bits() & crate::value::POINTER_MASK) as *mut ArrayHeader;
        if !all_arr.is_null() {
            let an = crate::array::js_array_length(all_arr);
            for i in 0..an {
                let kv = crate::array::js_array_get(all_arr, i);
                if let Some(name) = key_string(kv, &mut scratch) {
                    seen.insert(name);
                }
            }
        }
        current = super::super::object_ops::js_object_get_prototype_of(current);
    }
    out
}

fn closure_dynamic_enumerable_props(ptr: usize) -> Vec<(String, f64)> {
    let mut props: Vec<(String, f64)> = Vec::new();

    // Built-in function properties `length` and `name` are non-enumerable by
    // default. If the caller redefined them via `Object.defineProperty` with
    // `enumerable: true`, include them here BEFORE user-added dynamic props
    // so their relative order matches the spec insertion order (built-ins
    // precede dynamically-added own properties).
    for builtin_key in &["length", "name"] {
        if crate::closure::closure_is_key_deleted(ptr, builtin_key) {
            continue;
        }
        // Only include if the side table explicitly marks them enumerable.
        // Default (no entry in descriptor side table) = non-enumerable for
        // built-in function properties.
        if !get_property_attrs(ptr, builtin_key)
            .map(|attrs| attrs.enumerable())
            .unwrap_or(false)
        {
            continue;
        }
        // Value: prefer a side-table override written by defineProperty, then
        // fall back to the built-in computed value so Object.keys / entries
        // returns the right thing even when defineProperty only changed attrs.
        // Use `closure_has_own_dynamic_prop` to distinguish "has an explicit
        // dynamic value (possibly undefined)" from "no override" — using
        // `closure_get_dynamic_prop` as a sentinel conflates both cases and
        // also invokes getters, which is wrong for the keys-only path.
        let value = if crate::closure::closure_has_own_dynamic_prop(ptr, builtin_key) {
            f64::from_bits(crate::closure::closure_get_dynamic_prop(ptr, builtin_key).to_bits())
        } else if *builtin_key == "length" {
            let closure_value = crate::value::js_nanbox_pointer(ptr as i64);
            let len = unsafe {
                super::super::native_module::bound_native_callable_value_arity(closure_value)
            }
            .map(|a| a as f64)
            .or_else(|| super::super::native_module::builtin_closure_length(ptr).map(|l| l as f64))
            .or_else(|| {
                crate::closure::closure_length(ptr as *const crate::closure::ClosureHeader)
                    .map(|l| l as f64)
            })
            .unwrap_or(0.0);
            len
        } else {
            // "name"
            let func_ptr =
                unsafe { (*(ptr as *const crate::closure::ClosureHeader)).func_ptr as usize };
            let fname = crate::builtins::function_name_for_ptr(func_ptr).unwrap_or_default();
            let s = crate::string::js_string_from_bytes(fname.as_ptr(), fname.len() as u32);
            f64::from_bits(JSValue::string_ptr(s).bits())
        };
        props.push((builtin_key.to_string(), value));
    }

    // User-added dynamic props (skip "length"/"name" — handled above so we
    // don't double-count if defineProperty also wrote a value to dynamic props).
    let user_props = crate::closure::closure_dynamic_props_snapshot(ptr)
        .into_iter()
        .filter(|(name, _)| {
            if matches!(name.as_str(), "length" | "name") {
                return false;
            }
            get_property_attrs(ptr, name)
                .map(|attrs| attrs.enumerable())
                .unwrap_or(true)
        })
        .collect::<Vec<_>>();
    props.extend(user_props);

    for name in super::super::accessor_descriptor_keys_for_obj(ptr) {
        if props.iter().any(|(existing, _)| existing == &name) {
            continue;
        }
        if crate::closure::closure_is_key_deleted(ptr, &name) {
            continue;
        }
        if matches!(name.as_str(), "length" | "name") {
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

/// `Object.values` / `Object.entries` over a revocable Proxy: enumerate the
/// own keys through the `ownKeys` trap (same source `Object.keys` uses), then
/// read each value back through the `get` trap. Without this the proxy id — a
/// handle-band payload, not an address — either got dereferenced (SIGSEGV) or,
/// once the handle-band guard rejected it, silently reported no properties.
unsafe fn proxy_values_or_entries(value: f64, want_pairs: bool) -> *mut ArrayHeader {
    let keys_boxed = crate::proxy::proxy_enum_own_keys(value);
    let keys_arr = (keys_boxed.to_bits() & crate::value::POINTER_MASK) as *mut ArrayHeader;
    let len = crate::array::js_array_length(keys_arr);
    let mut out = crate::array::js_array_alloc(len.max(1) as u32);
    for i in 0..len {
        let key = crate::array::js_array_get(keys_arr, i);
        let val = crate::proxy::js_proxy_get(value, f64::from_bits(key.bits()));
        if want_pairs {
            let pair = crate::array::js_array_alloc(2);
            let pair = crate::array::js_array_push(pair, key);
            let pair = crate::array::js_array_push_f64(pair, val);
            out = crate::array::js_array_push(out, JSValue::array_ptr(pair));
        } else {
            out = crate::array::js_array_push_f64(out, val);
        }
    }
    out
}

/// Tag-dispatching `Object.values(value)` — see [`js_object_keys_value`].
/// A string yields its characters (`Object.values("hi") === ["h","i"]`);
/// objects/arrays delegate to `js_object_values`; primitives yield `[]`.
#[no_mangle]
pub extern "C" fn js_object_values_value(value: f64) -> *mut ArrayHeader {
    let jv = JSValue::from_bits(value.to_bits());
    if crate::proxy::js_proxy_is_proxy(value) != 0 {
        return unsafe {
            proxy_values_or_entries(value, /*want_pairs=*/ false)
        };
    }
    // #2818: ToObject(null/undefined) throws TypeError, matching Node.
    if jv.is_null() || jv.is_undefined() {
        super::super::has_own_helpers::throw_to_object_nullish_type_error();
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
    if let Some(addr) = crate::typedarray_props::typed_array_addr_from_value(value) {
        return unsafe {
            crate::typedarray_props::typed_array_own_enumerable_values(
                addr as *const crate::typedarray::TypedArrayHeader,
            )
        };
    }
    if jv.is_pointer() {
        let ptr = jv.as_pointer::<u8>() as usize;
        // A POINTER_TAG registry handle (zlib stream, fetch Request/Response/
        // Headers/Blob, …) is not an address. It exposes no own enumerable
        // properties — its surface lives on the prototype as accessors — so
        // return empty instead of dereferencing unmapped low memory.
        if crate::value::addr_class::is_handle_band(ptr) {
            return crate::array::js_array_alloc(0);
        }
        if crate::typedarray::lookup_typed_array_kind(ptr).is_some() {
            return unsafe {
                crate::typedarray_props::typed_array_own_enumerable_values(
                    ptr as *const crate::typedarray::TypedArrayHeader,
                )
            };
        }
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
    if crate::proxy::js_proxy_is_proxy(value) != 0 {
        return unsafe {
            proxy_values_or_entries(value, /*want_pairs=*/ true)
        };
    }
    // #2818: ToObject(null/undefined) throws TypeError, matching Node.
    if jv.is_null() || jv.is_undefined() {
        super::super::has_own_helpers::throw_to_object_nullish_type_error();
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
    if let Some(addr) = crate::typedarray_props::typed_array_addr_from_value(value) {
        return unsafe {
            crate::typedarray_props::typed_array_own_enumerable_entries(
                addr as *const crate::typedarray::TypedArrayHeader,
            )
        };
    }
    if jv.is_pointer() {
        let ptr = jv.as_pointer::<u8>() as usize;
        // A POINTER_TAG registry handle (zlib stream, fetch Request/Response/
        // Headers/Blob, …) is not an address. It exposes no own enumerable
        // properties — its surface lives on the prototype as accessors — so
        // return empty instead of dereferencing unmapped low memory.
        if crate::value::addr_class::is_handle_band(ptr) {
            return crate::array::js_array_alloc(0);
        }
        if crate::typedarray::lookup_typed_array_kind(ptr).is_some() {
            return unsafe {
                crate::typedarray_props::typed_array_own_enumerable_entries(
                    ptr as *const crate::typedarray::TypedArrayHeader,
                )
            };
        }
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
    // Hot on the JSON.stringify path — called once per serialized object
    // (#6009). Keys arrays are always materialized dense GC arrays, so read
    // the element slots raw instead of paying the exported `js_array_get`
    // validation per element, and reject on the first byte: a canonical
    // array index must start with an ASCII digit, which almost no object key
    // does, so the utf8 + numeric parse runs only for digit-leading keys.
    {
        let keys_addr = keys as usize;
        let aligned = (keys_addr as u64) >> 48 == 0 && keys_addr >= 0x10000 && keys_addr & 0x7 == 0;
        if aligned {
            let keys_gc =
                (keys as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            if (*keys_gc).obj_type == crate::gc::GC_TYPE_ARRAY && (*keys).length <= (*keys).capacity
            {
                let len = (*keys).length as usize;
                let elements =
                    (keys as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
                let mut sso_buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
                for i in 0..len {
                    let key_val = crate::JSValue::from_bits((*elements.add(i)).to_bits());
                    let Some(bytes) = crate::string::js_string_key_bytes(key_val, &mut sso_buf)
                    else {
                        continue;
                    };
                    if !bytes.first().is_some_and(|b| b.is_ascii_digit()) {
                        continue;
                    }
                    if std::str::from_utf8(bytes)
                        .ok()
                        .and_then(canonical_array_index)
                        .is_some()
                    {
                        return true;
                    }
                }
                return false;
            }
        }
    }
    // Fallback for anything that doesn't look like a plain dense keys array.
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
    // A Map/Set receiver is a MapHeader/SetHeader, NOT an ObjectHeader — the
    // generic object walk below reads collection-internal bytes as a
    // `keys_array` pointer and SIGSEGVs downstream (js_array_length's GC-kind
    // probe on the garbage pointer). Per spec a collection's entries live in
    // internal slots, not own enumerable properties: Node returns [] for
    // `Object.keys(new Map([...]))` — and likewise for values/entries/for-in.
    // A telemetry path in a large esbuild-bundled CLI app hit this via
    // `Object.keys(cache)` on a lodash-memoize Map cache.
    if crate::map::is_registered_map(stripped as usize)
        || crate::set::is_registered_set(stripped as usize)
    {
        return map_set_exotic_enum(stripped, MapSetEnum::Keys);
    }
    if let Some(addr) =
        crate::typedarray_props::typed_array_addr_from_value(f64::from_bits(obj as u64))
    {
        return unsafe {
            crate::typedarray_props::typed_array_own_property_names(
                addr as *const crate::typedarray::TypedArrayHeader,
                true,
            )
        };
    }
    if crate::typedarray::lookup_typed_array_kind(stripped as usize).is_some() {
        return unsafe {
            crate::typedarray_props::typed_array_own_property_names(
                stripped as *const crate::typedarray::TypedArrayHeader,
                true,
            )
        };
    }
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
                // Issue #233: a grown array installs a forwarding pointer at the
                // old location; a binding written before the grow still holds it.
                // Resolve the chain so we read the live header (without this,
                // `Object.keys(a)` after `a.length = N` saw a forwarding header
                // and returned []).
                let arr = crate::array::clean_arr_ptr(stripped as *const crate::array::ArrayHeader);
                let length = (*arr).length;
                if length > 100_000 {
                    let names = crate::array::array_named_property_names(arr, true);
                    let dense_limit = if length > (*arr).capacity && (*arr).capacity <= 1_000_000 {
                        (*arr).capacity
                    } else {
                        0
                    };
                    let result = crate::array::js_array_alloc(
                        dense_limit.saturating_add(names.len() as u32),
                    );
                    if dense_limit > 0 {
                        let elements = (arr as *const u8)
                            .add(std::mem::size_of::<crate::array::ArrayHeader>())
                            as *const u64;
                        for i in 0..dense_limit {
                            if std::ptr::read(elements.add(i as usize)) == crate::value::TAG_HOLE {
                                continue;
                            }
                            let s = i.to_string();
                            let key_box =
                                crate::string::js_string_new_sso(s.as_ptr(), s.len() as u32);
                            crate::array::js_array_push_f64(result, key_box);
                        }
                    }
                    for name in names {
                        let key =
                            crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                        crate::array::js_array_push(result, JSValue::string_ptr(key));
                    }
                    return result;
                }
                let elements = (arr as *const u8)
                    .add(std::mem::size_of::<crate::array::ArrayHeader>())
                    as *const u64;
                // Index properties may carry a non-default descriptor
                // (`Object.defineProperty(arr, i, { enumerable: false })`).
                // Object.keys / for-in must skip non-enumerable indices — but
                // the per-index side-table lookup is only needed when this array
                // actually has descriptor entries, so the common all-default
                // array stays on the fast path.
                let owner = stripped as usize;
                let has_idx_descriptors =
                    PROPERTY_DESCRIPTORS.with(|m| m.borrow().keys().any(|(ptr, _)| *ptr == owner));
                let result = crate::array::js_array_alloc(length);
                for i in 0..length {
                    if std::ptr::read(elements.add(i as usize)) == crate::value::TAG_HOLE {
                        continue;
                    }
                    // Format `i` as decimal into a stack buffer; SSO covers
                    // 0..=99999 (≤5 bytes), and a length-100k array hits the
                    // sanity-cap above so we never need a heap StringHeader.
                    let s = i.to_string();
                    if has_idx_descriptors {
                        if let Some(attrs) = get_property_attrs(owner, &s) {
                            if !attrs.enumerable() {
                                continue;
                            }
                        }
                    }
                    let key_box = crate::string::js_string_new_sso(s.as_ptr(), s.len() as u32);
                    crate::array::js_array_push_f64(result, key_box);
                }
                let named = crate::array::array_named_property_names(arr, true);
                for name in &named {
                    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                    crate::array::js_array_push(result, JSValue::string_ptr(key));
                }
                // Accessor-only named properties (defineProperty {get/set})
                // live solely in the accessor side table — include the
                // enumerable ones.
                if super::super::descriptors_in_use() {
                    for name in accessor_descriptor_keys_for_obj(owner) {
                        if super::super::canonical_array_index(&name).is_some()
                            || named.contains(&name)
                            || !get_property_attrs(owner, &name)
                                .map(|a| a.enumerable())
                                .unwrap_or(false)
                        {
                            continue;
                        }
                        let key =
                            crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                        crate::array::js_array_push(result, JSValue::string_ptr(key));
                    }
                }
                return result;
            }
        }
    }
    unsafe {
        if (*obj).class_id == NATIVE_MODULE_CLASS_ID {
            // Relocated to native_module.rs::vt_own_keys_array so the
            // module key tables are reachable only through the vtable
            // (linker-strippable when no namespace object exists).
            if let Some(vt) = super::super::native_module::native_module_vtable() {
                if let Some(out) = (vt.own_keys_array)(obj) {
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
        // Private elements (`#x`) are stored in a class instance's keys_array
        // but are never enumerable/reflectable properties. Take the filtering
        // path for class instances (class_id != 0) so they are dropped. Plain
        // object literals keep class_id 0, so `{"#fff": 1}` stays visible.
        let hide_private = (*obj).class_id != 0;
        if !has_descriptors && !hide_private {
            let out = crate::array::js_array_alloc(len as u32);
            for j in 0..len {
                let key_val = crate::array::js_array_get(keys, pos(j));
                crate::array::js_array_push_f64(out, f64::from_bits(key_val.bits()));
            }
            return out;
        }
        // Slow path: filter out non-enumerable and private (`#`) keys.
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
            if hide_private && (key_str.starts_with('#') || is_internal_runtime_key(key_str)) {
                continue;
            }
            // If a descriptor explicitly marks this key non-enumerable, skip it.
            if has_descriptors {
                if let Some(attrs) = get_property_attrs(obj as usize, key_str) {
                    if !attrs.enumerable() {
                        continue;
                    }
                }
            }
            crate::array::js_array_push_f64(filtered, f64::from_bits(key_val.bits()));
        }
        filtered
    }
}

/// Get the values of an object as an array
/// True when `obj` is a class instance (`class_id != 0`) and `key_val` names a
/// private element (`#x`). Private elements physically live in the instance
/// keys_array but are never enumerable/reflectable properties. Plain object
/// literals keep `class_id == 0`, so `{"#fff": 1}` stays visible.
pub(crate) unsafe fn instance_private_key_hidden(
    obj: *const ObjectHeader,
    key_val: crate::JSValue,
) -> bool {
    if obj.is_null() || (*obj).class_id == 0 {
        return false;
    }
    let mut buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    crate::string::js_string_key_bytes(key_val, &mut buf)
        .map(|b| b.first() == Some(&b'#') || is_internal_runtime_key_bytes(b))
        .unwrap_or(false)
}

/// True for perry's hidden runtime-internal own keys — the
/// `__perry_collection_backing__` field stashed on a `class … extends Map/Set`
/// instance, and the `__perry_wk_entries` field backing a `WeakMap`/`WeakSet`
/// (#6120). These physically live in the object's keys_array but must NEVER
/// surface to `Object.keys` / `for…in` / `Object.getOwnPropertyNames` /
/// `JSON.stringify` / `Object.hasOwn` / `hasOwnProperty` / `propertyIsEnumerable`.
///
/// Matches each key EXACTLY (an allowlist), not a broad `__perry_*` prefix — a
/// prefix test would wrongly hide legitimate user properties whose name happens
/// to begin with `__perry_` (e.g. `this.__perry_user = 1`).
#[inline]
pub(crate) fn is_internal_runtime_key_bytes(b: &[u8]) -> bool {
    b == crate::object::map_set_subclass::BACKING_KEY || b == crate::weakref::WEAK_ENTRIES_KEY
}

/// `&str` form of [`is_internal_runtime_key_bytes`].
#[inline]
pub(crate) fn is_internal_runtime_key(s: &str) -> bool {
    is_internal_runtime_key_bytes(s.as_bytes())
}

/// True when a per-property descriptor marks `key_val`'s name non-enumerable
/// (`Object.defineProperty(o, k, { enumerable: false })`). Mirrors the
/// slow-path filter in `js_object_keys` so `Object.values`/`Object.entries`
/// agree with `Object.keys` (#5046). Callers gate on a cheap "does this object
/// have any descriptors at all" probe so the common descriptor-free object
/// never pays the string extraction.
pub(crate) unsafe fn descriptor_marks_non_enumerable(
    obj: *const ObjectHeader,
    key_val: crate::JSValue,
) -> bool {
    let mut buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let bytes = match crate::string::js_string_key_bytes(key_val, &mut buf) {
        Some(b) => b,
        None => return false,
    };
    let key_str = match std::str::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => return false,
    };
    get_property_attrs(obj as usize, key_str)
        .map(|attrs| !attrs.enumerable())
        .unwrap_or(false)
}

/// Returns an array of the object's field values
#[no_mangle]
pub extern "C" fn js_object_values(obj: *const ObjectHeader) -> *mut ArrayHeader {
    let stripped = {
        let bits = obj as u64;
        let top16 = bits >> 48;
        if top16 == 0x7FFD || top16 >= 0x7FF8 {
            (bits & 0x0000_FFFF_FFFF_FFFF) as *const ObjectHeader
        } else {
            obj
        }
    };
    // Map/Set receiver → no own enumerable properties; see the matching
    // guard in `js_object_keys` for the rationale.
    if crate::map::is_registered_map(stripped as usize)
        || crate::set::is_registered_set(stripped as usize)
    {
        return map_set_exotic_enum(stripped, MapSetEnum::Values);
    }
    if let Some(addr) =
        crate::typedarray_props::typed_array_addr_from_value(f64::from_bits(obj as u64))
    {
        return unsafe {
            crate::typedarray_props::typed_array_own_enumerable_values(
                addr as *const crate::typedarray::TypedArrayHeader,
            )
        };
    }
    if crate::typedarray::lookup_typed_array_kind(stripped as usize).is_some() {
        return unsafe {
            crate::typedarray_props::typed_array_own_enumerable_values(
                stripped as *const crate::typedarray::TypedArrayHeader,
            )
        };
    }
    // Arrays: emit each present (non-hole) element value, then enumerable named
    // properties. `js_object_values` has no `ArrayHeader` layout, so the generic
    // object path below would read an array's body as object fields and crash;
    // handle arrays explicitly (mirrors the `js_object_keys` array branch).
    if !stripped.is_null() && (stripped as usize) >= crate::gc::GC_HEADER_SIZE + 0x1000 {
        unsafe {
            let gc_header = (stripped as *const u8).sub(crate::gc::GC_HEADER_SIZE)
                as *const crate::gc::GcHeader;
            if (*gc_header).obj_type == crate::gc::GC_TYPE_ARRAY {
                let arr = crate::array::clean_arr_ptr(stripped as *const crate::array::ArrayHeader);
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
                    let v = crate::array::js_array_get(arr, i);
                    crate::array::js_array_push_f64(result, f64::from_bits(v.bits()));
                }
                for name in crate::array::array_named_property_names(arr, true) {
                    if let Some(v) = crate::array::array_named_property_get_by_name(arr, &name) {
                        crate::array::js_array_push_f64(result, v);
                    }
                }
                return result;
            }
        }
    }
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
        // Snapshot the own key list before reading values, then read each
        // through the name-keyed `[[Get]]` so own accessors fire and getter side
        // effects don't perturb the key set (mirrors `js_object_entries`).
        //
        // Two correctness requirements drive this shape:
        //   * GC safety — a getter fired by `js_object_get_field_by_name` can
        //     delete a future key and allocate/GC before we visit it. A key kept
        //     only as a NaN-boxed pointer inside this Rust-heap `Vec` is not a
        //     stack-visible GC root, so it could dangle. We snapshot the owned
        //     key *bytes* and rematerialize the string at read time instead.
        //   * EnumerableOwnProperties — enumerability is determined per key at
        //     read time, not cached up front: an earlier getter can create a
        //     descriptor or flip a future key's enumerability, so we defer the
        //     `descriptor_marks_non_enumerable` check to the read phase.
        let mut snapshot_keys: Vec<Vec<u8>> = Vec::with_capacity(count);
        let mut key_buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        for j in 0..count {
            let i = pos(j);
            if keys.is_null() || i >= crate::array::js_array_length(keys) {
                continue;
            }
            let key_val = crate::array::js_array_get(keys, i);
            if instance_private_key_hidden(obj, key_val) {
                continue;
            }
            if let Some(bytes) = crate::string::js_string_key_bytes(key_val, &mut key_buf) {
                snapshot_keys.push(bytes.to_vec());
            }
        }
        for key_bytes in snapshot_keys {
            let key_str =
                crate::string::js_string_from_bytes(key_bytes.as_ptr(), key_bytes.len() as u32);
            if key_str.is_null() {
                continue;
            }
            // Re-check own + enumerable at read time (a prior getter may have
            // removed/hidden the key, or created a descriptor) — see
            // `js_object_entries`.
            if !super::super::own_key_present(obj as *mut ObjectHeader, key_str) {
                continue;
            }
            if descriptor_marks_non_enumerable(obj, JSValue::string_ptr(key_str)) {
                continue;
            }
            let value = js_object_get_field_by_name(obj as *const ObjectHeader, key_str);
            crate::array::js_array_push_f64(result, f64::from_bits(value.bits()));
        }

        result
    }
}

/// Get the entries of an object as an array of [key, value] pairs
/// Returns an array where each element is a 2-element array [key, value]
#[no_mangle]
pub extern "C" fn js_object_entries(obj: *const ObjectHeader) -> *mut ArrayHeader {
    let stripped = {
        let bits = obj as u64;
        let top16 = bits >> 48;
        if top16 == 0x7FFD || top16 >= 0x7FF8 {
            (bits & 0x0000_FFFF_FFFF_FFFF) as *const ObjectHeader
        } else {
            obj
        }
    };
    // Map/Set receiver → no own enumerable properties; see the matching
    // guard in `js_object_keys` for the rationale.
    if crate::map::is_registered_map(stripped as usize)
        || crate::set::is_registered_set(stripped as usize)
    {
        return map_set_exotic_enum(stripped, MapSetEnum::Entries);
    }
    if let Some(addr) =
        crate::typedarray_props::typed_array_addr_from_value(f64::from_bits(obj as u64))
    {
        return unsafe {
            crate::typedarray_props::typed_array_own_enumerable_entries(
                addr as *const crate::typedarray::TypedArrayHeader,
            )
        };
    }
    if crate::typedarray::lookup_typed_array_kind(stripped as usize).is_some() {
        return unsafe {
            crate::typedarray_props::typed_array_own_enumerable_entries(
                stripped as *const crate::typedarray::TypedArrayHeader,
            )
        };
    }
    // Arrays: emit [index, value] pairs for present elements, then named props.
    // `js_object_entries` has no `ArrayHeader` layout, so the generic object
    // path below would read an array's body as object fields and crash; handle
    // arrays explicitly (mirrors the `js_object_keys` / `js_object_values`
    // array branches).
    if !stripped.is_null() && (stripped as usize) >= crate::gc::GC_HEADER_SIZE + 0x1000 {
        unsafe {
            let gc_header = (stripped as *const u8).sub(crate::gc::GC_HEADER_SIZE)
                as *const crate::gc::GcHeader;
            if (*gc_header).obj_type == crate::gc::GC_TYPE_ARRAY {
                let arr = crate::array::clean_arr_ptr(stripped as *const crate::array::ArrayHeader);
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
                    let pair = crate::array::js_array_alloc(2);
                    let s = i.to_string();
                    let key_box = crate::string::js_string_new_sso(s.as_ptr(), s.len() as u32);
                    crate::array::js_array_push_f64(pair, key_box);
                    let v = crate::array::js_array_get(arr, i);
                    crate::array::js_array_push_f64(pair, f64::from_bits(v.bits()));
                    crate::array::js_array_push_f64(
                        result,
                        crate::value::js_nanbox_pointer(pair as i64),
                    );
                }
                for name in crate::array::array_named_property_names(arr, true) {
                    if let Some(v) = crate::array::array_named_property_get_by_name(arr, &name) {
                        let pair = crate::array::js_array_alloc(2);
                        let key =
                            crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                        crate::array::js_array_push(pair, JSValue::string_ptr(key));
                        crate::array::js_array_push_f64(pair, v);
                        crate::array::js_array_push_f64(
                            result,
                            crate::value::js_nanbox_pointer(pair as i64),
                        );
                    }
                }
                return result;
            }
        }
    }
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
        // Spec (EnumerableOwnProperties): the own key list is determined ONCE up
        // front, then `[[Get]]` is invoked per key. A getter that adds, removes,
        // or hides a future key during enumeration must not change the set of
        // entries reported (test262 entries/getter-adding-key,
        // getter-removing-future-key, getter-making-future-key-nonenumerable).
        //
        // Snapshot the own key *bytes* (not NaN-boxed pointers): a getter fired
        // by `js_object_get_field_by_name` can delete a future key and
        // allocate/GC before we visit it, and a key kept only inside this
        // Rust-heap `Vec` is not a stack-visible GC root — it could dangle.
        // Owning the bytes and rematerializing the string at read time sidesteps
        // that. Enumerability is likewise re-evaluated per key in the read phase
        // (an earlier getter can create a descriptor or flip a future key's
        // enumerability), so we deliberately do NOT filter it during the snapshot.
        let mut snapshot_keys: Vec<Vec<u8>> = Vec::with_capacity(count);
        let mut key_buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        for j in 0..count {
            let i = pos(j);
            if keys.is_null() || i >= crate::array::js_array_length(keys) {
                continue;
            }
            let key_val = crate::array::js_array_get(keys, i);
            if instance_private_key_hidden(obj, key_val) {
                continue;
            }
            if let Some(bytes) = crate::string::js_string_key_bytes(key_val, &mut key_buf) {
                snapshot_keys.push(bytes.to_vec());
            }
        }

        for key_bytes in snapshot_keys {
            let key_str =
                crate::string::js_string_from_bytes(key_bytes.as_ptr(), key_bytes.len() as u32);
            if key_str.is_null() {
                continue;
            }
            // Spec EnumerableOwnProperties re-reads `[[GetOwnProperty]]` per key
            // and skips it when the descriptor is now undefined or no longer
            // enumerable — a getter earlier in the loop may have deleted or
            // hidden a key that was in the initial snapshot (test262
            // entries/getter-removing-future-key, getter-making-future-key-
            // nonenumerable).
            if !super::super::own_key_present(obj as *mut ObjectHeader, key_str) {
                continue;
            }
            if descriptor_marks_non_enumerable(obj, JSValue::string_ptr(key_str)) {
                continue;
            }
            // Create a pair array [key, value].
            let pair = crate::array::js_array_alloc(2);
            crate::array::js_array_push_f64(
                pair,
                f64::from_bits(JSValue::string_ptr(key_str).bits()),
            );

            // Read the value through the name-keyed `[[Get]]`, which fires an
            // own accessor's getter (the raw index-based field read returned the
            // empty data slot for accessor-defined properties — test262
            // entries/getter-adding-key expected the getter's "B").
            let value = js_object_get_field_by_name(obj as *const ObjectHeader, key_str);
            crate::array::js_array_push_f64(pair, f64::from_bits(value.bits()));

            // Push the pair to result (NaN-box the array pointer)
            let pair_boxed = crate::value::js_nanbox_pointer(pair as i64);
            crate::array::js_array_push_f64(result, pair_boxed);
        }

        result
    }
}
