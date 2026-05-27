//! GC-managed opaque native handles for external native-library bindings.
//!
//! A native handle is a Perry heap value whose payload contains only native
//! metadata and a raw resource pointer. The GC treats the payload as a leaf:
//! the resource pointer is not a Perry heap edge and finalizers must be basic
//! native cleanup callbacks only.

use std::collections::hash_map::DefaultHasher;
use std::ffi::c_void;
use std::hash::{Hash, Hasher};
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};

const NATIVE_HANDLE_MAGIC: u64 = 0x5045_5252_5948_4e44; // "PERRYHND"
const DEBUG_NAME_CAP: usize = 64;

const OWNERSHIP_NULL: u8 = 0;
const OWNERSHIP_BORROWED: u8 = 1;
const OWNERSHIP_OWNED: u8 = 2;

const THREAD_ANY: u8 = 0;
const THREAD_MAIN: u8 = 1;
const THREAD_CREATOR: u8 = 2;

static MAIN_THREAD_ID: AtomicU64 = AtomicU64::new(0);

type NativeHandleFinalizer = unsafe extern "C" fn(*mut c_void, *mut c_void);

/// GC payload for a Perry native handle.
#[repr(C)]
pub struct NativeHandleHeader {
    pub magic: u64,
    pub type_id: u64,
    pub resource_ptr: *mut c_void,
    pub ownership: u8,
    pub nullable: u8,
    pub thread_affinity: u8,
    pub finalized: u8,
    pub _pad0: u32,
    pub creator_thread_id: u64,
    pub finalizer: *mut c_void,
    pub finalizer_hint: *mut c_void,
    pub debug_name_len: u16,
    pub _pad1: [u8; 6],
    pub debug_name: [u8; DEBUG_NAME_CAP],
}

fn current_thread_id() -> u64 {
    let mut hasher = DefaultHasher::new();
    std::thread::current().id().hash(&mut hasher);
    hasher.finish()
}

fn runtime_main_thread_id() -> u64 {
    let current = current_thread_id();
    match MAIN_THREAD_ID.compare_exchange(0, current, Ordering::AcqRel, Ordering::Acquire) {
        Ok(_) => current,
        Err(existing) => existing,
    }
}

