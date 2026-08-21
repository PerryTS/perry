//! Provider-safe GC registration for Web Streams registry roots.

use super::*;
use std::ffi::c_void;

const FFI_SLOT_I64: u32 = 1;
const FFI_SLOT_RAW_MUT_PTR: u32 = 3;
const FFI_SLOT_NANBOX_U64: u32 = 5;

type FfiMutableRootVisitor = extern "C" fn(kind: u32, slot: *mut c_void, ctx: *mut c_void) -> bool;
type FfiNamedMutableRootScanner =
    extern "C" fn(scanner_id: usize, visit: FfiMutableRootVisitor, ctx: *mut c_void);
type StreamGetReaderFn = unsafe extern "C" fn(f64) -> f64;
type StreamReaderReadFn = unsafe extern "C" fn(f64) -> *mut Promise;
type StreamExpandoSetFn =
    unsafe extern "C" fn(id: usize, key_ptr: *const u8, key_len: usize, value: f64) -> i32;

extern "C" {
    fn perry_ffi_gc_register_mutable_root_scanner_named(
        source_ptr: *const u8,
        source_len: usize,
        scanner_id: usize,
        scanner: FfiNamedMutableRootScanner,
    );
    #[link_name = "js_register_stream_consumer_callbacks"]
    fn provider_js_register_stream_consumer_callbacks(
        get_reader: StreamGetReaderFn,
        reader_read: StreamReaderReadFn,
    );
    #[link_name = "js_register_stream_expando_set"]
    fn provider_js_register_stream_expando_set(hook: StreamExpandoSetFn);
}

pub(super) trait StreamRootVisitor {
    fn visit_i64_slot(&mut self, slot: &mut i64);
    fn visit_raw_mut_ptr_slot<T>(&mut self, slot: &mut *mut T);
    fn visit_nanbox_u64_slot(&mut self, slot: &mut u64);
}

impl StreamRootVisitor for perry_runtime::gc::RuntimeRootVisitor<'_> {
    fn visit_i64_slot(&mut self, slot: &mut i64) {
        perry_runtime::gc::RuntimeRootVisitor::visit_i64_slot(self, slot);
    }

    fn visit_raw_mut_ptr_slot<T>(&mut self, slot: &mut *mut T) {
        perry_runtime::gc::RuntimeRootVisitor::visit_raw_mut_ptr_slot(self, slot);
    }

    fn visit_nanbox_u64_slot(&mut self, slot: &mut u64) {
        perry_runtime::gc::RuntimeRootVisitor::visit_nanbox_u64_slot(self, slot);
    }
}

struct FfiStreamRootVisitor {
    visit: FfiMutableRootVisitor,
    ctx: *mut c_void,
}

impl StreamRootVisitor for FfiStreamRootVisitor {
    fn visit_i64_slot(&mut self, slot: &mut i64) {
        (self.visit)(FFI_SLOT_I64, slot as *mut i64 as *mut c_void, self.ctx);
    }

    fn visit_raw_mut_ptr_slot<T>(&mut self, slot: &mut *mut T) {
        (self.visit)(
            FFI_SLOT_RAW_MUT_PTR,
            slot as *mut *mut T as *mut c_void,
            self.ctx,
        );
    }

    fn visit_nanbox_u64_slot(&mut self, slot: &mut u64) {
        (self.visit)(
            FFI_SLOT_NANBOX_U64,
            slot as *mut u64 as *mut c_void,
            self.ctx,
        );
    }
}

static CALLBACKS_REGISTERED: std::sync::Once = std::sync::Once::new();

thread_local! {
    // The mutable-root scanner registry is thread-local, so this latch must be too.
    static GC_REGISTERED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Register through the stable C ABI so a separately packaged stdlib installs
/// its scanner in the current thread's runtime registry, not in fallback Rust
/// glue that may also be present in the stdlib image.
pub(super) fn ensure_gc_registered() {
    GC_REGISTERED.with(|registered| {
        if registered.get() {
            return;
        }
        const SOURCE: &[u8] = b"stdlib:streams";
        unsafe {
            perry_ffi_gc_register_mutable_root_scanner_named(
                SOURCE.as_ptr(),
                SOURCE.len(),
                0,
                scan_stream_roots_ffi,
            );
        }
        registered.set(true);
    });
    CALLBACKS_REGISTERED.call_once(|| unsafe {
        provider_js_register_stream_consumer_callbacks(
            js_readable_stream_get_reader,
            js_reader_read,
        );
        provider_js_register_stream_expando_set(expando::stream_expando_set_hook);
    });
}

extern "C" fn scan_stream_roots_ffi(
    _scanner_id: usize,
    visit: FfiMutableRootVisitor,
    ctx: *mut c_void,
) {
    scan_stream_roots_with(&mut FfiStreamRootVisitor { visit, ctx });
}

#[cfg(test)]
pub(super) fn scan_stream_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = perry_runtime::gc::RuntimeRootVisitor::for_copy(mark);
    scan_stream_roots_with(&mut visitor);
}

