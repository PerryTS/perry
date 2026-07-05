//! push / pop / shift / unshift / set_length / delete + grow primitive.
use super::*;
use crate::arena::arena_alloc_gc;
use std::ptr;

/// `pop`/`shift`/`push`/`unshift` on a frozen array perform a `Set`/`Delete`
/// with `Throw = true` internally (ECMA-262 §23.1.3.*), so a non-writable
/// `length` / non-extensible receiver makes them throw a **TypeError** — they
/// must not silently no-op. Used by the frozen guards below.
#[cold]
fn throw_frozen_array_mutation() -> ! {
    crate::collection_iter::throw_type_error("Cannot mutate a frozen array");
}

/// `push`/`pop`/`shift`/`unshift` always perform `Set(O, "length", …, true)`
/// (ECMA-262 §23.1.3.*), so an array whose `length` was made non-writable via
/// `Object.defineProperty(arr, "length", { writable: false })` makes them throw
/// a **TypeError** — even when the call would otherwise be a no-op (empty array,
/// zero-arg). A *frozen* array is caught by `array_is_frozen` first (same throw);
/// this covers the non-writable-`length`-only case. (test262
/// Array.prototype.{push,pop,shift,unshift}/set-length-*-non-writable.)
#[inline]
pub(crate) fn array_length_is_non_writable(arr: *const ArrayHeader) -> bool {
    let flags = array_object_flags(arr);
    flags & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS != 0
        && crate::object::get_property_attrs(arr as usize, "length")
            .map(|a| !a.writable())
            .unwrap_or(false)
}

#[cold]
fn throw_non_writable_length() -> ! {
    crate::collection_iter::throw_type_error(
        "Cannot assign to read only property 'length' of object '[object Array]'",
    );
}

#[inline]
pub(crate) fn guard_writable_length(arr: *const ArrayHeader) {
    if array_length_is_non_writable(arr) {
        throw_non_writable_length();
    }
}

/// Guard called from the static `push_single`/`push` codegen path so that
/// frozen + non-writable-`length` checks fire even for `arr.push()` with no
/// arguments.  ECMA-262 §23.1.3.21 always performs `Set(O,"length",…,true)`.
#[no_mangle]
pub extern "C" fn js_array_push_guard(arr: *mut ArrayHeader) {
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return;
    }
    if array_is_frozen(arr) {
        throw_frozen_array_mutation();
    }
    guard_writable_length(arr);
}

