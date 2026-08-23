//! push / pop / shift / unshift / set_length / delete + grow primitive.
use super::*;
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
    array_length_is_non_writable_with_flags(arr, flags)
}

#[inline]
fn array_length_is_non_writable_with_flags(arr: *const ArrayHeader, flags: u16) -> bool {
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

#[cold]
fn throw_cannot_delete_array_index(index: u32) -> ! {
    crate::collection_iter::throw_type_error(&format!(
        "Cannot delete property '{index}' of [object Array]"
    ));
}

/// Install an array-growth forwarding stub after `tracked_header_for` proves
/// allocator ownership of `old_user_addr`. Keeping the classifier injectable
/// makes the below-2-TiB macOS case deterministic without mapping a fixed low
/// virtual address in the test process.
///
/// # Safety
/// The classifier must return the live header for `old_user_addr`, and
/// `new_user_addr` must be a live array allocation.
#[inline]
pub(super) unsafe fn install_array_growth_forwarding_with(
    old_user_addr: usize,
    new_user_addr: *mut u8,
    tracked_header_for: impl FnOnce(usize) -> Option<std::ptr::NonNull<crate::gc::GcHeader>>,
) -> bool {
    let Some(header) = tracked_header_for(old_user_addr) else {
        return false;
    };
    let header = header.as_ptr();
    if (*header).obj_type != crate::gc::GC_TYPE_ARRAY
        || (*header).gc_flags & crate::gc::GC_FLAG_ARENA == 0
    {
        return false;
    }
    crate::gc::set_forwarding_address(header, new_user_addr);
    true
}

#[inline]
pub(crate) fn guard_writable_length(arr: *const ArrayHeader) {
    if array_length_is_non_writable(arr) {
        throw_non_writable_length();
    }
}

#[inline]
fn guard_writable_length_with_flags(arr: *const ArrayHeader, flags: u16) {
    if array_length_is_non_writable_with_flags(arr, flags) {
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
    // SAFETY: `clean_arr_ptr_mut` just proved this exact pointer is the live,
    // non-forwarded array head; no allocation occurs before the flag read.
    let flags = unsafe { array_object_flags_resolved(arr) };
    if flags & crate::gc::OBJ_FLAG_FROZEN != 0 {
        throw_frozen_array_mutation();
    }
    guard_writable_length_with_flags(arr, flags);
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

        // A growth stub outlives the array operation: aliases can keep its
        // address and `clean_arr_ptr` follows it on a later access. Therefore
        // a non-moving source must not forward into the copying nursery. A
        // minor does not trace a retained `GC_FLAG_FORWARDED` stub as a normal
        // array object, so its payload forwarding word is outside ordinary
        // layout and remembered-set scanning. It would neither move nor retain
        // a young target; resetting from-space would leave the permanent old
        // stub pointing at recycled bytes.
        //
        // For a young source, use the nursery only when the already-open block
        // can satisfy the grow without collecting. If that allocation would
        // collect, the source may be promoted while its handle is reloaded;
        // birth the target old instead so the post-collection source cannot
        // acquire the same old->young forwarding edge.
        let old_header =
            (arr as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
        let source_requires_old_target = (*old_header).gc_flags & crate::gc::GC_FLAG_TENURED != 0
            || !matches!(
                crate::arena::classify_heap_generation(arr as usize),
                crate::arena::HeapGeneration::Nursery
            );
        let new_ptr = if source_requires_old_target {
            crate::arena::arena_alloc_gc_old_born_tenured(new_size, 8, crate::gc::GC_TYPE_ARRAY)
        } else {
            let young =
                crate::arena::arena_alloc_gc_no_collect(new_size, 8, crate::gc::GC_TYPE_ARRAY);
            if young.is_null() {
                crate::arena::arena_alloc_gc_old_born_tenured(new_size, 8, crate::gc::GC_TYPE_ARRAY)
            } else {
                young
            }
        } as *mut ArrayHeader;
        let arr = arr_handle.get_raw_mut_ptr::<ArrayHeader>();
        // GC_STORE_AUDIT(BARRIERED): array growth copy transfers layout and replays write barriers below.
        ptr::copy_nonoverlapping(arr as *const u8, new_ptr as *mut u8, old_size);

        (*new_ptr).capacity = new_capacity;
        // HOLE-initialize the newly added [old_capacity, new_capacity) slack
        // so it never holds stale arena bits the whole-heap from-space scan
        // misreads as live from-space pointers.
        {
            let new_elems =
                (new_ptr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut u64;
            for i in old_capacity as usize..new_capacity as usize {
                // GC_STORE_AUDIT(INIT): initialization of the freshly grown
                // array's added [old_capacity, new_capacity) slack — storage
                // this allocation has never published, written with the
                // non-pointer TAG_HOLE sentinel. No edge is created, so no
                // write barrier (the copied prefix replays its own barriers
                // via replay_array_growth_write_barriers below).
                ptr::write(new_elems.add(i), crate::value::TAG_HOLE);
            }
        }
        let old_header =
            (arr as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
        let new_header =
            (new_ptr as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
        (*new_header)._reserved = (*old_header)._reserved;
        crate::gc::layout_transfer(arr as *mut u8, new_ptr as *mut u8);
        // #7742-adjacent: the copy above is verbatim at offset 0, so the old
        // store's dirty-page coverage can be TRANSLATED to the new address
        // instead of re-derived from 3 M slot values. Falls back to the full
        // value-derived replay whenever the translation declines.
        if !crate::gc::relocate_copied_old_object_dirty_pages(
            new_ptr as usize,
            arr as usize,
            new_ptr as usize,
            old_size,
        ) {
            replay_array_growth_write_barriers(new_ptr);
        }

        // Issue #233: install a forwarding pointer at the OLD location
        // so any stale reference (e.g. an async function's caller still
        // holding the pre-grow pointer in its parameter slot) resolves
        // to the new head via clean_arr_ptr's GC_FLAG_FORWARDED follow.
        // Uses the same forwarding-slot representation as GC evacuation:
        // first 8 bytes of payload (length+capacity) become the new user
        // ptr. Unlike GC-evacuation originals, array-growth stubs stay
        // retained because stale array references rely on clean_arr_ptr
        // following this chain.
        // Ownership comes from the same canonical arena/malloc classifier
        // that clean_arr_ptr uses while following this stub. In particular,
        // valid low-address macOS arena allocations are accepted, while
        // handles, synthetic pointers, and unrelated allocations are rejected
        // before a header dereference.
        let installed =
            install_array_growth_forwarding_with(arr as usize, new_ptr as *mut u8, |addr| {
                crate::value::addr_class::try_read_tracked_gc_header(addr)
            });
        assert!(
            installed,
            "array growth could not install a forwarding stub for the tracked source at {:#x}",
            arr as usize
        );

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

/// `Get(proxy, <string key>)` through the proxy's `get` trap; fresh key
/// string per call, same GC rationale as `proxy_set_str_key`.
unsafe fn proxy_get_str_key(proxy: f64, key_bytes: &[u8]) -> f64 {
    let key = crate::string::js_string_from_bytes(key_bytes.as_ptr(), key_bytes.len() as u32);
    let key_f64 = crate::value::js_nanbox_string(key as i64);
    crate::proxy::js_proxy_get(proxy, key_f64)
}

/// `HasProperty(proxy, <string key>)` through the proxy's `has` trap.
unsafe fn proxy_has_str_key(proxy: f64, key_bytes: &[u8]) -> bool {
    let key = crate::string::js_string_from_bytes(key_bytes.as_ptr(), key_bytes.len() as u32);
    let key_f64 = crate::value::js_nanbox_string(key as i64);
    crate::proxy::js_proxy_has(proxy, key_f64).to_bits() == crate::value::TAG_TRUE
}

/// Spec `DeletePropertyOrThrow(proxy, <string key>)`: routes the
/// `deleteProperty` trap and throws the spec TypeError when the trap reports
/// failure — `pop`/`shift`/`unshift` must abort BEFORE their length write
/// rather than report a successful mutation.
unsafe fn proxy_delete_str_key_or_throw(proxy: f64, key_bytes: &[u8]) {
    let key = crate::string::js_string_from_bytes(key_bytes.as_ptr(), key_bytes.len() as u32);
    let key_f64 = crate::value::js_nanbox_string(key as i64);
    if crate::proxy::js_proxy_delete(proxy, key_f64).to_bits() != crate::value::TAG_TRUE {
        let msg = format!(
            "'deleteProperty' on proxy: trap returned falsish for property '{}'",
            String::from_utf8_lossy(key_bytes)
        );
        crate::collection_iter::throw_type_error(&msg);
    }
}

/// `ToIntegerOrInfinity(ToNumber(v))` — NaN → 0, ±Infinity preserved. The
/// sibling `generic_object` helper skips the ToNumber step (its callers'
/// args are pre-coerced); trap-routed args arrive as arbitrary NaN-boxed
/// values, so coerce first.
fn to_integer_or_infinity_coerced(v: f64) -> f64 {
    let n = crate::builtins::js_number_coerce(v);
    if n.is_nan() {
        0.0
    } else if n.is_infinite() {
        n
    } else {
        n.trunc()
    }
}

/// Resolve a relative-index argument (`splice`/`fill` start, `fill` end) to
/// an absolute index clamped to `[0, len]`.
fn proxy_relative_index(v: f64, len: u64) -> u64 {
    let n = to_integer_or_infinity_coerced(v);
    if n < 0.0 {
        (len as f64 + n).max(0.0) as u64
    } else {
        n.min(len as f64) as u64
    }
}

/// `Array.prototype` mutators on a Proxy receiver, spec-routed through the
/// proxy's `get`/`set`/`deleteProperty` traps (§23.1.3.21 push, §23.1.3.19
/// pop, §23.1.3.24 shift, §23.1.3.32 unshift, §23.1.3.26 reverse, §23.1.3.31
/// splice — length reads/writes and every element move go through
/// `[[Get]]`/`[[Set]]`, which is what fires the traps).
///
/// This is the receiver-normalization gap behind the `holder.list.push(3)`
/// silent no-op: `array_proto_mutator` normalized the receiver with
/// `as_real_array` (which rightly rejects handle-band proxy ids — deref'ing
/// one as an ArrayHeader is the #6279 segfault class) and then
/// `run_object_mutator` (proxy ids are not plain objects), so the whole call
/// fell through to `undefined` WITHOUT mutating anything. The eager HIR fold
/// used to hide this by calling `js_array_push_f64` (proxy-aware via
/// `array_ptr_as_proxy`) directly, until #6397 correctly deferred untyped
/// receivers to the runtime dispatch.
///
/// `sort` is NOT here: `js_arraylike_sort` → `object_sort` already runs the
/// spec algorithm over the proxy-aware `al_*` primitives (and roots its
/// carried values). `fill` lives in [`proxy_array_fill`] below (its
/// `has_start`/`has_end` shape matches `js_array_fill_generic`, its caller),
/// and `copyWithin` in `js_array_copy_within_value`, which routes proxies
/// itself. Returns `None` for anything else — callers keep their previous
/// fall-through behavior.
pub(super) fn proxy_array_mutator(
    proxy: f64,
    method: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    // Every trap below runs arbitrary user JS, which can allocate and move
    // heap values. Root the proxy and the incoming args once and reload each
    // carried value from its handle after any trap/allocating call (same
    // pattern as `js_native_call_method`'s dispatch prologue). The proxy
    // value itself is a registry handle id (not a heap pointer), but rooting
    // it is free and keeps the discipline uniform.
    let scope = crate::gc::RuntimeHandleScope::new();
    let proxy_handle = scope.root_nanbox_f64(proxy);
    let original_args: Vec<f64> = if args_len > 0 && !args_ptr.is_null() {
        unsafe { std::slice::from_raw_parts(args_ptr, args_len).to_vec() }
    } else {
        Vec::new()
    };
    let arg_handles = scope.root_nanbox_f64_slice(&original_args);
    let p = || proxy_handle.get_nanbox_f64();
    let arg = |i: usize| -> f64 {
        arg_handles
            .get(i)
            .map(|h| h.get_nanbox_f64())
            .unwrap_or(undefined)
    };
    unsafe {
        match method {
            // §23.1.3.21 Array.prototype.push
            "push" => {
                let len = proxy_array_length(p());
                for i in 0..args_len {
                    proxy_set_str_key(p(), (len + i as u64).to_string().as_bytes(), arg(i));
                }
                let new_len = len + args_len as u64;
                proxy_set_str_key(p(), b"length", new_len as f64);
                Some(new_len as f64)
            }
            // §23.1.3.19 Array.prototype.pop
            "pop" => {
                let len = proxy_array_length(p());
                if len == 0 {
                    proxy_set_str_key(p(), b"length", 0.0);
                    return Some(undefined);
                }
                let idx = (len - 1).to_string();
                let value_handle = scope.root_nanbox_f64(proxy_get_str_key(p(), idx.as_bytes()));
                proxy_delete_str_key_or_throw(p(), idx.as_bytes());
                proxy_set_str_key(p(), b"length", (len - 1) as f64);
                Some(value_handle.get_nanbox_f64())
            }
            // §23.1.3.24 Array.prototype.shift — HasProperty gates each move:
            // a hole in the source deletes the destination instead of
            // materializing an own `undefined`.
            "shift" => {
                let len = proxy_array_length(p());
                if len == 0 {
                    proxy_set_str_key(p(), b"length", 0.0);
                    return Some(undefined);
                }
                let first_handle = scope.root_nanbox_f64(proxy_get_str_key(p(), b"0"));
                for k in 1..len {
                    let from = k.to_string();
                    let to = (k - 1).to_string();
                    if proxy_has_str_key(p(), from.as_bytes()) {
                        let v_handle =
                            scope.root_nanbox_f64(proxy_get_str_key(p(), from.as_bytes()));
                        proxy_set_str_key(p(), to.as_bytes(), v_handle.get_nanbox_f64());
                    } else {
                        proxy_delete_str_key_or_throw(p(), to.as_bytes());
                    }
                }
                proxy_delete_str_key_or_throw(p(), (len - 1).to_string().as_bytes());
                proxy_set_str_key(p(), b"length", (len - 1) as f64);
                Some(first_handle.get_nanbox_f64())
            }
            // §23.1.3.32 Array.prototype.unshift — same HasProperty gating on
            // the right-shift loop.
            "unshift" => {
                let len = proxy_array_length(p());
                let count = args_len as u64;
                if count > 0 {
                    for k in (0..len).rev() {
                        let from = k.to_string();
                        let to = (k + count).to_string();
                        if proxy_has_str_key(p(), from.as_bytes()) {
                            let v_handle =
                                scope.root_nanbox_f64(proxy_get_str_key(p(), from.as_bytes()));
                            proxy_set_str_key(p(), to.as_bytes(), v_handle.get_nanbox_f64());
                        } else {
                            proxy_delete_str_key_or_throw(p(), to.as_bytes());
                        }
                    }
                    for i in 0..args_len {
                        proxy_set_str_key(p(), i.to_string().as_bytes(), arg(i));
                    }
                }
                let new_len = len + count;
                proxy_set_str_key(p(), b"length", new_len as f64);
                Some(new_len as f64)
            }
            // §23.1.3.26 Array.prototype.reverse — four-case swap with
            // HasProperty gating each side: a hole DELETES the opposite slot
            // instead of materializing an own `undefined`. Returns the
            // receiver.
            "reverse" => {
                let len = proxy_array_length(p());
                for lower in 0..len / 2 {
                    let upper = len - lower - 1;
                    let lower_key = lower.to_string();
                    let upper_key = upper.to_string();
                    let lower_handle = if proxy_has_str_key(p(), lower_key.as_bytes()) {
                        Some(scope.root_nanbox_f64(proxy_get_str_key(p(), lower_key.as_bytes())))
                    } else {
                        None
                    };
                    let upper_handle = if proxy_has_str_key(p(), upper_key.as_bytes()) {
                        Some(scope.root_nanbox_f64(proxy_get_str_key(p(), upper_key.as_bytes())))
                    } else {
                        None
                    };
                    match (&lower_handle, &upper_handle) {
                        (Some(l), Some(u)) => {
                            proxy_set_str_key(p(), lower_key.as_bytes(), u.get_nanbox_f64());
                            proxy_set_str_key(p(), upper_key.as_bytes(), l.get_nanbox_f64());
                        }
                        (None, Some(u)) => {
                            proxy_set_str_key(p(), lower_key.as_bytes(), u.get_nanbox_f64());
                            proxy_delete_str_key_or_throw(p(), upper_key.as_bytes());
                        }
                        (Some(l), None) => {
                            proxy_delete_str_key_or_throw(p(), lower_key.as_bytes());
                            proxy_set_str_key(p(), upper_key.as_bytes(), l.get_nanbox_f64());
                        }
                        (None, None) => {}
                    }
                }
                Some(p())
            }
            // §23.1.3.31 Array.prototype.splice — removed elements land in a
            // FRESH real array (holes preserved: absent sources leave the
            // pre-holed slot untouched); tail moves are HasProperty-gated and
            // a refused `deleteProperty` throws BEFORE the length write.
            "splice" => {
                let len = proxy_array_length(p());
                let actual_start = if args_len >= 1 {
                    proxy_relative_index(arg(0), len)
                } else {
                    0
                };
                let actual_delete_count = if args_len == 0 {
                    0
                } else if args_len == 1 {
                    len - actual_start
                } else {
                    let dc = to_integer_or_infinity_coerced(arg(1));
                    dc.max(0.0).min((len - actual_start) as f64) as u64
                };
                // ArrayCreate throws RangeError for a count ≥ 2^32 (test262
                // splice/create-non-array-invalid-len).
                if actual_delete_count > u32::MAX as u64 {
                    crate::array::array_length_range_error();
                }
                let removed = js_array_alloc_with_length(actual_delete_count as u32);
                let removed_handle = scope.root_raw_mut_ptr(removed);
                for k in 0..actual_delete_count {
                    let from = (actual_start + k).to_string();
                    if proxy_has_str_key(p(), from.as_bytes()) {
                        // The trap runs arbitrary JS and can move `removed`, so
                        // its address is only valid after the call. `across_mut`
                        // is that pattern as one combinator: it runs the call and
                        // hands back the post-collection address, so a stale
                        // pointer is never bound in between (#7341).
                        let (v, removed) = removed_handle.across_mut::<ArrayHeader, _>(|| {
                            proxy_get_str_key(p(), from.as_bytes())
                        });
                        let elems = (removed as *mut u8).add(std::mem::size_of::<ArrayHeader>())
                            as *mut f64;
                        // GC_STORE_AUDIT(BARRIERED): note_array_slot re-stores
                        // the slot with the barrier.
                        ptr::write(elems.add(k as usize), v);
                        note_array_slot(removed, k as usize, v.to_bits());
                    }
                }
                let item_count = args_len.saturating_sub(2) as u64;
                if item_count < actual_delete_count {
                    // Close the gap: shift the tail down…
                    let mut k = actual_start;
                    while k < len - actual_delete_count {
                        let from = (k + actual_delete_count).to_string();
                        let to = (k + item_count).to_string();
                        if proxy_has_str_key(p(), from.as_bytes()) {
                            let v_handle =
                                scope.root_nanbox_f64(proxy_get_str_key(p(), from.as_bytes()));
                            proxy_set_str_key(p(), to.as_bytes(), v_handle.get_nanbox_f64());
                        } else {
                            proxy_delete_str_key_or_throw(p(), to.as_bytes());
                        }
                        k += 1;
                    }
                    // …then delete the vacated trailing slots.
                    let mut k = len;
                    while k > len - actual_delete_count + item_count {
                        proxy_delete_str_key_or_throw(p(), (k - 1).to_string().as_bytes());
                        k -= 1;
                    }
                } else if item_count > actual_delete_count {
                    // Open a gap: shift the tail up, high index first.
                    let mut k = len - actual_delete_count;
                    while k > actual_start {
                        let from = (k + actual_delete_count - 1).to_string();
                        let to = (k + item_count - 1).to_string();
                        if proxy_has_str_key(p(), from.as_bytes()) {
                            let v_handle =
                                scope.root_nanbox_f64(proxy_get_str_key(p(), from.as_bytes()));
                            proxy_set_str_key(p(), to.as_bytes(), v_handle.get_nanbox_f64());
                        } else {
                            proxy_delete_str_key_or_throw(p(), to.as_bytes());
                        }
                        k -= 1;
                    }
                }
                for j in 0..args_len.saturating_sub(2) {
                    proxy_set_str_key(
                        p(),
                        (actual_start + j as u64).to_string().as_bytes(),
                        arg(2 + j),
                    );
                }
                proxy_set_str_key(
                    p(),
                    b"length",
                    (len - actual_delete_count + item_count) as f64,
                );
                Some(super::generic::nanbox_arr(
                    removed_handle.get_raw_mut_ptr::<ArrayHeader>(),
                ))
            }
            _ => None,
        }
    }
}

/// §23.1.3.7 `Array.prototype.fill` on a Proxy receiver — the length read and
/// every element write go through the proxy's traps. Parameter shape matches
/// [`js_array_fill_generic`](crate::array::js_array_fill_generic), its only
/// caller besides the prototype thunk (which routes through it). Returns the
/// receiver.
pub(super) fn proxy_array_fill(
    proxy: f64,
    value: f64,
    has_start: i32,
    start: f64,
    has_end: i32,
    end: f64,
) -> f64 {
    // #5552: the same source lands in many slots — demote a uniquely-owned
    // heap string once (no-op for SSO / non-string), mirroring the dense fill.
    crate::string::js_string_addref_if_heap_string(value);
    let scope = crate::gc::RuntimeHandleScope::new();
    let proxy_handle = scope.root_nanbox_f64(proxy);
    let value_handle = scope.root_nanbox_f64(value);
    unsafe {
        let len = proxy_array_length(proxy_handle.get_nanbox_f64());
        let k = if has_start != 0 {
            proxy_relative_index(start, len)
        } else {
            0
        };
        // Spec: `end === undefined` resolves to `len`, not ToIntegerOrInfinity
        // (which would give 0) — mirror the dense branch's undefined check.
        let end_absent =
            has_end == 0 || crate::value::JSValue::from_bits(end.to_bits()).is_undefined();
        let final_index = if end_absent {
            len
        } else {
            proxy_relative_index(end, len)
        };
        for i in k..final_index {
            proxy_set_str_key(
                proxy_handle.get_nanbox_f64(),
                i.to_string().as_bytes(),
                value_handle.get_nanbox_f64(),
            );
        }
        proxy_handle.get_nanbox_f64()
    }
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
    let cleaned = clean_arr_ptr_mut(arr);
    if cleaned.is_null() {
        // #7574: a `class X extends Array` instance (or any array-like object)
        // in a `T[]`-annotated binding. Pre-fix `clean_arr_ptr` waved its
        // `ObjectHeader` through and the store below overwrote `keys_array` /
        // `meta` — the SECOND push SIGSEGVed (exit 139). Run the spec-generic
        // `Array.prototype.push` on the object instead, and return the ORIGINAL
        // receiver so codegen's realloc write-back leaves the binding pointing
        // at the instance (returning a fresh empty array here is what made the
        // push look silently dropped).
        if let Some(recv) = crate::array::subclass::array_object_receiver(arr) {
            crate::array::subclass::array_object_method(recv, "push", &[value]);
            return arr;
        }
        return js_array_alloc(0);
    }
    let arr = cleaned;
    // One resolved header word answers every policy/layout question below.
    // Re-entering the public helpers here used to run `clean_arr_ptr` (and its
    // allocator-ownership proof) once for each individual bit test.
    // SAFETY: `clean_arr_ptr_mut` just returned this live head and no
    // allocation or safepoint intervenes before the read.
    let flags = unsafe { array_object_flags_resolved(arr) };
    if flags & crate::gc::OBJ_FLAG_FROZEN != 0 {
        throw_frozen_array_mutation();
    }
    guard_writable_length_with_flags(arr, flags);
    if flags & (crate::gc::OBJ_FLAG_SEALED | crate::gc::OBJ_FLAG_NO_EXTEND) != 0 {
        return arr;
    }
    unsafe {
        let length = (*arr).length;
        let capacity = (*arr).capacity;

        if length >= capacity {
            return js_array_push_f64_grow(arr, length, value);
        }

        let value = canonicalize_array_numeric_store_value_from_flags(flags, value);
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
#[cfg(feature = "keepalive-anchors")]
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
    // #7542: call-spread (`f(...arr)`) is `GetIterator(arr)` + drain, so a
    // patched `Array.prototype[Symbol.iterator]` decides how many arguments the
    // callee receives. The element copy below never consults the protocol, so
    // `f(...[1,2,3])` passed 3 arguments where node passes whatever the patched
    // iterator yields (1).
    //
    // Materialize through the protocol and copy THAT, rather than concatenating:
    // this helper appends into `target` in place and returns it, and callers
    // rely on that identity. `js_array_clone_for_spread` is the same entry point
    // `[...arr]` uses, so the two spread forms cannot disagree.
    let source = if crate::array::array_proto_iterator_modified() {
        let boxed = crate::value::js_nanbox_pointer(source as i64);
        let materialized = crate::array::js_array_clone_for_spread(boxed);
        if materialized.is_null() {
            return target;
        }
        materialized as *const ArrayHeader
    } else {
        source
    };
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
        if !crate::array::array_iteration_is_exotic(arr) {
            let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
            let value = *elements_ptr.add(new_length as usize);
            (*arr).length = new_length;
            return value;
        }

        let scope = crate::gc::RuntimeHandleScope::new();
        let arr_handle = scope.root_raw_mut_ptr(arr);
        // `Get(O, ToString(newLength))` must consult the prototype chain. It
        // can also run an accessor which allocates, moves `arr`, freezes it,
        // or makes its length non-writable, so keep both values rooted and
        // resolve the receiver again after every observable operation.
        let (value_handle, arr) = arr_handle.across_mut::<ArrayHeader, _>(|| {
            scope.root_nanbox_f64(crate::array::array_spec_get(arr, new_length))
        });
        let arr = clean_arr_ptr_mut(arr);

        // DeletePropertyOrThrow only targets an own property. An inherited
        // value is returned without deleting it from Array.prototype.
        let (_, arr) = arr_handle.across_mut::<ArrayHeader, _>(|| {
            if crate::array::array_has_own_index(arr, new_length)
                && js_array_delete(arr, new_length) == 0
            {
                throw_cannot_delete_array_index(new_length);
            }
        });

        let arr = clean_arr_ptr_mut(arr);
        if array_is_frozen(arr) {
            throw_frozen_array_mutation();
        }
        guard_writable_length(arr);
        (*arr).length = new_length;
        value_handle.get_nanbox_f64()
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
/// `arr.length = v` as the user-visible assignment — `Set(O, "length", v, true)`
/// (PutValue with `Throw = true`, ECMA-262 §13.15.2). A frozen array's `length`
/// is a non-writable data property, so the write must throw a **TypeError**
/// rather than silently no-op — even when `v` equals the current length or is an
/// invalid length (V8 rejects the non-writable write before coercing the value,
/// so a frozen `arr.length = -1` is a TypeError, not a RangeError; hence the
/// guard precedes `array_length_from_property_value_or_throw`).
///
/// This throwing variant is separate from `js_array_set_length` so the *internal*
/// callers of the latter (`Object.defineProperty(arr, "length", …)`,
/// array-object length coercion, stream/url bookkeeping) keep their silent,
/// non-throwing `[[DefineOwnProperty]]`/no-Throw contract. Only the assignment
/// codegen paths (`field_set_by_name` / `property_set` / proxy `PutValue`) route
/// here. test262 built-ins/Array length-write-on-frozen.
#[no_mangle]
pub extern "C" fn js_array_set_length_strict(arr: *mut ArrayHeader, new_length: f64) {
    let cleaned = clean_arr_ptr_mut(arr);
    if cleaned.is_null() {
        // #7574: `a.length = n` on a `class X extends Array` instance reached
        // here through the `is_array_expr`-keyed `property_set` lowering and
        // wrote the first `ObjectHeader` word (`class_id` since #8113 — i.e.
        // the write corrupts class identity, not an inert tag). Perform the
        // Array-exotic
        // `Set(O, "length", n, true)` on the object instead.
        if let Some(recv) = crate::array::subclass::array_object_receiver(arr) {
            crate::array::subclass::array_object_set_length(recv, new_length);
        }
        return;
    }
    let arr = cleaned;
    if array_object_flags(arr) & crate::gc::OBJ_FLAG_FROZEN != 0 {
        throw_non_writable_length();
    }
    js_array_set_length(arr, new_length);
}

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
            // ArraySetLength deletes indices from high to low. This must clear
            // descriptor-backed indices as well as dense slots: an accessor at
            // index 0 cannot remain observable after its getter truncates the
            // array to zero. If a non-configurable index blocks deletion, keep
            // that index and restore length to index + 1 per §10.4.2.4.
            //
            // A large logical extension does not allocate its holes (see the
            // growth branch below), so do not walk those holes when the length
            // is restored. Far materialized indices live in the named-property
            // table. Delete them first, in the same descending order required
            // by ArraySetLength, then visit the allocated dense prefix.
            let capacity = (*arr).capacity;
            if cur > capacity {
                let mut sparse_indices: Vec<u32> = array_named_property_names(arr, false)
                    .into_iter()
                    .filter_map(|name| {
                        let index = name.parse::<u32>().ok()?;
                        (index != u32::MAX
                            && index >= n.max(capacity)
                            && index < cur
                            && index.to_string() == name)
                            .then_some(index)
                    })
                    .collect();
                sparse_indices.sort_unstable_by(|a, b| b.cmp(a));
                sparse_indices.dedup();
                for i in sparse_indices {
                    if js_array_delete(arr, i) == 0 {
                        (*arr).length = i + 1;
                        refresh_array_numeric_layout(arr);
                        return;
                    }
                }
            }
            for i in (n..cur.min(capacity)).rev() {
                if js_array_delete(arr, i) == 0 {
                    (*arr).length = i + 1;
                    refresh_array_numeric_layout(arr);
                    return;
                }
            }
            (*arr).length = n;
            refresh_array_numeric_layout(arr);
        } else if n > cur {
            // Growing `length` creates holes conceptually; it must not allocate
            // a dense backing store proportional to the requested length.
            // Test262's descriptor probe writes 2^32-1 here. Keep large sparse
            // extensions logical and let later indexed writes choose storage.
            if n > (*arr).capacity && n > 1_000_000 {
                (*arr).length = n;
                refresh_array_numeric_layout(arr);
                return;
            }
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
        if index < (*arr).capacity {
            note_array_slot(arr, index as usize, crate::value::TAG_HOLE);
        }
        // Sparse indices live in the named-property side table rather than the
        // dense allocation. Always clear that representation too: a later
        // growth can make a formerly sparse index fall below capacity.
        array_named_property_delete_by_name(arr, &key);
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

#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_UNSHIFT_VARIADIC: extern "C" fn(*mut ArrayHeader, *const f64, u32) -> *mut ArrayHeader =
    js_array_unshift_variadic;
