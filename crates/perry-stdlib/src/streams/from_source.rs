//! `ReadableStream.from(iterable)`, the Blob and Response body bridges, and the `renderToReadableStream` backend.

use super::{
    alloc_readable, alloc_readable_from_bytes, ensure_gc_registered, js_array_alloc, js_array_get,
    js_array_is_array, js_array_length, js_array_push_f64, js_iterator_to_array,
    js_nanbox_get_pointer, js_native_call_value, js_object_get_field_by_name, js_promise_reason,
    js_promise_run_microtasks, js_promise_state, js_promise_value, js_string_from_bytes,
    js_value_is_promise, provider_js_ffi_root_get_nanbox, provider_js_ffi_root_push_nanbox,
    provider_js_ffi_root_scope_enter, provider_js_ffi_root_scope_exit, rs_rec, stream_value,
    throw_type_error, JSValue, ObjectHeader, Promise, ReadableState, RsRef, POINTER_MASK, TAG_TRUE,
    TAG_UNDEFINED,
};

// Refs #915 (effect smoke fallback path): the `crate::fetch::*` helpers
// referenced below only exist behind the `web-fetch` feature, so the
// streams-only auto-optimize build (`bundled-streams` alone, no
// `web-fetch`) failed to compile. Gate the two blob/response constructors
// on `web-fetch` and provide no-op stubs when it's off — anything that
// actually needs a Blob/Response went through `web-fetch` anyway. (#5174:
// these track `fetch.rs`, which moved from `http-client` to `web-fetch`.)
#[cfg(feature = "web-fetch")]
#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_from_blob(blob_id: f64) -> f64 {
    let bytes = crate::fetch::blob_bytes_clone(blob_id as usize).unwrap_or_default();
    alloc_readable_from_bytes(bytes).value()
}

#[cfg(not(feature = "web-fetch"))]
#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_from_blob(_blob_id: f64) -> f64 {
    alloc_readable_from_bytes(Vec::new()).value()
}

#[cfg(feature = "web-fetch")]
#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_from_response(resp_id: f64) -> f64 {
    crate::fetch::response_body_stream(resp_id as usize)
}

#[cfg(not(feature = "web-fetch"))]
#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_from_response(_resp_id: f64) -> f64 {
    alloc_readable_from_bytes(Vec::new()).value()
}

pub(super) fn ptr_addr_from_nanbox(value: f64) -> Option<usize> {
    let bits = value.to_bits();
    let top = bits >> 48;
    if top == 0x7FFD || top == 0x7FFF {
        Some((bits & POINTER_MASK) as usize)
    } else if top == 0 && perry_runtime::value::addr_class::is_above_handle_band(bits as usize) {
        Some(bits as usize)
    } else {
        None
    }
}

pub(super) unsafe fn chunks_from_array_ptr(arr_ptr: *const perry_runtime::ArrayHeader) -> Vec<u64> {
    let len = js_array_length(arr_ptr);
    (0..len).map(|i| js_array_get(arr_ptr, i).bits()).collect()
}

pub(super) unsafe fn chunks_from_sync_iterable(value: f64) -> Option<Vec<u64>> {
    let iter = perry_runtime::symbol::js_get_iterator(value);
    if iter.to_bits() == value.to_bits() {
        return None;
    }
    let arr = js_iterator_to_array(iter);
    Some(chunks_from_array_ptr(arr))
}

pub(super) struct ReadableFromSource {
    chunks: Vec<u64>,
    error: Option<u64>,
}

impl ReadableFromSource {
    fn closed(chunks: Vec<u64>) -> Self {
        Self {
            chunks,
            error: None,
        }
    }
}

pub(super) enum SettledValue {
    Fulfilled(f64),
    Rejected(u64),
    Pending,
}

pub(super) unsafe fn is_callable_value(value: f64) -> bool {
    let raw = js_nanbox_get_pointer(value);
    raw >= 0x10000 && perry_runtime::closure::is_closure_ptr(raw as usize)
}

pub(super) unsafe fn async_iterator_method(value: f64) -> Option<f64> {
    let sym = perry_runtime::symbol::well_known_symbol("asyncIterator");
    if sym.is_null() {
        return None;
    }
    let sym_value = f64::from_bits(JSValue::pointer(sym as *const u8).bits());
    let method = perry_runtime::symbol::js_object_get_symbol_property(value, sym_value);
    is_callable_value(method).then_some(method)
}

/// Whether `value` has a callable `Symbol.asyncIterator` (without calling it).
pub(crate) unsafe fn has_async_iterator(value: f64) -> bool {
    async_iterator_method(value).is_some()
}