#[no_mangle]
pub extern "C" fn js_array_grow(arr: *mut ArrayHeader, min_capacity: u32) -> *mut ArrayHeader {
    if arr.is_null() || (arr as usize) < 0x1000 {
        return js_array_alloc(min_capacity);
    }
    // Issue #233: resolve any existing forwarding chain before deciding
    // whether to grow — caller may pass a stale pre-grow pointer.
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return js_array_alloc(min_capacity);
    }
    if array_is_sealed_or_no_extend(arr) || array_is_frozen(arr) {
        return arr;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let arr_handle = scope.root_raw_mut_ptr(arr);
    unsafe {
        let old_capacity = (*arr).capacity;
        if min_capacity <= old_capacity {
            return arr;
        }

        // Double the capacity, or use min_capacity if larger
        let new_capacity = std::cmp::max(old_capacity * 2, min_capacity);
        let old_size = array_byte_size(old_capacity as usize);
        let new_size = array_byte_size(new_capacity as usize);

        // Allocate new from arena and copy old data.
        let new_ptr = arena_alloc_gc(new_size, 8, crate::gc::GC_TYPE_ARRAY) as *mut ArrayHeader;
        let arr = arr_handle.get_raw_mut_ptr::<ArrayHeader>();
        // GC_STORE_AUDIT(BARRIERED): array growth copy transfers layout and replays write barriers below.
        ptr::copy_nonoverlapping(arr as *const u8, new_ptr as *mut u8, old_size);

        (*new_ptr).capacity = new_capacity;
        let old_header =
            (arr as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
        let new_header =
            (new_ptr as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
        (*new_header)._reserved = (*old_header)._reserved;
        crate::gc::layout_transfer(arr as *mut u8, new_ptr as *mut u8);
        replay_array_growth_write_barriers(new_ptr);

        // Issue #233: install a forwarding pointer at the OLD location
        // so any stale reference (e.g. an async function's caller still
        // holding the pre-grow pointer in its parameter slot) resolves
        // to the new head via clean_arr_ptr's GC_FLAG_FORWARDED follow.
        // Uses the same forwarding-slot representation as GC evacuation:
        // first 8 bytes of payload (length+capacity) become the new user
        // ptr. Unlike GC-evacuation originals, array-growth stubs stay
        // retained because stale array references rely on clean_arr_ptr
        // following this chain.
        // Only valid for arena-allocated arrays (which have a GcHeader
        // 8 bytes before the user pointer); guard with a heap-bounds
        // check that mirrors clean_arr_ptr's HEAP_MIN to skip pointers
        // that don't have a real GcHeader behind them (e.g. test-mode
        // synthetic pointers, longlived-arena edge cases).
        // #1136: iOS family device allocates via libsystem_malloc in the
        // same low range as Android/Linux; mirror `clean_arr_ptr`'s
        // platform split so growth forwarding can install a stub for
        // arrays that live below 2 TB.
        #[cfg(any(
            target_os = "android",
            target_os = "linux",
            target_os = "windows",
            target_os = "ios",
            target_os = "tvos",
            target_os = "watchos",
            target_os = "visionos",
        ))]
        const HEAP_MIN: usize = 0x1000;
        #[cfg(not(any(
            target_os = "android",
            target_os = "linux",
            target_os = "windows",
            target_os = "ios",
            target_os = "tvos",
            target_os = "watchos",
            target_os = "visionos",
        )))]
        const HEAP_MIN: usize = 0x200_0000_0000;
        if (arr as usize) >= HEAP_MIN + crate::gc::GC_HEADER_SIZE {
            // Only forward arrays that came from the GC arena. A
            // non-array obj_type would mean something has gone wrong
            // upstream; bail out without forwarding rather than corrupt
            // an unrelated allocation's header.
            if (*old_header).obj_type == crate::gc::GC_TYPE_ARRAY {
                crate::gc::set_forwarding_address(old_header, new_ptr as *mut u8);
            }
        }

        new_ptr
    }
}

/// Push an element to the end of an array, growing if needed
/// #5135: read `Get(proxy, "length")` and ToLength-coerce it. Used by the
/// proxy-array push path so immer drafts (Proxies typed as arrays) mutate
/// through their traps instead of a native ArrayHeader deref.
unsafe fn proxy_array_length(proxy: f64) -> u64 {
    let key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
    let key_f64 = crate::value::js_nanbox_string(key as i64);
    let n = crate::builtins::js_number_coerce(crate::proxy::js_proxy_get(proxy, key_f64));
    if n.is_finite() && n >= 0.0 {
        n as u64
    } else {
        0
    }
}

/// #5135: `Set(proxy, <string key>, value)` through the proxy's `set` trap. The
/// key string is allocated fresh per call so an intervening GC can't leave a
/// stale interior pointer.
unsafe fn proxy_set_str_key(proxy: f64, key_bytes: &[u8], value: f64) {
    let key = crate::string::js_string_from_bytes(key_bytes.as_ptr(), key_bytes.len() as u32);
    let key_f64 = crate::value::js_nanbox_string(key as i64);
    crate::proxy::js_proxy_set(proxy, key_f64, value);
}

