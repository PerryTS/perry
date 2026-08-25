//! flat / clone / entries / keys / values.
use super::*;
use std::ptr;

/// Read the GC object-type byte for an already-range-validated heap pointer
/// (the value returned by `clean_arr_ptr`, which guarantees the address is in
/// the live heap window). Returns `0` if the pointer is too low to hold a
/// preceding `GcHeader`.
///
/// Used by `entries`/`keys`/`values` to detect when the codegen `.entries()`
/// catch-all (`Expr::ArrayEntries`, lowered for any non-class receiver because
/// the static type was lost — see perry-hir `array_only_methods.rs` #597) was
/// actually handed a Map or Set rather than an Array. Effect's
/// `FiberRefs.diff` does `for (const [k, v] of newValue.locals.entries())`
/// where `locals` is a `Map`; without this dispatch the Map was reinterpreted
/// as an Array and its entry buffer read out as garbage `[index, value]`
/// pairs, segfaulting downstream on `pairs.length` (#321 effect Context/Layer).
#[inline]
unsafe fn receiver_gc_type(ptr: *const ArrayHeader) -> u8 {
    let addr = ptr as usize;
    if addr < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return 0;
    }
    let gc_header = (addr - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    (*gc_header).obj_type
}

/// Is `value` an ordinary dense Array whose `[...value]` is *observably* a
/// straight element copy — i.e. an iteration nobody can intercept?
///
/// Spread on a plain array is by far the most common spread in real code, and
/// per ECMA-262 it is `GetIterator` → `%ArrayIteratorPrototype%.next()` per
/// element. Perry implements that literally, which is why `[...tags]` on a
/// 3-element array cost ~66x what `Array.from(tags)` cost: the protocol resolves
/// `@@iterator` through the by-name prototype tower, builds a bound closure,
/// allocates an iterator object, and then allocates a fresh `{ value, done }`
/// result object (plus its two key strings and its keys array — five heap
/// allocations) for every element AND for the terminating step. None of that is
/// observable when the array is ordinary, so this predicate proves ordinariness
/// and lets the caller memcpy instead (#7533).
///
/// Every gate rejects a way the copy could differ from the drain:
///  - `try_read_gc_header` + `GC_TYPE_ARRAY`: a real dense array, not a
///    Set/Map/Buffer/TypedArray/lazy array (each has its own `obj_type`), not a
///    proxy or native handle (rejected by band, without a deref), and not a
///    small-buffer slab allocation (which carries no `GcHeader` at all). A
///    `class X extends Array` instance is object-backed (`GC_TYPE_OBJECT`), so
///    it is excluded here and keeps its snapshot path.
///  - `array_iteration_is_exotic`: no per-index accessor descriptors, no
///    `Array.prototype` / `Object.prototype` index properties shadowing the
///    dense slots, and no live indices past the dense backing store — the three
///    cases where `arr[i]` is not the raw slot.
///  - `array_proto_iterator_modified`: user code replaced or deleted
///    `Array.prototype[Symbol.iterator]`, so the builtin walk is no longer what
///    a spread must run.
///  - `object_static_prototype`: `Object.setPrototypeOf(array, custom)` can
///    replace the inherited iterator without touching Array.prototype.
///  - `has_own_symbol_property`: the instance carries its OWN `[Symbol.iterator]`,
///    which shadows the prototype's. Existence is probed WITHOUT invoking an
///    accessor, so falling through to the slow path calls a user getter exactly
///    once, as the spec requires.
pub(crate) fn dense_spread_source(value: f64) -> Option<*const ArrayHeader> {
    let raw = crate::value::js_nanbox_get_pointer(value) as usize;
    // Band/slab validation BEFORE any deref: rejects handle ids and the
    // header-less small-buffer slab without touching memory.
    unsafe { crate::value::addr_class::try_read_gc_header(raw)? };
    // Only THEN follow the forwarding chain. `js_array_grow` (issue #233) leaves
    // a `GC_FLAG_FORWARDED` header at the OLD address whose first eight bytes —
    // where `length`/`capacity` used to live — now hold the forwarding pointer.
    // Reading `length` off the stale header yields a garbage element count, and
    // the copy below then memcpy'd from an address derived from it: `[...sparse]`
    // after `sparse.length = 5`, and `[...beyond]` after `beyond[9] = 9`, both
    // took EXC_BAD_ACCESS in `_platform_memmove`. Every other array helper cleans
    // first, including `js_array_clone`'s own memcpy tail; this one must too.
    let arr = crate::array::clean_arr_ptr(raw as *const ArrayHeader);
    if arr.is_null() {
        return None;
    }
    // Re-read the type off the POST-forwarding header: the stale one is the
    // address the caller named, the live one is the array we would copy.
    let header = unsafe { crate::value::addr_class::try_read_gc_header(arr as usize)? };
    if header.obj_type != crate::gc::GC_TYPE_ARRAY {
        return None;
    }
    if crate::array::array_iteration_is_exotic(arr) {
        return None;
    }
    if crate::array::array_proto_iterator_modified() {
        return None;
    }
    if crate::object::prototype_chain::object_static_prototype(arr as usize).is_some() {
        return None;
    }
    // Do not materialize Symbol.iterator from a guard. If it is not cached,
    // user code cannot have installed it as an own key; if it is cached, the
    // side-table existence probe below is non-allocating and never invokes an
    // accessor. This keeps dense_spread_source usable in call-site guards that
    // hold evaluated operands in SSA registers.
    let iter_sym = crate::symbol::well_known_symbol_if_cached("iterator");
    if !iter_sym.is_null() {
        let sym_value =
            f64::from_bits(crate::value::JSValue::pointer(iter_sym as *const u8).bits());
        if unsafe { crate::symbol::has_own_symbol_property(value, sym_value) } {
            return None;
        }
    }
    Some(arr)
}

