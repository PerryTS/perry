//! Load-bearing tests for native-call receiver classification.
//!
//! A result-only test would stay green if every call went back through the
//! Buffer and typed-array registries. These tests assert the registry-counter
//! deltas as well as the dispatch answer, then change the receiver kind at the
//! same feedback site so a cache that fails to revalidate goes wrong.

use super::*;

const SITE: u64 = 0x4E41_5449_5645_0001;

fn boxed(ptr: usize) -> f64 {
    f64::from_bits(JSValue::pointer(ptr as *mut u8).bits())
}

unsafe fn call_at(site: u64, receiver: f64, name: &[u8], args: &[f64]) -> f64 {
    js_native_call_method_at_site(
        site,
        receiver,
        name.as_ptr() as *const i8,
        name.len(),
        args.as_ptr(),
        args.len(),
    )
}

fn result_string(value: f64) -> String {
    let jv = JSValue::from_bits(value.to_bits());
    assert!(
        jv.is_any_string(),
        "expected string result, got {:#x}",
        value.to_bits()
    );
    let ptr = crate::value::js_get_string_pointer_unified(value) as *const crate::StringHeader;
    crate::string::string_as_str(ptr).to_owned()
}

/// Sabotage case one: with both registries armed, an ordinary shaped object at
/// a warmed typed-feedback site must cause zero Buffer and typed-array registry
/// probes. Removing the header/class gate makes either counter move.
#[test]
fn cached_plain_object_receiver_probes_zero_buffer_registries() {
    let _buffer = crate::buffer::js_buffer_alloc(1, 0);
    let _typed =
        crate::typedarray::js_typed_array_new_empty(crate::typedarray::KIND_INT16 as i32, 1);
    let object = crate::object::js_object_alloc(0, 1);
    let buffer_key = crate::string::js_string_from_bytes(b"buffer".as_ptr(), 6);
    crate::object::js_object_set_field_by_name(object, buffer_key, 17.0);
    let receiver = boxed(object as usize);

    assert_eq!(
        result_string(unsafe { call_at(SITE, receiver, b"toString", &[]) }),
        "[object Object]",
        "an own property named `buffer` must not change the receiver brand"
    );
    let buffer_before = crate::buffer::test_buffer_registry_probe_count();
    let typed_before = crate::typedarray::test_typed_array_registry_probe_count();
    let cache_before = test_native_receiver_site_cache_hits();

    assert_eq!(
        result_string(unsafe { call_at(SITE, receiver, b"toString", &[]) }),
        "[object Object]"
    );
    assert_eq!(
        crate::buffer::test_buffer_registry_probe_count(),
        buffer_before,
        "a cached plain-object receiver must enter no Buffer registry"
    );
    assert_eq!(
        crate::typedarray::test_typed_array_registry_probe_count(),
        typed_before,
        "a cached plain-object receiver must enter no typed-array registry"
    );
    assert!(
        test_native_receiver_site_cache_hits() > cache_before,
        "test premise: the measured call must hit the per-site kind cache"
    );
}

#[test]
fn primitive_receiver_tag_skips_byte_storage_registries() {
    let _buffer = crate::buffer::js_buffer_alloc(1, 0);
    let _typed =
        crate::typedarray::js_typed_array_new_empty(crate::typedarray::KIND_INT16 as i32, 1);
    let string = crate::string::js_string_from_bytes(b"tagged".as_ptr(), 6);
    let receiver = f64::from_bits(JSValue::string_ptr(string).bits());
    let buffer_before = crate::buffer::test_buffer_registry_probe_count();
    let typed_before = crate::typedarray::test_typed_array_registry_probe_count();

    assert_eq!(
        result_string(unsafe { call_at(SITE + 7, receiver, b"toString", &[]) }),
        "tagged"
    );
    assert_eq!(
        crate::buffer::test_buffer_registry_probe_count(),
        buffer_before
    );
    assert_eq!(
        crate::typedarray::test_typed_array_registry_probe_count(),
        typed_before
    );
}

/// Sabotage case two: replace the plain receiver with a real `Buffer.from`
/// result at the SAME site. A cache keyed only by site returns Object here and
/// produces `[object Object]`; revalidation by GC type routes Buffer.toString.
#[test]
fn cached_site_revalidates_when_plain_receiver_becomes_buffer() {
    let object = crate::object::js_object_alloc(0, 0);
    let receiver = boxed(object as usize);
    let _ = unsafe { call_at(SITE + 1, receiver, b"toString", &[]) };

    let source = crate::string::js_string_from_bytes(b"A".as_ptr(), 1);
    let buffer = crate::buffer::js_buffer_from_string(source, 0);
    let buffer_probes = crate::buffer::test_buffer_registry_probe_count();
    let typed_probes = crate::typedarray::test_typed_array_registry_probe_count();
    assert_eq!(
        result_string(unsafe { call_at(SITE + 1, boxed(buffer as usize), b"toString", &[]) }),
        "A",
        "the changed receiver kind must take Buffer dispatch"
    );
    assert_eq!(
        crate::buffer::test_buffer_registry_probe_count(),
        buffer_probes,
        "a managed Buffer is identified by GC_TYPE_BUFFER, not its registry"
    );
    assert_eq!(
        crate::typedarray::test_typed_array_registry_probe_count(),
        typed_probes,
        "Buffer classification must not fall through to the typed-array registry"
    );
}