/// Returns a pointer to the (possibly reallocated) array
#[no_mangle]
pub extern "C" fn js_array_push_f64(arr: *mut ArrayHeader, value: f64) -> *mut ArrayHeader {
    // A uniquely-owned (refcount==1) string pushed into the array aliases an
    // element slot — demote it to shared so a later `s += x` on the source local
    // allocates fresh instead of mutating the stored element in place. No-op for
    // SSO / non-string. Done on the raw value, covering the inline, grow, and
    // proxy paths below (mirrors the object-field demote in
    // `runtime_store_jsvalue_slot`).
    crate::string::js_string_addref_if_heap_string(value);
    // #5135: a Proxy whose static type is an array (immer drafts) reaches here
    // with the masked proxy id. Perform the spec `Array.prototype.push` for a
    // single element directly through the proxy's `get`/`set` traps:
    //   len = ToLength(Get(P, "length")); Set(P, len, value); Set(P, "length", len+1)
    // Routing through the native push (`js_native_call_method`) would recurse
    // back here with the same proxy. Return `arr` unchanged so the codegen's
    // realloc write-back is a no-op (the proxy mutates its target in place).
    if let Some(proxy) = array_ptr_as_proxy(arr) {
        let len = unsafe { proxy_array_length(proxy) };
        unsafe {
            proxy_set_str_key(proxy, len.to_string().as_bytes(), value);
            proxy_set_str_key(proxy, b"length", (len as f64) + 1.0);
        }
        return arr;
    }
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return js_array_alloc(0);
    }
    if array_is_frozen(arr) {
        throw_frozen_array_mutation();
    }
    guard_writable_length(arr);
    if array_is_sealed_or_no_extend(arr) {
        return arr;
    }
    unsafe {
        let length = (*arr).length;
        let capacity = (*arr).capacity;

        if length >= capacity {
            return js_array_push_f64_grow(arr, length, value);
        }

        let value = canonicalize_array_numeric_store_value(arr, value);
        let value_bits = value.to_bits();
        let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        // GC_STORE_AUDIT(BARRIERED): push slot is immediately recorded via note_array_slot.
        ptr::write(elements_ptr.add(length as usize), value);
        note_array_slot(arr, length as usize, value_bits);
        (*arr).length = length + 1;
        arr
    }
}

#[no_mangle]
pub extern "C" fn js_array_push_hole(arr: *mut ArrayHeader) -> *mut ArrayHeader {
    js_array_push_f64(arr, f64::from_bits(crate::value::TAG_HOLE))
}

#[no_mangle]
pub extern "C" fn js_array_numeric_push_f64_unboxed(
    arr: *mut ArrayHeader,
    value: f64,
) -> *mut ArrayHeader {
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return js_array_alloc(0);
    }
    if array_is_sealed_or_no_extend(arr) || array_is_frozen(arr) {
        return arr;
    }
    guard_writable_length(arr);
    unsafe {
        if array_numeric_raw_f64_push_inbounds(arr, value) {
            return arr;
        }
    }
    js_array_push_f64(arr, value)
}

// This raw numeric-array helper is called from generated code, so release/LTO
// builds may otherwise internalize and strip the `#[no_mangle]` export.
#[used]
static KEEP_JS_ARRAY_NUMERIC_PUSH_F64_UNBOXED: extern "C" fn(
    *mut ArrayHeader,
    f64,
) -> *mut ArrayHeader = js_array_numeric_push_f64_unboxed;

#[cold]
unsafe fn js_array_push_f64_grow(
    arr: *mut ArrayHeader,
    length: u32,
    value: f64,
) -> *mut ArrayHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let arr_handle = scope.root_raw_mut_ptr(arr);
    let value_handle = scope.root_nanbox_f64(value);

    let arr = js_array_grow(arr_handle.get_raw_mut_ptr::<ArrayHeader>(), length + 1);
    let value = canonicalize_array_numeric_store_value(arr, value_handle.get_nanbox_f64());
    let value_bits = value.to_bits();

    let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
    // GC_STORE_AUDIT(BARRIERED): grown push slot is immediately recorded via note_array_slot.
    ptr::write(elements_ptr.add(length as usize), value);
    note_array_slot(arr, length as usize, value_bits);
    (*arr).length = length + 1;
    arr
}