/// Copy a short, exact packed-array spread tail into caller-owned storage.
///
/// Returns the element count (`0..=4`) on success and `-1` when spread must use
/// the generic iterator path. In addition to [`dense_spread_source`]'s exact
/// ordinary-array proof, this rejects holes: the general dense-copy path may
/// normalize a hole to `undefined`, while a direct-call arm promises that each
/// value came from a present packed slot.
///
/// This helper is deliberately non-allocating. Generated code evaluates and
/// roots `receiver`, fixed arguments, and the spread expression before calling
/// it, then uses the copied values only when the returned arity is nonnegative.
#[no_mangle]
pub unsafe extern "C" fn js_short_packed_spread_values(value: f64, out: *mut f64) -> i32 {
    let Some(arr) = dense_spread_source(value) else {
        return -1;
    };
    let len = (*arr).length as usize;
    if len > 4 || (len != 0 && out.is_null()) {
        return -1;
    }
    let elements = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const u64;
    for index in 0..len {
        let bits = std::ptr::read(elements.add(index));
        if bits == crate::value::TAG_HOLE {
            return -1;
        }
        // GC_STORE_AUDIT(STACK): caller-owned generated stack storage; the
        // rooted spread operand keeps copied heap values live during the guard.
        std::ptr::write(out.add(index), f64::from_bits(bits));
    }
    len as i32
}