pub(super) unsafe fn call_symbol_async_iterator(value: f64) -> Option<f64> {
    let method = async_iterator_method(value)?;
    let iterator = js_native_call_value(method, value, std::ptr::null(), 0);
    if iterator.to_bits() == TAG_UNDEFINED {
        None
    } else {
        Some(iterator)
    }
}

pub(super) unsafe fn has_iterator_next(value: f64) -> bool {
    let ptr = js_nanbox_get_pointer(value);
    if ptr == 0 {
        return false;
    }
    let obj = ptr as *const ObjectHeader;
    let next_key = js_string_from_bytes(b"next".as_ptr(), 4);
    let next = js_object_get_field_by_name(obj, next_key);
    is_callable_value(f64::from_bits(next.bits()))
}

pub(super) unsafe fn await_maybe_promise(value: f64) -> SettledValue {
    if js_value_is_promise(value) == 0 {
        return SettledValue::Fulfilled(value);
    }
    let promise = js_nanbox_get_pointer(value) as *mut Promise;
    if promise.is_null() {
        return SettledValue::Fulfilled(value);
    }

    // The microtask drain runs JS: a collection inside it may move the
    // promise, so the frame roots it and re-reads it after every drain.
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let held = scope.root_raw_mut_ptr(promise);
    for _ in 0..100_000 {
        if js_promise_state(held.get_raw_mut_ptr()) != 0 {
            break;
        }
        if js_promise_run_microtasks() == 0 {
            break;
        }
    }

    let promise = held.get_raw_mut_ptr::<Promise>();
    match js_promise_state(promise) {
        1 => SettledValue::Fulfilled(js_promise_value(promise)),
        2 => SettledValue::Rejected(js_promise_reason(promise).to_bits()),
        _ => SettledValue::Pending,
    }
}

pub(super) unsafe fn call_iterator_next(iterator: f64) -> Option<f64> {
    if js_nanbox_get_pointer(iterator) == 0 {
        return None;
    }
    // The key allocation can collect: the iterator is the receiver of the
    // call below, so it is rooted across it and re-read.
    let base = provider_call!(provider_js_ffi_root_scope_enter());
    let slot = provider_call!(provider_js_ffi_root_push_nanbox(iterator.to_bits()));
    let next_key = js_string_from_bytes(b"next".as_ptr(), 4);
    let iterator = f64::from_bits(provider_call!(provider_js_ffi_root_get_nanbox(slot)));
    provider_call!(provider_js_ffi_root_scope_exit(base));
    let iter_obj = js_nanbox_get_pointer(iterator) as *const ObjectHeader;
    let next_val = js_object_get_field_by_name(iter_obj, next_key);
    let next = f64::from_bits(next_val.bits());
    if is_callable_value(next) {
        Some(js_native_call_value(next, iterator, std::ptr::null(), 0))
    } else {
        Some(perry_runtime::object::js_native_call_method(
            iterator,
            b"next".as_ptr() as *const i8,
            4,
            std::ptr::null(),
            0,
        ))
    }
}

pub(super) unsafe fn try_call_iterator_next(iterator: f64) -> Result<Option<f64>, u64> {
    perry_runtime::exception::catch_js_throw(|| unsafe { call_iterator_next(iterator) })
        .map_err(f64::to_bits)
}

pub(super) unsafe fn chunks_from_async_iterable(value: f64) -> Option<ReadableFromSource> {
    let iterator = if let Some(iterator) = call_symbol_async_iterator(value) {
        iterator
    } else if has_iterator_next(value) {
        value
    } else {
        return None;
    };
    // Each `next()` runs JS, which can move heap values: keep everything we
    // hold across it in runtime handles.
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let iterator = scope.root_nanbox_f64(iterator);
    let done_key = scope.root_string_ptr(js_string_from_bytes(b"done".as_ptr(), 4));
    let value_key = scope.root_string_ptr(js_string_from_bytes(b"value".as_ptr(), 5));
    let mut chunks = Vec::new();
    let finish =
        |chunks: &[perry_runtime::gc::RuntimeHandle<'_>], error: Option<u64>| ReadableFromSource {
            chunks: perry_runtime::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(chunks)
                .into_iter()
                .map(f64::to_bits)
                .collect(),
            error,
        };

    for _ in 0..100_000 {
        let step = match try_call_iterator_next(iterator.get_nanbox_f64()) {
            Ok(Some(step)) => step,
            Ok(None) => break,
            Err(reason) => return Some(finish(&chunks, Some(reason))),
        };
        let step_result = match await_maybe_promise(step) {
            SettledValue::Fulfilled(result) => result,
            SettledValue::Rejected(reason) => return Some(finish(&chunks, Some(reason))),
            SettledValue::Pending => break,
        };
        let result_ptr = js_nanbox_get_pointer(step_result);
        if result_ptr == 0 {
            break;
        }
        let result_obj = result_ptr as *const ObjectHeader;
        let done_val = js_object_get_field_by_name(result_obj, done_key.get_raw_const_ptr());
        let done = f64::from_bits(done_val.bits());
        if perry_runtime::value::js_is_truthy(done) != 0 {
            break;
        }
        let item = js_object_get_field_by_name(result_obj, value_key.get_raw_const_ptr());
        chunks.push(scope.root_nanbox_u64(item.bits()));
    }

    Some(finish(&chunks, None))
}