/// Push every element of `source` to the end of `target`, growing as needed.
/// Returns a pointer to the (possibly reallocated) target. Refs #488
/// drizzle-sqlite: drizzle's `mergeQueries` does
/// `result.params.push(...query.params)` which the HIR lowers to
/// `NativeMethodCall { module: "array", method: "push_spread" }` —
/// pre-fix, codegen had no arm for `push_spread`, falling through to the
/// "Unknown native method" catch-all that lowered receiver+args for side
/// effects and returned the `0.0` sentinel. The push never happened and
/// SQL queries went out with 0 params, so INSERT silently inserted
/// nothing and SELECT returned `count=0`. This helper plus the
/// matching codegen arm in `lower_native_method_call` does the actual
/// push loop.
#[no_mangle]
pub extern "C" fn js_array_push_spread_f64(
    target: *mut ArrayHeader,
    source: *const ArrayHeader,
) -> *mut ArrayHeader {
    let source = clean_arr_ptr(source);
    if source.is_null() {
        return target;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let source_handle = scope.root_raw_const_ptr(source);
    unsafe {
        let src_len = (*source).length;
        if src_len == 0 {
            return target;
        }
        let mut current = target;
        for i in 0..src_len {
            let source = clean_arr_ptr(source_handle.get_raw_const_ptr::<ArrayHeader>());
            if source.is_null() {
                break;
            }
            let src_elements_ptr =
                (source as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
            let value = *src_elements_ptr.add(i as usize);
            current = js_array_push_f64(current, value);
        }
        current
    }
}

/// Pop an element from the end of an array.
/// Returns the removed element, or `undefined` if the array is empty (per
/// ECMAScript §23.1.3.21 — `Array.prototype.pop` on an empty array returns
/// undefined, NOT NaN). Pre-fix this returned `f64::NAN` (bare NaN bits,
/// which compare `!== undefined`); callers like `@perryts/mysql`'s pool
/// `acquire()` did `const entry = this.idle.shift(); if (entry !== undefined)`
/// and took the wrong branch on an empty pool. Issue #536.
#[no_mangle]
pub extern "C" fn js_array_pop_f64(arr: *mut ArrayHeader) -> f64 {
    const TAG_UNDEFINED_F64: f64 = f64::from_bits(0x7FFC_0000_0000_0001u64);
    // Borrowed array-like receiver (`obj.pop = Array.prototype.pop; obj.pop()`):
    // the thunk hands this dense helper the plain object pointer. Run the
    // spec-generic engine instead of reading the object as an `ArrayHeader`.
    if let Some(recv) = crate::array::plain_object_value(arr) {
        return crate::array::generic_object_pop(recv);
    }
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return TAG_UNDEFINED_F64;
    }
    if array_is_frozen(arr) {
        throw_frozen_array_mutation();
    }
    guard_writable_length(arr);
    unsafe {
        let length = (*arr).length;
        if length == 0 {
            return TAG_UNDEFINED_F64;
        }

        let new_length = length - 1;
        let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        let value = *elements_ptr.add(new_length as usize);
        (*arr).length = new_length;
        value
    }
}

/// Set the length of an array, JS-spec style.
///
/// Closes #304: `arr.length = N` must truncate when N < length and create holes
/// when N > length. Pre-fix Perry routed this through the generic
/// `js_object_set_field_by_name(obj, "length", N)` path which silently set a
/// new "length" property on the array's hidden object dispatch but never
/// touched the `ArrayHeader.length` field — so `arr.length` still read back
/// the original value, and the elements were never cleared.
///
/// `new_length` arrives as f64 from the codegen (assignment value is a
/// JSValue). The JS ArraySetLength path coerces it with `Number(...)`, then
/// rejects NaN, negative, fractional, infinite, and >uint32 lengths with
/// `RangeError: Invalid array length`.
#[no_mangle]
pub extern "C" fn js_array_set_length(arr: *mut ArrayHeader, new_length: f64) {
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return;
    }
    let n = array_length_from_property_value_or_throw(new_length);
    let scope = crate::gc::RuntimeHandleScope::new();
    let _arr_handle = scope.root_raw_mut_ptr(arr);
    unsafe {
        let cur = (*arr).length;
        let flags = array_object_flags(arr);
        if flags & crate::gc::OBJ_FLAG_FROZEN != 0 {
            return;
        }
        if flags & (crate::gc::OBJ_FLAG_SEALED | crate::gc::OBJ_FLAG_NO_EXTEND) != 0 && n != cur {
            return;
        }
        // `defineProperty(arr, "length", {writable:false})` records the flag
        // in the attrs side table; an ordinary `arr.length = n` write must
        // then no-op (strict-mode throw is handled by the caller's PutValue).
        if n != cur
            && flags & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS != 0
            && crate::object::get_property_attrs(arr as usize, "length")
                .map(|a| !a.writable())
                .unwrap_or(false)
        {
            return;
        }
        if n < cur {
            // Truncate: clear elements at indices [n..cur) to TAG_HOLE so
            // any code that resurrects the slot via `arr[i]` reads `undefined`,
            // not stale data. The capacity stays unchanged — JS doesn't
            // require Perry to release the underlying buffer here, and growing
            // back via `push` would just re-overwrite these slots anyway.
            for i in n..cur {
                note_array_slot(arr, i as usize, crate::value::TAG_HOLE);
            }
            (*arr).length = n;
            refresh_array_numeric_layout(arr);
        } else if n > cur {
            // Extend: pad with TAG_HOLE. Past-capacity extensions go
            // through `js_array_grow` which installs a forwarding pointer at
            // the OLD location (issue #233 mechanism), so the caller's stale
            // pointer transparently follows the chain to the resized buffer
            // on the next access — no callsite-side writeback needed.
            let target = if n > (*arr).capacity {
                js_array_grow(arr, n)
            } else {
                arr
            };
            if !target.is_null() {
                for i in cur..n {
                    note_array_slot(target, i as usize, crate::value::TAG_HOLE);
                }
                (*target).length = n;
            }
        }
        // n == cur is a no-op.
    }
}