/// Element-copy an array [`dense_spread_source`] has already proven ordinary.
///
/// `value` is the NaN-boxed receiver rather than a raw pointer because
/// `js_array_alloc` below is a collection point: pre-#7497 the sibling
/// `js_array_clone` derived its source elements from the pre-collection address
/// and memcpy'd retired from-space. Root first, allocate, then re-read BOTH
/// addresses from their handles.
///
/// Holes are the one place a raw copy and the iterator drain disagree: the drain
/// reads `arr[i]`, which yields `undefined` for a hole, while the slot itself
/// holds `TAG_HOLE`. Normalize on the way out so `[...[1, , 3]]` stays
/// `[1, undefined, 3]` and `1 in [...[1, , 3]]` stays `true`.
///
/// Both reads of the source go through `clean_arr_ptr`, so a `js_array_grow`
/// forwarding header (#233) is followed rather than mistaken for the array.
pub(crate) fn dense_spread_copy(value: f64) -> *mut ArrayHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let src_h = scope.root_nanbox_f64(value);
    let read_src = || {
        clean_arr_ptr(
            crate::value::js_nanbox_get_pointer(src_h.get_nanbox_f64()) as *const ArrayHeader
        )
    };
    unsafe {
        let src = read_src();
        if src.is_null() {
            return js_array_alloc(0);
        }
        let len = (*src).length;
        let result_h =
            scope.root_nanbox_f64(crate::value::js_nanbox_pointer(js_array_alloc(len) as i64));
        // Re-read AFTER the allocation: `js_array_alloc` is a collection point,
        // and pre-#7497 the sibling `js_array_clone` memcpy'd from the
        // pre-collection address, i.e. out of retired from-space.
        let src = read_src();
        let result =
            crate::value::js_nanbox_get_pointer(result_h.get_nanbox_f64()) as *mut ArrayHeader;
        if src.is_null() || result.is_null() {
            return js_array_alloc(0);
        }
        if len > 0 {
            let src_elements =
                (src as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const u64;
            let dst_elements =
                (result as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut u64;
            // GC_STORE_AUDIT(BARRIERED): bulk copy into an unpublished array,
            // followed by the exact layout/barrier rebuild below.
            ptr::copy_nonoverlapping(src_elements, dst_elements, len as usize);
            for i in 0..len as usize {
                if ptr::read(dst_elements.add(i)) == crate::value::TAG_HOLE {
                    ptr::write(dst_elements.add(i), crate::value::TAG_UNDEFINED);
                }
            }
            (*result).length = len;
            rebuild_array_layout_exact(result);
        }
        result
    }
}

/// Return a real `ArrayHeader` only when `value` satisfies ECMAScript's
/// `IsArray` check. This unwraps proxy targets, rejects every other
/// `POINTER_TAG` heap object by its GC type, and materializes lazy arrays
/// through `clean_arr_ptr` before callers read the dense array layout.
///
/// The result is suitable for a flattening operation (`flat` / `flatMap`),
/// where arrays are spread by one level but ordinary objects, closures, and
/// other pointer-tagged values remain elements.
#[inline]
pub(crate) fn flattenable_array_ptr(value: f64) -> *const ArrayHeader {
    // IsArray recurses through proxies and must throw for a revoked proxy.
    let mut value = value;
    while let Some(target) = crate::proxy::is_array_proxy_step(value) {
        value = target;
    }

    let bits = value.to_bits();
    let top16 = bits >> 48;
    let addr = if top16 == 0x7FFD {
        (bits & crate::value::POINTER_MASK) as usize
    } else if top16 == 0 && bits >= 0x10000 && (bits & 0x7) == 0 {
        bits as usize
    } else {
        return ptr::null();
    };

    // Do not interpret a pointer-tagged handle as a heap address. The GC type
    // check must happen before reading `ArrayHeader.length`: ObjectHeader and
    // ClosureHeader have incompatible layouts.
    let is_array = unsafe {
        matches!(
            crate::value::addr_class::try_read_gc_header(addr),
            Some(header)
                if header.obj_type == crate::gc::GC_TYPE_ARRAY
                    || header.obj_type == crate::gc::GC_TYPE_LAZY_ARRAY
        )
    };
    if !is_array {
        return ptr::null();
    }

    let array = clean_arr_ptr(addr as *const ArrayHeader);
    if array.is_null() {
        return ptr::null();
    }
    unsafe {
        match crate::value::addr_class::try_read_gc_header(array as usize) {
            Some(header) if header.obj_type == crate::gc::GC_TYPE_ARRAY => array,
            _ => ptr::null(),
        }
    }
}

/// `Array.prototype.flat(depth)` — flatten up to `depth` levels deep
/// (ECMA-262 §23.1.3.10).
#[no_mangle]
pub extern "C" fn js_array_flat_depth(arr: *const ArrayHeader, depth: f64) -> *mut ArrayHeader {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return js_array_alloc(0);
    }
    let levels: u32 = if depth.is_nan() || depth <= 0.0 {
        0
    } else if depth.is_infinite() || depth > u32::MAX as f64 {
        u32::MAX
    } else {
        depth as u32
    };
    unsafe {
        let mut result = js_array_alloc(0);
        result = js_array_flat_into(result, arr, levels);
        result
    }
}