pub(super) fn visit_stream_value_slot<V: StreamRootVisitor>(visitor: &mut V, slot: &mut u64) {
    let top = *slot >> 48;
    if matches!(top, 0x7FFA | 0x7FFD | 0x7FFF) {
        visitor.visit_nanbox_u64_slot(slot);
    }
}

pub(super) fn scan_stream_roots_with<V: StreamRootVisitor>(visitor: &mut V) {
    expando::scan_expando_roots(visitor);
    if let Ok(mut map) = READABLE_STREAMS.lock() {
        for stream in map.values_mut() {
            visitor.visit_i64_slot(&mut stream.start_cb);
            visitor.visit_i64_slot(&mut stream.pull_cb);
            visitor.visit_i64_slot(&mut stream.cancel_cb);
            visitor.visit_i64_slot(&mut stream.strategy_size_cb);
            for chunk in stream.chunks.iter_mut() {
                visit_stream_value_slot(visitor, chunk);
            }
            for promise in stream.pending_reads.iter_mut() {
                visitor.visit_raw_mut_ptr_slot(promise);
            }
            if stream.state == ReadableState::Errored {
                visit_stream_value_slot(visitor, &mut stream.error_value);
            }
            if let Some(error) = &mut stream.pending_error_after_chunks {
                visit_stream_value_slot(visitor, error);
            }
        }
    }
    byob::scan_byob_roots(visitor);
    if let Ok(mut map) = WRITABLE_STREAMS.lock() {
        for stream in map.values_mut() {
            visitor.visit_i64_slot(&mut stream.write_cb);
            visitor.visit_i64_slot(&mut stream.close_cb);
            visitor.visit_i64_slot(&mut stream.abort_cb);
            visitor.visit_i64_slot(&mut stream.strategy_size_cb);
            for (chunk, promise, _size) in stream.write_queue.iter_mut() {
                visit_stream_value_slot(visitor, chunk);
                visitor.visit_raw_mut_ptr_slot(promise);
            }
            visitor.visit_raw_mut_ptr_slot(&mut stream.ready_promise);
            visitor.visit_raw_mut_ptr_slot(&mut stream.closed_promise);
            visitor.visit_raw_mut_ptr_slot(&mut stream.close_request_promise);
            if stream.state == WritableState::Errored {
                visit_stream_value_slot(visitor, &mut stream.error_value);
            }
        }
    }
    if let Ok(mut map) = TRANSFORM_STREAMS.lock() {
        for transform in map.values_mut() {
            visitor.visit_i64_slot(&mut transform.transform_cb);
            visitor.visit_i64_slot(&mut transform.flush_cb);
        }
    }
    scan_transform_deferred_roots(visitor);
    if let Ok(mut map) = READERS.lock() {
        for reader in map.values_mut() {
            visitor.visit_raw_mut_ptr_slot(&mut reader.closed_promise);
        }
    }
    if let Ok(mut map) = WRITERS.lock() {
        for writer in map.values_mut() {
            visitor.visit_raw_mut_ptr_slot(&mut writer.closed_promise);
            visitor.visit_raw_mut_ptr_slot(&mut writer.ready_promise);
        }
    }
}

pub(super) fn scan_transform_deferred_roots<V: StreamRootVisitor>(visitor: &mut V) {
    if let Ok(mut map) = transform::TRANSFORM_WRITE_RELEASES.lock() {
        for promises in map.values_mut() {
            for slot in promises.iter_mut() {
                let mut promise = *slot as *mut Promise;
                visitor.visit_raw_mut_ptr_slot(&mut promise);
                *slot = promise as usize;
            }
        }
    }
    if let Ok(mut map) = transform::TRANSFORM_PENDING_CLOSE.lock() {
        for slot in map.values_mut() {
            let mut promise = *slot as *mut Promise;
            visitor.visit_raw_mut_ptr_slot(&mut promise);
            *slot = promise as usize;
        }
    }
    if let Ok(mut map) = transform::TRANSFORM_BACKPRESSURED_JOBS.lock() {
        for jobs in map.values_mut() {
            for slot in jobs.iter_mut() {
                let mut job = *slot as *mut ClosureHeader;
                visitor.visit_raw_mut_ptr_slot(&mut job);
                *slot = job as usize;
            }
        }
    }
}