#[cold]
fn throw_type_error(message: &str) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn type_id_from_parts(ptr: *const u8, len: usize) -> u64 {
    let bytes = if ptr.is_null() || len == 0 {
        b"handle"
    } else {
        unsafe { std::slice::from_raw_parts(ptr, len) }
    };
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn init_debug_name(dst: &mut [u8; DEBUG_NAME_CAP], ptr: *const u8, len: usize) -> u16 {
    dst.fill(0);
    let bytes = if ptr.is_null() || len == 0 {
        b"handle".as_slice()
    } else {
        unsafe { std::slice::from_raw_parts(ptr, len) }
    };
    let copy_len = bytes.len().min(DEBUG_NAME_CAP);
    dst[..copy_len].copy_from_slice(&bytes[..copy_len]);
    copy_len as u16
}

unsafe fn native_handle_new(
    resource_ptr: i64,
    type_id: i64,
    ownership: u8,
    nullable: i32,
    thread_affinity: i32,
    finalizer: *mut c_void,
    debug_name_ptr: *const u8,
    debug_name_len: i64,
) -> f64 {
    let ptr_value = resource_ptr as *mut c_void;
    let stored_ownership = if ptr_value.is_null() {
        OWNERSHIP_NULL
    } else {
        ownership
    };
    let handle = crate::gc::gc_malloc(
        std::mem::size_of::<NativeHandleHeader>(),
        crate::gc::GC_TYPE_NATIVE_HANDLE,
    ) as *mut NativeHandleHeader;
    (*handle).magic = NATIVE_HANDLE_MAGIC;
    (*handle).type_id = type_id as u64;
    (*handle).resource_ptr = ptr_value;
    (*handle).ownership = stored_ownership;
    (*handle).nullable = if nullable != 0 { 1 } else { 0 };
    (*handle).thread_affinity = match thread_affinity as u8 {
        THREAD_MAIN => THREAD_MAIN,
        THREAD_CREATOR => THREAD_CREATOR,
        _ => THREAD_ANY,
    };
    (*handle).finalized = 0;
    (*handle)._pad0 = 0;
    (*handle).creator_thread_id = current_thread_id();
    (*handle).finalizer = if stored_ownership == OWNERSHIP_OWNED {
        finalizer
    } else {
        ptr::null_mut()
    };
    (*handle).finalizer_hint = ptr::null_mut();
    (*handle).debug_name_len = init_debug_name(
        &mut (*handle).debug_name,
        debug_name_ptr,
        debug_name_len.max(0) as usize,
    );
    (*handle)._pad1 = [0; 6];
    f64::from_bits(crate::value::JSValue::pointer(handle as *const u8).bits())
}

unsafe fn handle_from_value(value: f64) -> *mut NativeHandleHeader {
    let js_value = crate::value::JSValue::from_bits(value.to_bits());
    if !js_value.is_pointer() {
        return ptr::null_mut();
    }
    let handle = js_value.as_pointer::<NativeHandleHeader>() as *mut NativeHandleHeader;
    if handle.is_null() || (handle as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return ptr::null_mut();
    }
    let gc_header =
        (handle as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    if !crate::gc::gc_malloc_header_is_tracked(gc_header) {
        return ptr::null_mut();
    }
    if (*gc_header).obj_type != crate::gc::GC_TYPE_NATIVE_HANDLE {
        return ptr::null_mut();
    }
    if (*handle).magic != NATIVE_HANDLE_MAGIC {
        return ptr::null_mut();
    }
    handle
}

unsafe fn finalize_once(handle: *mut NativeHandleHeader) -> bool {
    if handle.is_null() || (*handle).finalized != 0 {
        return false;
    }
    (*handle).finalized = 1;
    let should_finalize = (*handle).ownership == OWNERSHIP_OWNED
        && !(*handle).resource_ptr.is_null()
        && !(*handle).finalizer.is_null();
    if should_finalize {
        let finalizer: NativeHandleFinalizer = std::mem::transmute((*handle).finalizer);
        finalizer((*handle).resource_ptr, (*handle).finalizer_hint);
    }
    (*handle).resource_ptr = ptr::null_mut();
    (*handle).ownership = OWNERSHIP_NULL;
    should_finalize
}

unsafe fn validate_thread(handle: *const NativeHandleHeader) {
    match (*handle).thread_affinity {
        THREAD_ANY => {}
        THREAD_MAIN => {
            if current_thread_id() != runtime_main_thread_id() {
                throw_type_error("Native handle used from the wrong thread");
            }
        }
        THREAD_CREATOR => {
            if current_thread_id() != (*handle).creator_thread_id {
                throw_type_error("Native handle used from the wrong thread");
            }
        }
        _ => throw_type_error("Native handle has invalid thread affinity"),
    }
}

/// Mark the current runtime thread as the main thread for `thread: "main"`
/// validation. The first native-handle operation also initializes this lazily.
#[no_mangle]
pub extern "C" fn js_native_handle_mark_main_thread() {
    let current = current_thread_id();
    let _ = MAIN_THREAD_ID.compare_exchange(0, current, Ordering::AcqRel, Ordering::Acquire);
}

/// Stable FNV-1a type id for codegen and native tests.
#[no_mangle]
pub extern "C" fn js_native_handle_type_id(type_name_ptr: *const u8, type_name_len: usize) -> i64 {
    type_id_from_parts(type_name_ptr, type_name_len) as i64
}

/// Wrap an owned raw native resource pointer in an opaque Perry JS value.
#[no_mangle]
pub extern "C" fn js_native_handle_new_owned(
    resource_ptr: i64,
    type_id: i64,
    nullable: i32,
    thread_affinity: i32,
    finalizer: *mut c_void,
    debug_name_ptr: *const u8,
    debug_name_len: i64,
) -> f64 {
    runtime_main_thread_id();
    unsafe {
        native_handle_new(
            resource_ptr,
            type_id,
            OWNERSHIP_OWNED,
            nullable,
            thread_affinity,
            finalizer,
            debug_name_ptr,
            debug_name_len,
        )
    }
}

/// Wrap a borrowed raw native resource pointer in an opaque Perry JS value.
#[no_mangle]
pub extern "C" fn js_native_handle_new_borrowed(
    resource_ptr: i64,
    type_id: i64,
    nullable: i32,
    thread_affinity: i32,
    debug_name_ptr: *const u8,
    debug_name_len: i64,
) -> f64 {
    runtime_main_thread_id();
    unsafe {
        native_handle_new(
            resource_ptr,
            type_id,
            OWNERSHIP_BORROWED,
            nullable,
            thread_affinity,
            ptr::null_mut(),
            debug_name_ptr,
            debug_name_len,
        )
    }
}

/// Validate and unwrap a native handle argument to its raw resource pointer.
#[no_mangle]
pub extern "C" fn js_native_handle_unwrap(
    value: f64,
    expected_type_id: i64,
    nullable: i32,
    required_ownership: i32,
    thread_affinity: i32,
) -> i64 {
    runtime_main_thread_id();
    unsafe {
        let handle = handle_from_value(value);
        if handle.is_null() {
            throw_type_error("Expected a Perry native handle");
        }
        if (*handle).type_id != expected_type_id as u64 {
            throw_type_error("Native handle type mismatch");
        }
        if (*handle).finalized != 0 {
            throw_type_error("Native handle has been disposed");
        }
        let expected_thread = match thread_affinity as u8 {
            THREAD_MAIN => THREAD_MAIN,
            THREAD_CREATOR => THREAD_CREATOR,
            _ => THREAD_ANY,
        };
        if expected_thread != (*handle).thread_affinity {
            throw_type_error("Native handle thread-affinity mismatch");
        }
        validate_thread(handle);
        if (*handle).ownership == OWNERSHIP_NULL {
            if nullable != 0 && (*handle).nullable != 0 {
                return 0;
            }
            throw_type_error("Native handle is null");
        }
        match required_ownership as u8 {
            OWNERSHIP_OWNED if (*handle).ownership != OWNERSHIP_OWNED => {
                throw_type_error("Native handle ownership mismatch");
            }
            OWNERSHIP_BORROWED
                if (*handle).ownership != OWNERSHIP_BORROWED
                    && (*handle).ownership != OWNERSHIP_OWNED =>
            {
                throw_type_error("Native handle ownership mismatch");
            }
            OWNERSHIP_NULL => {}
            _ => {}
        }
        (*handle).resource_ptr as i64
    }
}

/// Explicitly dispose a native handle. Used by tests and future explicit
/// resource-management surfaces.
#[no_mangle]
pub extern "C" fn js_native_handle_dispose(value: f64) -> i32 {
    unsafe {
        let handle = handle_from_value(value);
        if handle.is_null() {
            return 0;
        }
        if finalize_once(handle) {
            1
        } else {
            (*handle).finalized = 1;
            (*handle).resource_ptr = ptr::null_mut();
            (*handle).ownership = OWNERSHIP_NULL;
            0
        }
    }
}

pub(crate) unsafe fn finalize_native_handle_for_gc(handle: *mut NativeHandleHeader) {
    let _ = finalize_once(handle);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::raw::c_int;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static FINALIZER_CALLS: AtomicUsize = AtomicUsize::new(0);
    static NULL_FINALIZER_CALLS: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn count_finalizer(_resource: *mut c_void, _hint: *mut c_void) {
        FINALIZER_CALLS.fetch_add(1, Ordering::SeqCst);
    }

    unsafe extern "C" fn count_null_finalizer(_resource: *mut c_void, _hint: *mut c_void) {
        NULL_FINALIZER_CALLS.fetch_add(1, Ordering::SeqCst);
    }

    fn catch_runtime_throw(f: impl FnOnce()) -> bool {
        let env = crate::exception::js_try_push();
        let jumped = unsafe { crate::ffi::setjmp::setjmp(env as *mut c_int) };
        if jumped == 0 {
            f();
            crate::exception::js_try_end();
            false
        } else {
            crate::exception::js_try_end();
            crate::exception::js_clear_exception();
            true
        }
    }

    fn type_id(name: &str) -> i64 {
        js_native_handle_type_id(name.as_ptr(), name.len())
    }

    fn borrowed(ptr: i64, name: &str) -> f64 {
        js_native_handle_new_borrowed(
            ptr,
            type_id(name),
            0,
            THREAD_ANY as i32,
            name.as_ptr(),
            name.len() as i64,
        )
    }

    #[test]
    fn numeric_and_non_handle_values_fail_unwrap() {
        assert!(catch_runtime_throw(|| {
            js_native_handle_unwrap(
                42.0,
                type_id("Thing"),
                0,
                OWNERSHIP_BORROWED as i32,
                THREAD_ANY as i32,
            );
        }));
        let object = crate::object::js_object_alloc(0, 0);
        let boxed = crate::value::js_nanbox_pointer(object as i64);
        assert!(catch_runtime_throw(|| {
            js_native_handle_unwrap(
                boxed,
                type_id("Thing"),
                0,
                OWNERSHIP_BORROWED as i32,
                THREAD_ANY as i32,
            );
        }));
        let forged = crate::value::js_nanbox_pointer(0x1234_5678);
        assert!(catch_runtime_throw(|| {
            js_native_handle_unwrap(
                forged,
                type_id("Thing"),
                0,
                OWNERSHIP_BORROWED as i32,
                THREAD_ANY as i32,
            );
        }));
    }

    #[test]
    fn wrong_handle_type_fails_unwrap() {
        let value = borrowed(0x1234, "A");
        assert!(catch_runtime_throw(|| {
            js_native_handle_unwrap(
                value,
                type_id("B"),
                0,
                OWNERSHIP_BORROWED as i32,
                THREAD_ANY as i32,
            );
        }));
    }

    #[test]
    fn owned_finalizer_runs_once_across_dispose_and_gc() {
        FINALIZER_CALLS.store(0, Ordering::SeqCst);
        let value = js_native_handle_new_owned(
            0x1234,
            type_id("Owned"),
            0,
            THREAD_ANY as i32,
            count_finalizer as *mut c_void,
            b"Owned".as_ptr(),
            5,
        );
        assert_eq!(js_native_handle_dispose(value), 1);
        assert_eq!(js_native_handle_dispose(value), 0);
        unsafe {
            let handle = handle_from_value(value);
            finalize_native_handle_for_gc(handle);
        }
        assert_eq!(FINALIZER_CALLS.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn borrowed_and_null_handles_never_run_finalizers() {
        NULL_FINALIZER_CALLS.store(0, Ordering::SeqCst);
        let borrowed = js_native_handle_new_borrowed(
            0x1234,
            type_id("Borrowed"),
            0,
            THREAD_ANY as i32,
            b"Borrowed".as_ptr(),
            8,
        );
        assert_eq!(js_native_handle_dispose(borrowed), 0);
        let null_owned = js_native_handle_new_owned(
            0,
            type_id("Nullable"),
            1,
            THREAD_ANY as i32,
            count_null_finalizer as *mut c_void,
            b"Nullable".as_ptr(),
            8,
        );
        unsafe {
            finalize_native_handle_for_gc(handle_from_value(null_owned));
        }
        assert_eq!(NULL_FINALIZER_CALLS.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn nullable_descriptors_unwrap_null_to_zero() {
        let value = js_native_handle_new_borrowed(
            0,
            type_id("Maybe"),
            1,
            THREAD_ANY as i32,
            b"Maybe".as_ptr(),
            5,
        );
        assert_eq!(
            js_native_handle_unwrap(
                value,
                type_id("Maybe"),
                1,
                OWNERSHIP_BORROWED as i32,
                THREAD_ANY as i32,
            ),
            0
        );
        assert!(catch_runtime_throw(|| {
            js_native_handle_unwrap(
                value,
                type_id("Maybe"),
                0,
                OWNERSHIP_BORROWED as i32,
                THREAD_ANY as i32,
            );
        }));
    }

    #[test]
    fn handle_payload_is_gc_leaf() {
        let value = borrowed(0x1234, "Leaf");
        unsafe {
            let handle = handle_from_value(value);
            let gc =
                (handle as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            assert_eq!((*gc).obj_type, crate::gc::GC_TYPE_NATIVE_HANDLE);
            assert!(crate::gc::gc_type_is_pointer_free((*gc).obj_type));
            assert!(!crate::gc::gc_type_is_movable((*gc).obj_type));
        }
    }

    #[test]
    fn thread_affinity_rejects_wrong_thread() {
        let value = js_native_handle_new_borrowed(
            0x1234,
            type_id("Threaded"),
            0,
            THREAD_CREATOR as i32,
            b"Threaded".as_ptr(),
            8,
        );
        let bits = value.to_bits();
        let expected = type_id("Threaded");
        let joined = std::thread::spawn(move || {
            let value = f64::from_bits(bits);
            catch_runtime_throw(|| {
                js_native_handle_unwrap(
                    value,
                    expected,
                    0,
                    OWNERSHIP_BORROWED as i32,
                    THREAD_CREATOR as i32,
                );
            })
        })
        .join()
        .expect("thread join");
        assert!(joined);
    }
}