/// The three managed byte-storage brands are all `GC_TYPE_BUFFER` cells; their
/// finer distinction remains inside buffer dispatch, where method semantics
/// need it. Receiver classification must not flatten those answers.
#[test]
fn buffer_uint8array_and_arraybuffer_keep_their_method_paths() {
    let uint8 = crate::buffer::js_uint8array_new(2.0);
    assert!(crate::buffer::is_uint8array_buffer(uint8 as usize));
    assert_eq!(
        unsafe { call_at(SITE + 5, boxed(uint8 as usize), b"length", &[]) },
        2.0,
        "a Uint8Array-branded Buffer cell must retain Uint8Array dispatch"
    );

    let array_buffer = crate::buffer::js_buffer_alloc(3, 9);
    crate::buffer::mark_as_array_buffer(array_buffer as usize);
    let sliced = unsafe { call_at(SITE + 6, boxed(array_buffer as usize), b"slice", &[]) };
    let sliced_addr = JSValue::from_bits(sliced.to_bits()).as_pointer::<u8>() as usize;
    assert!(
        crate::buffer::is_array_buffer(sliced_addr),
        "ArrayBuffer.prototype.slice must still return an ArrayBuffer"
    );
}

/// The residual headerless cases still use their authoritative registries.
/// This covers an embedder-owned external Buffer and a typed-array view over a
/// process-global SharedArrayBuffer backing while pinning their old dispatch.
#[test]
fn external_buffer_and_sab_backed_view_keep_native_dispatch() {
    let layout = std::alloc::Layout::from_size_align(
        std::mem::size_of::<crate::buffer::BufferHeader>() + 1,
        8,
    )
    .unwrap();
    let external = unsafe { std::alloc::alloc_zeroed(layout) };
    assert!(!external.is_null());
    let header = external as *mut crate::buffer::BufferHeader;
    unsafe {
        (*header).length = 1;
        (*header).capacity = 1;
        *external.add(std::mem::size_of::<crate::buffer::BufferHeader>()) = b'X';
    }
    crate::buffer::js_buffer_register_external(header as usize);
    assert_eq!(
        result_string(unsafe { call_at(SITE + 2, boxed(header as usize), b"toString", &[]) }),
        "X",
        "an external headerless Buffer must still dispatch through its registry"
    );
    // The external allocation is intentionally leaked: registration is
    // process-global and exposes no unregister operation.

    let sab = crate::shared_sab::alloc_shared_sab(4);
    unsafe { *crate::buffer::buffer_data_mut(sab) = 7 };
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    let view = crate::typedarray_view::js_typed_array_view(
        crate::typedarray::KIND_INT32 as i32,
        boxed(sab as usize),
        0.0,
        undefined,
    );
    assert_eq!(
        unsafe { call_at(SITE + 3, boxed(view as usize), b"length", &[]) },
        1.0,
        "a typed-array view backed by a SharedArrayBuffer must keep typed-array dispatch"
    );
}

extern "C" fn patched_to_string(_closure: *const crate::closure::ClosureHeader) -> f64 {
    let s = crate::string::js_string_from_bytes(b"patched".as_ptr(), 7);
    crate::value::js_nanbox_string(s as i64)
}

/// Receiver-kind caching must not cache prototype state. Reassigning the
/// prototype after the site is warm has to affect the very next call.
#[test]
fn cached_receiver_kind_does_not_hide_reassigned_prototype() {
    let object = crate::object::js_object_alloc(0, 0);
    let receiver = boxed(object as usize);
    assert_eq!(
        result_string(unsafe { call_at(SITE + 4, receiver, b"toString", &[]) }),
        "[object Object]"
    );

    let proto = crate::object::js_object_alloc(0, 1);
    let closure = crate::closure::js_closure_alloc(patched_to_string as *const u8, 0);
    crate::closure::js_register_closure_arity(patched_to_string as *const u8, 0);
    let key = crate::string::js_string_from_bytes(b"toString".as_ptr(), 8);
    crate::object::js_object_set_field_by_name(
        proto,
        key,
        crate::value::js_nanbox_pointer(closure as i64),
    );
    crate::object::object_ops::js_object_set_prototype_of(receiver, boxed(proto as usize));

    assert_eq!(
        result_string(unsafe { call_at(SITE + 4, receiver, b"toString", &[]) }),
        "patched",
        "the kind cache must not cache a receiver's prototype resolution"
    );
}