/// Delete an element from an array by index, creating a "hole".
/// Clears the element without changing the array length.
/// Matches JavaScript `delete arr[index]` semantics.
/// Returns 1 (true) on success, 0 (false) on failure.
#[no_mangle]
pub extern "C" fn js_array_delete(arr: *mut ArrayHeader, index: u32) -> i32 {
    let obj = arr as *mut crate::object::ObjectHeader;
    if crate::object::is_arguments_object(obj) {
        let name = index.to_string();
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        return crate::object::js_object_delete_field(obj, key);
    }
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return 1;
    }
    unsafe {
        let length = (*arr).length;
        if index >= length {
            return 1; // delete on out-of-bounds always returns true in JS
        }
        let key = index.to_string();
        if let Some(attrs) = crate::object::get_property_attrs(arr as usize, &key) {
            if !attrs.configurable() {
                return 0;
            }
        }
        note_array_slot(arr, index as usize, crate::value::TAG_HOLE);
        crate::object::clear_property_attrs(arr as usize, &key);
        crate::object::clear_accessor_descriptor(arr as usize, &key);
        1
    }
}

/// Shift an element from the beginning of an array.
/// Returns the removed element, or `undefined` if the array is empty (per
/// ECMAScript §23.1.3.27). See the matching note on `js_array_pop_f64` —
/// returning bare `f64::NAN` here was a perry bug that broke the
/// `entry !== undefined` check in connection-pool drivers like
/// `@perryts/mysql`. Issue #536.
#[no_mangle]
pub extern "C" fn js_array_shift_f64(arr: *mut ArrayHeader) -> f64 {
    const TAG_UNDEFINED_F64: f64 = f64::from_bits(0x7FFC_0000_0000_0001u64);
    // Borrowed array-like receiver — see `js_array_pop_f64`.
    if let Some(recv) = crate::array::plain_object_value(arr) {
        return crate::array::generic_object_shift(recv);
    }
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return TAG_UNDEFINED_F64;
    }
    if array_is_frozen(arr) {
        throw_frozen_array_mutation();
    }
    guard_writable_length(arr);
    unsafe {
        let length = (*arr).length;
        if length == 0 {
            return TAG_UNDEFINED_F64;
        }

        let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        let value = *elements_ptr;

        // Shift all elements down
        // GC_STORE_AUDIT(BARRIERED): shift memmove is followed by layout/barrier rebuild.
        ptr::copy(elements_ptr.add(1), elements_ptr, (length - 1) as usize);
        (*arr).length = length - 1;
        rebuild_array_layout(arr);
        value
    }
}