/// Recursive worker for `js_array_flat_depth`. Returns the (possibly
/// re-grown) `result` pointer so `js_array_push_f64`'s reallocation
/// stays in sync across recursive calls.
unsafe fn js_array_flat_into(
    mut result: *mut ArrayHeader,
    src: *const ArrayHeader,
    depth_left: u32,
) -> *mut ArrayHeader {
    let len = (*src).length as usize;
    let elements = (src as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
    for i in 0..len {
        let element = *elements.add(i);
        // Per ECMAScript FlattenIntoArray, holes are absent (HasProperty is
        // false) and are skipped, not copied as `null`/`undefined`.
        if element.to_bits() == crate::value::TAG_HOLE {
            continue;
        }
        let mut pushed = false;
        if depth_left > 0 {
            let sub_arr = flattenable_array_ptr(element);
            if !sub_arr.is_null() {
                let sub_len = (*sub_arr).length as usize;
                if sub_len <= 1_000_000 {
                    result = js_array_flat_into(result, sub_arr, depth_left - 1);
                    pushed = true;
                }
            }
        }
        if !pushed {
            result = js_array_push_f64(result, element);
        }
    }
    result
}

/// Flatten an array of arrays into a single array (depth=1).
/// For each element: if it's an array pointer (NaN-boxed with POINTER_TAG or raw pointer),
/// append all its elements; otherwise append the element directly.
#[no_mangle]
pub extern "C" fn js_array_flat(arr: *const ArrayHeader) -> *mut ArrayHeader {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return js_array_alloc(0);
    }
    unsafe {
        let len = (*arr).length as usize;
        let elements = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        let mut result = js_array_alloc(0);

        for i in 0..len {
            let element = *elements.add(i);
            // Per ECMAScript FlattenIntoArray, holes are absent and skipped.
            if element.to_bits() == crate::value::TAG_HOLE {
                continue;
            }
            let sub_arr = flattenable_array_ptr(element);
            if !sub_arr.is_null() {
                let sub_len = (*sub_arr).length as usize;
                // Sanity check: if length is unreasonably large, treat as non-array.
                if sub_len <= 1_000_000 {
                    let sub_elements = (sub_arr as *const u8)
                        .add(std::mem::size_of::<ArrayHeader>())
                        as *const f64;
                    for j in 0..sub_len {
                        let sub = *sub_elements.add(j);
                        // Skip holes in the flattened sub-array too.
                        if sub.to_bits() == crate::value::TAG_HOLE {
                            continue;
                        }
                        result = js_array_push_f64(result, sub);
                    }
                } else {
                    result = js_array_push_f64(result, element);
                }
            } else {
                // Not an array (non-pointer, or a non-array object) — push as-is.
                result = js_array_push_f64(result, element);
            }
        }

        result
    }
}

/// Spread (`[...x]`) entry point: spec-mandated `GetIterator(x)` throws
/// `TypeError` when `x` is `null` or `undefined`. `js_array_clone` below
/// silently returns `[]` for those inputs (kept for back-compat with
/// `Array.from`'s "not iterable → empty" behavior in Perry today), so
/// spread routes through this wrapper to throw first.
///
/// `boxed` is the raw NaN-boxed f64 value (not pre-unboxed), so we can
/// inspect the tag bits before stripping. The codegen emits this call
/// for the `[..x]` single-spread fast path in
/// `crates/perry-codegen/src/expr/objects_arrays_lit.rs`.
#[no_mangle]
pub extern "C" fn js_array_clone_for_spread(boxed: f64) -> *mut ArrayHeader {
    super::iterator::array_from_spread_value(boxed)
}