/// `ReadableStream.from(iterable)` (Node 20+, #1645) — build a Web
/// ReadableStream pre-loaded with the iterable's items, then closed. Async
/// iterators preserve a terminal rejection after any chunks already yielded.
/// Each element becomes one chunk so `getReader().read()` / `for await` yield
/// them in order, then `done`.
#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_from_iterable(value: f64) -> f64 {
    ensure_gc_registered();
    let ptr_addr = ptr_addr_from_nanbox(value);

    let source = if let Some(source) = chunks_from_async_iterable(value) {
        source
    } else if js_array_is_array(value).to_bits() == TAG_TRUE {
        let arr_ptr = ptr_addr.unwrap_or(0) as *const perry_runtime::ArrayHeader;
        ReadableFromSource::closed(chunks_from_array_ptr(arr_ptr))
    } else if let Some(addr) = ptr_addr {
        if perry_runtime::typedarray_hook_slots::lookup_typed_array_kind(addr).is_some() {
            let ta = addr as *const perry_runtime::typedarray_hook_slots::TypedArrayHeader;
            let len = perry_runtime::typedarray_hook_slots::js_typed_array_length(ta).max(0);
            let chunks = (0..len)
                .map(|i| perry_runtime::typedarray_hook_slots::js_typed_array_get(ta, i).to_bits())
                .collect();
            ReadableFromSource::closed(chunks)
        } else if perry_runtime::buffer::is_registered_buffer(addr)
            && !perry_runtime::buffer::is_any_array_buffer(addr)
            && !perry_runtime::buffer::is_data_view(addr)
        {
            let chunks = perry_runtime::buffer::bytes::no_gc(|scope| {
                perry_runtime::buffer::bytes::bytes(value, scope)
                    .unwrap_or(&[])
                    .iter()
                    .map(|&byte| (byte as f64).to_bits())
                    .collect()
            });
            ReadableFromSource::closed(chunks)
        } else if let Some(chunks) = chunks_from_sync_iterable(value) {
            ReadableFromSource::closed(chunks)
        } else {
            throw_type_error("ReadableStream.from requires an iterable");
        }
    } else {
        throw_type_error("ReadableStream.from requires an iterable");
    };

    let id = alloc_readable(0, 0, 0, 1.0);
    {
        if let Some(s) = rs_rec(RsRef(id)).as_deref_mut() {
            for bits in source.chunks {
                s.push_chunk(bits, 1.0);
            }
            s.started = true;
            if let Some(error) = source.error {
                if s.chunks.is_empty() {
                    s.state = ReadableState::Errored;
                    s.error_value = error;
                } else {
                    s.state = ReadableState::Readable;
                    s.pending_error_after_chunks = Some(error);
                }
            } else {
                s.state = ReadableState::Closed;
            }
        }
    }
    stream_value(id)
}

/// #1671: `renderToReadableStream` stream backend. Builds a closed
/// single-chunk readable stream carrying the already-rendered HTML string, so
/// hono's `renderToReadableStream(<App/>)` returns a real Web stream that
/// `getReader()` / `for await` can drain. Registered into the runtime via
/// `js_register_jsx_render_stream` from `js_stdlib_init_dispatch` when
/// `bundled-streams` is linked; absent that, the runtime thunk returns the
/// rendered HTML node directly (degraded but usable).
#[no_mangle]
pub unsafe extern "C" fn js_jsx_render_stream_from_value(html_value: f64) -> f64 {
    // The array allocation can collect: root the HTML string across it.
    let base = provider_call!(provider_js_ffi_root_scope_enter());
    let slot = provider_call!(provider_js_ffi_root_push_nanbox(html_value.to_bits()));
    let empty = js_array_alloc(1);
    let html_value = f64::from_bits(provider_call!(provider_js_ffi_root_get_nanbox(slot)));
    provider_call!(provider_js_ffi_root_scope_exit(base));
    let arr = js_array_push_f64(empty, html_value);
    let arr_f64 = f64::from_bits(perry_runtime::JSValue::pointer(arr as *const u8).bits());
    js_readable_stream_from_iterable(arr_f64)
}