/// Unshift an element to the beginning of an array, growing if needed
/// Returns a pointer to the (possibly reallocated) array
#[no_mangle]
pub extern "C" fn js_array_unshift_f64(arr: *mut ArrayHeader, value: f64) -> *mut ArrayHeader {
    // #5552: a uniquely-owned (refcount==1) string unshifted to the front aliases
    // the new slot — demote it to shared so a later `s += x` on the source local
    // allocates fresh instead of mutating the stored element. No-op for SSO /
    // non-string (mirrors `js_array_push_f64`, #5548).
    crate::string::js_string_addref_if_heap_string(value);
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return js_array_alloc(0);
    }
    if array_is_frozen(arr) {
        throw_frozen_array_mutation();
    }
    guard_writable_length(arr);
    if array_is_sealed_or_no_extend(arr) {
        return arr;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let _arr_handle = scope.root_raw_mut_ptr(arr);
    let value_handle = scope.root_nanbox_f64(value);
    unsafe {
        let length = (*arr).length;
        let capacity = (*arr).capacity;

        let arr = if length >= capacity {
            js_array_grow(arr, length + 1)
        } else {
            arr
        };
        let value = value_handle.get_nanbox_f64();

        let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;

        // Shift all elements up
        // GC_STORE_AUDIT(BARRIERED): unshift memmove and new slot are followed by layout/barrier rebuild.
        ptr::copy(elements_ptr, elements_ptr.add(1), length as usize);
        // Write new element at beginning
        ptr::write(elements_ptr, value);
        (*arr).length = length + 1;
        rebuild_array_layout(arr);
        arr
    }
}

/// Unshift an element as raw JSValue bits (u64), for object/pointer values
/// Returns a pointer to the (possibly reallocated) array
#[no_mangle]
pub extern "C" fn js_array_unshift_jsvalue(arr: *mut ArrayHeader, value: u64) -> *mut ArrayHeader {
    let bits_as_f64 = f64::from_bits(value);
    js_array_unshift_f64(arr, bits_as_f64)
}

/// `arr.unshift(...items)` (#2814) — insert zero or more elements at the front
/// in source order, growing the array if needed. Returns the (possibly
/// reallocated) array header so the caller can read the new length / write the
/// new pointer back. With `count == 0` the array is returned unchanged.
#[no_mangle]
pub extern "C" fn js_array_unshift_variadic(
    arr: *mut ArrayHeader,
    items: *const f64,
    count: u32,
) -> *mut ArrayHeader {
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return js_array_alloc(0);
    }
    // `unshift` always performs `Set(O, "length", …)` (even zero-arg), so both
    // a frozen array and a non-writable `length` throw before the no-op early
    // return. Frozen check must come first because freeze doesn't record "length"
    // attrs, so `guard_writable_length` alone wouldn't catch it.
    if array_is_frozen(arr) {
        throw_frozen_array_mutation();
    }
    guard_writable_length(arr);
    if count == 0 {
        return arr;
    }
    if array_is_sealed_or_no_extend(arr) {
        return arr;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let _arr_handle = scope.root_raw_mut_ptr(arr);
    // Copy the items out before any grow can move arena memory; `items`
    // points at a caller-owned alloca, so it is stable, but we read it
    // before mutating to keep the logic simple.
    let item_vec: Vec<f64> = unsafe {
        if items.is_null() {
            Vec::new()
        } else {
            std::slice::from_raw_parts(items, count as usize).to_vec()
        }
    };
    let n = item_vec.len();
    unsafe {
        let length = (*arr).length;
        let capacity = (*arr).capacity;
        let arr = if length + n as u32 > capacity {
            js_array_grow(arr, length + n as u32)
        } else {
            arr
        };
        let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        // Shift existing elements up by `n`.
        // GC_STORE_AUDIT(BARRIERED): memmove + new slots followed by layout/barrier rebuild.
        ptr::copy(elements_ptr, elements_ptr.add(n), length as usize);
        // Write items in source order at the front. #5552: demote each
        // uniquely-owned string before it aliases its slot (no-op for SSO /
        // non-string).
        for (i, v) in item_vec.into_iter().enumerate() {
            crate::string::js_string_addref_if_heap_string(v);
            // GC_STORE_AUDIT(BARRIERED): inserted slots are followed by the layout/barrier rebuild below.
            ptr::write(elements_ptr.add(i), v);
        }
        (*arr).length = length + n as u32;
        rebuild_array_layout(arr);
        arr
    }
}

#[used]
static KEEP_UNSHIFT_VARIADIC: extern "C" fn(*mut ArrayHeader, *const f64, u32) -> *mut ArrayHeader =
    js_array_unshift_variadic;