/// Clone an array from a NaN-boxed f64 pointer value.
/// Extracts the array pointer from the NaN-boxed value and creates a shallow copy.
/// If the value is not a valid array pointer, returns an empty array.
/// Also handles Sets (via registry check) — converts Set to Array transparently.
#[no_mangle]
pub extern "C" fn js_array_clone(src: *const ArrayHeader) -> *mut ArrayHeader {
    // Strip a NaN-box tag for the registry/string checks below; the
    // raw_addr path is reused for typed-array / Buffer / string
    // detection. Plain-pointer call sites already pass a clean ptr.
    let raw_addr = if !src.is_null() {
        let bits = src as u64;
        if (bits >> 48) >= 0x7FF8 {
            (bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else {
            bits as usize
        }
    } else {
        0
    };

    if let Some(entries) = crate::array::entries_array_for_small_handle_id(raw_addr as i64) {
        return entries;
    }

    // Buffers allocated from the small-buffer slab do not carry a GC header.
    // Detect them before any GC-header probing below; otherwise arbitrary slab
    // bytes immediately before the BufferHeader can be misread as a String or
    // Object header and `Array.from(buf)` materializes nonsense.
    if raw_addr != 0 && crate::buffer::is_registered_buffer(raw_addr) {
        return crate::buffer::buffer_to_array(raw_addr as *const crate::buffer::BufferHeader);
    }

    // `Array.from(string)` iterates the source by Unicode codepoint
    // (each codepoint becomes a 1-char string element) per ECMA-262
    // §23.1.2.1. Pre-fix this fell through to the array memcpy path
    // and emitted garbage f64s built from the string's underlying
    // UTF-8 bytes. Detect via the canonical STRING_TAG (top16=0x7FFF)
    // OR via the GC header's obj_type byte when the receiver arrived
    // as a raw pointer (e.g. through a typed-Any local).
    let is_string_src = {
        let top16 = (src as u64) >> 48;
        if top16 == 0x7FFF {
            true
        } else if raw_addr >= crate::gc::GC_HEADER_SIZE + 0x1000 {
            unsafe {
                let hdr = (raw_addr as *const u8).sub(crate::gc::GC_HEADER_SIZE)
                    as *const crate::gc::GcHeader;
                (*hdr).obj_type == crate::gc::GC_TYPE_STRING
            }
        } else {
            false
        }
    };
    if is_string_src {
        let s_ptr = raw_addr as *const crate::string::StringHeader;
        return unsafe { js_array_from_string_codepoints(s_ptr) };
    }

    // Small native handles (Fetch Headers, streams, timers, etc.) are NaN-boxed
    // as pointer-shaped ids. `Array.from(handle)` / `[...handle]` reach this
    // helper after codegen strips the tag, so ask the generic iterator resolver
    // before treating the id as a non-array and returning [].
    if crate::value::addr_class::is_small_handle(raw_addr) {
        if let Some(dispatch) = crate::object::handle_property_dispatch() {
            let method = b"@@iterator";
            let iter_fn = unsafe { dispatch(raw_addr as i64, method.as_ptr(), method.len()) };
            let fn_raw = crate::value::js_nanbox_get_pointer(iter_fn) as usize;
            if iter_fn.to_bits() != crate::value::TAG_UNDEFINED
                && fn_raw >= 0x10000
                && crate::closure::is_closure_ptr(fn_raw)
            {
                let fn_ptr = fn_raw as *const crate::closure::ClosureHeader;
                let iter = crate::closure::js_closure_call0(fn_ptr);
                if js_array_is_array(iter).to_bits() == crate::value::TAG_TRUE {
                    let ptr = crate::value::js_nanbox_get_pointer(iter) as *mut ArrayHeader;
                    if !ptr.is_null() {
                        return ptr;
                    }
                }
                return js_iterator_to_array(iter);
            }
        }
        return js_array_alloc(0);
    }

    // Check if this is actually a Set (type unknown at compile time)
    if !src.is_null() && crate::set::is_registered_set(src as usize) {
        return crate::set::js_set_to_array(src as *const crate::set::SetHeader);
    }
    // Check if this is a Map (for Array.from(map) → array of [key, value] pairs)
    if !src.is_null() && crate::map::is_registered_map(src as usize) {
        return crate::map::js_map_entries(src as *const crate::map::MapHeader);
    }

    // `Array.from({length: N, 0: ..., 1: ...})` (array-like object) per
    // ECMA-262 §23.1.2.1 step 8: read `.length`, then for each index
    // 0..length read `obj[i]` (missing slots → undefined). Pre-fix this
    // fell through to the array-memcpy path which read an `ObjectHeader` word
    // as `length` (`class_id` since #8113) and the inline f64 slots as
    // elements — garbage. Detect via `GC_TYPE_OBJECT`.
    if raw_addr >= crate::gc::GC_HEADER_SIZE + 0x1000 {
        let obj_type = unsafe {
            let hdr = (raw_addr as *const u8).sub(crate::gc::GC_HEADER_SIZE)
                as *const crate::gc::GcHeader;
            (*hdr).obj_type
        };
        if obj_type == crate::gc::GC_TYPE_OBJECT {
            let obj = raw_addr as *mut crate::ObjectHeader;
            // #1668: `[...searchParams]` / `Array.from(searchParams)` yield the
            // `[key, value]` entry pairs. Detect a URLSearchParams by its shape
            // (`_entries` leads the keys array) and return its entries array.
            // The previous heuristic required `keys_array.length == 1`, but a
            // URL-adopted URLSearchParams also carries a `_owner` key (2 keys),
            // so spread fell through to the array-like path and produced `[]`.
            if crate::url::try_read_as_search_params(obj).is_some() {
                let boxed = crate::url::js_url_search_params_entries_arr(obj);
                let bits = boxed.to_bits();
                let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *mut ArrayHeader;
                if !ptr.is_null() {
                    return ptr;
                }
            }
            // #321: per ECMA-262 §23.1.2.1, `Array.from` prefers the ITERATOR
            // protocol (`obj[Symbol.iterator]`) over the array-like `.length`
            // path. An effect `Chunk` carries BOTH a `.length` field AND a
            // `[Symbol.iterator]` that delegates to `backing.array`'s iterator,
            // so the pre-fix array-like fallback read `.length`=N and `obj[i]`
            // (which a Chunk doesn't store positionally) → N undefined elements.
            // That surfaced downstream as `Cannot read properties of undefined
            // (reading '_tag')` in effect's `exitZipWith`. Drive the iterator
            // protocol when the object is iterable, or when it IS an iterator
            // (the runtime array-iterator class id / a stored `.next` closure).
            unsafe {
                let iter_f64 = crate::value::js_nanbox_pointer(raw_addr as i64);
                // #2856: Map/Set iterator objects dispatch `.next()` /
                // `[Symbol.iterator]()` via class id (no stored symbol prop or
                // `.next` field), so detect them here so `[...m.entries()]` /
                // `Array.from(s.values())` drive the iterator protocol.
                let is_array_iterator = (*obj).class_id == ARRAY_ITERATOR_CLASS_ID
                    || (*obj).class_id == crate::collection_iter_object::MAP_ITERATOR_CLASS_ID
                    || (*obj).class_id == crate::collection_iter_object::SET_ITERATOR_CLASS_ID
                    // #2874: lazy iterator-helper objects (`Iterator.from(x).map(f)`)
                    // dispatch `.next()` via class id, so `[...it]` / `Array.from(it)`
                    // must drive the iterator protocol.
                    || (*obj).class_id == crate::iterator_helpers::ITERATOR_HELPER_CLASS_ID
                    // #3909: Buffer iterators (`buf.keys()`/`values()`/`entries()`)
                    // dispatch `.next()` via class id too — without this `[...buf.keys()]`
                    // / `Array.from(buf.values())` produced an empty array even though
                    // `.next()` and `for...of` already worked.
                    || (*obj).class_id == crate::buffer::BUFFER_ITERATOR_CLASS_ID
                    || (*obj).class_id == crate::regex::REGEXP_STRING_ITERATOR_CLASS_ID;
                let is_iterable = is_array_iterator || {
                    let iter_sym = crate::symbol::well_known_symbol("iterator");
                    if iter_sym.is_null() {
                        false
                    } else {
                        let sym_f64 = f64::from_bits(
                            crate::value::JSValue::pointer(iter_sym as *const u8).bits(),
                        );
                        let iter_fn =
                            crate::symbol::js_object_get_symbol_property(iter_f64, sym_f64);
                        iter_fn.to_bits() != crate::value::TAG_UNDEFINED
                    }
                };
                // Also catch a bare iterator object that exposes `.next()` as a
                // stored closure field but no `[Symbol.iterator]` (uncommon).
                let has_next_field = {
                    let next_key = crate::string::js_string_from_bytes(b"next".as_ptr(), 4);
                    let next_val = crate::object::js_object_get_field_by_name(
                        obj as *const crate::ObjectHeader,
                        next_key,
                    );
                    let next_ptr =
                        crate::value::js_nanbox_get_pointer(f64::from_bits(next_val.bits()))
                            as usize;
                    !next_val.is_undefined() && crate::closure::is_closure_ptr(next_ptr)
                };
                if is_iterable || has_next_field {
                    return js_iterator_to_array(crate::symbol::js_get_iterator(iter_f64));
                }
            }
            return unsafe { js_array_from_arraylike(raw_addr as *const crate::ObjectHeader) };
        }
    }
    // Issue #578: typed array source — materialize each element through the
    // per-kind accessor instead of memcpy'ing the byte-packed storage as if
    // it were a flat f64 array. Without this, `Array.from(uint8array)` /
    // `[...uint8array]` / `for (const b of uint8array)` (which now wraps
    // the iterable in `Expr::ArrayFrom`) all produced raw bit reinterpretations
    // of the underlying bytes rather than the byte values themselves.
    // Strip NaN-box first so the registry lookup sees the real address.
    if !src.is_null() {
        let bits = src as u64;
        let raw_addr = if (bits >> 48) >= 0x7FF8 {
            (bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else {
            bits as usize
        };
        if crate::typedarray::lookup_typed_array_kind(raw_addr).is_some() {
            return crate::typedarray::typed_array_to_array(
                raw_addr as *const crate::typedarray::TypedArrayHeader,
            );
        }
    }
    let src = clean_arr_ptr(src);
    if src.is_null() {
        return js_array_alloc(0);
    }
    // #7497: `js_array_alloc` below can trigger the copying minor, which MOVES
    // `src`. Pre-fix, `src_elements` was derived from the pre-collection address
    // and `copy_nonoverlapping` read retired from-space — so `[...arr]`,
    // `Array.from(arr)` and every combinator's iterable snapshot could copy
    // whatever the recycled bytes now hold. `PERRY_GC_PROTECT_FROMSPACE=1` faults
    // here on the `Promise.all` snapshot at minor #0. Root the source across the
    // allocation and re-read both addresses from their handles afterwards.
    let scope = crate::gc::RuntimeHandleScope::new();
    let src_h = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(src as i64));
    unsafe {
        let len = (*src).length;
        let result_h =
            scope.root_nanbox_f64(crate::value::js_nanbox_pointer(js_array_alloc(len) as i64));
        let src = crate::value::js_nanbox_get_pointer(src_h.get_nanbox_f64()) as *const ArrayHeader;
        let result =
            crate::value::js_nanbox_get_pointer(result_h.get_nanbox_f64()) as *mut ArrayHeader;
        if len > 0 {
            let src_elements =
                (src as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
            let dst_elements =
                (result as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
            // GC_STORE_AUDIT(BARRIERED): clone bulk copy is followed by exact layout/barrier rebuild.
            ptr::copy_nonoverlapping(src_elements, dst_elements, len as usize);
            (*result).length = len;
            rebuild_array_layout_exact(result);
        }
        result
    }
}

/// `arr.entries()` — return a new array of [index, value] pairs.
/// Each pair is itself a 2-element array, NaN-boxed with POINTER_TAG so it
/// reads back as an array pointer when iterated. This eagerly materializes
/// the iterator (Perry has no generic iterator protocol yet) so a `for...of`
/// loop over the result walks it as a normal array via `length`/`arr[i]`.
#[no_mangle]
pub extern "C" fn js_array_entries(arr: *const ArrayHeader) -> *mut ArrayHeader {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return js_array_alloc(0);
    }
    unsafe {
        // The codegen `.entries()` catch-all (Expr::ArrayEntries) lowers any
        // non-class receiver here. When the runtime value is actually a Map or
        // Set, route to the correct iterator materialization instead of
        // reinterpreting its buffer as an Array (#321 effect Context/Layer).
        match receiver_gc_type(arr) {
            t if t == crate::gc::GC_TYPE_MAP => {
                return crate::map::js_map_entries(arr as *const crate::map::MapHeader);
            }
            t if t == crate::gc::GC_TYPE_SET => {
                // Set entries yield `[value, value]` pairs in JS.
                let values = crate::set::js_set_to_array(arr as *const crate::set::SetHeader);
                let len = (*values).length;
                let result = js_array_alloc(len);
                (*result).length = len;
                clear_array_numeric_layout(result);
                for i in 0..len as usize {
                    let v = js_array_get_f64(values, i as u32);
                    let pair = js_array_alloc(2);
                    (*pair).length = 2;
                    store_array_slot(pair, 0, v.to_bits());
                    store_array_slot(pair, 1, v.to_bits());
                    rebuild_array_layout(pair);
                    let pair_value = crate::value::js_nanbox_pointer(pair as i64);
                    store_array_slot(result, i, pair_value.to_bits());
                }
                rebuild_array_layout(result);
                return result;
            }
            _ => {}
        }
        let len = (*arr).length;
        let result = js_array_alloc(len);
        (*result).length = len;
        clear_array_numeric_layout(result);
        let src_elements = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        let dst_elements = (result as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        for i in 0..len as usize {
            // Build a 2-element [index, value] pair as an inner array.
            let pair = js_array_alloc(2);
            (*pair).length = 2;
            let pair_elems = (pair as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
            // GC_STORE_AUDIT(BARRIERED): entries pair slots are immediately recorded via note_array_slot.
            *pair_elems.add(0) = i as f64;
            *pair_elems.add(1) = *src_elements.add(i);
            note_array_slot(pair, 0, (i as f64).to_bits());
            note_array_slot(pair, 1, (*src_elements.add(i)).to_bits());
            // NaN-box the inner array pointer so the outer storage slot keeps tag info.
            let pair_value = crate::value::js_nanbox_pointer(pair as i64);
            // GC_STORE_AUDIT(BARRIERED): outer entries slot is immediately recorded via note_array_slot.
            *dst_elements.add(i) = pair_value;
            note_array_slot(result, i, pair_value.to_bits());
        }
        result
    }
}

/// `arr.keys()` — return a new array of indices [0, 1, ..., length-1].
#[no_mangle]
pub extern "C" fn js_array_keys(arr: *const ArrayHeader) -> *mut ArrayHeader {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return js_array_alloc(0);
    }
    unsafe {
        // Map/Set receivers reaching the `.keys()` catch-all (see
        // js_array_entries) — route to the correct keys. (#321)
        match receiver_gc_type(arr) {
            t if t == crate::gc::GC_TYPE_MAP => {
                return crate::map::js_map_keys(arr as *const crate::map::MapHeader);
            }
            t if t == crate::gc::GC_TYPE_SET => {
                // Set `.keys()` is an alias for `.values()`.
                return crate::set::js_set_to_array(arr as *const crate::set::SetHeader);
            }
            _ => {}
        }
        let len = (*arr).length;
        let result = js_array_alloc(len);
        (*result).length = len;
        let dst_elements = (result as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        for i in 0..len as usize {
            // GC_STORE_AUDIT(POINTER_FREE): keys array stores numeric indices only.
            *dst_elements.add(i) = i as f64;
        }
        result
    }
}

/// `arr.values()` — return a shallow copy of the array.
/// (In JS this returns an iterator; Perry materializes it as a clone so
/// `for...of` over the result iterates the values eagerly.)
#[no_mangle]
pub extern "C" fn js_array_values(arr: *const ArrayHeader) -> *mut ArrayHeader {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return js_array_alloc(0);
    }
    unsafe {
        // Map/Set receivers reaching the `.values()` catch-all (see
        // js_array_entries) — route to the correct values. (#321)
        match receiver_gc_type(arr) {
            t if t == crate::gc::GC_TYPE_MAP => {
                return crate::map::js_map_values(arr as *const crate::map::MapHeader);
            }
            t if t == crate::gc::GC_TYPE_SET => {
                return crate::set::js_set_to_array(arr as *const crate::set::SetHeader);
            }
            _ => {}
        }
        // #7497 (CodeRabbit): the same shape `js_array_clone` had — `arr` is the
        // memcpy SOURCE and `js_array_alloc` below can move it. Root and re-read.
        let scope = crate::gc::RuntimeHandleScope::new();
        let src_h = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(arr as i64));
        let len = (*arr).length;
        let result_h =
            scope.root_nanbox_f64(crate::value::js_nanbox_pointer(js_array_alloc(len) as i64));
        let arr = crate::value::js_nanbox_get_pointer(src_h.get_nanbox_f64()) as *const ArrayHeader;
        let result =
            crate::value::js_nanbox_get_pointer(result_h.get_nanbox_f64()) as *mut ArrayHeader;
        if len > 0 {
            let src_elements =
                (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
            let dst_elements =
                (result as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
            // GC_STORE_AUDIT(BARRIERED): values bulk copy is followed by layout/barrier rebuild.
            ptr::copy_nonoverlapping(src_elements, dst_elements, len as usize);
            (*result).length = len;
            rebuild_array_layout(result);
        }
        result
    }
}
