use super::*;
use crate::gc::GC_TYPE_BUFFER_ARRAY_BUFFER as ARRAY_BUFFER;
use crate::value::JSValue;

#[test]
fn placement_capacity_and_native_output_copies() {
    let _lock = crate::gc::global_side_table_test_lock();
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    let _policy = crate::gc::ByteStorePolicyTestGuard::new(256);
    let scope = crate::gc::RuntimeHandleScope::new();
    for len in [0, 64, 256, 257, 4096] {
        let body: Vec<u8> = (0..len).map(|i| (i * 31) as u8).collect();
        let ptr = store_alloc(ARRAY_BUFFER, len as u32, Init::Copy(&body));
        let root = scope.root_raw_mut_ptr(ptr);
        assert_eq!(
            super::super::header::has_owned_backing(ptr as usize),
            len > 256
        );
        let collections = crate::gc::byte_store_test_collection_count();
        crate::gc::js_gc_collect();
        assert!(crate::gc::byte_store_test_collection_count() > collections);
        let value = f64::from_bits(
            JSValue::pointer(root.get_raw_mut_ptr::<super::super::BufferHeader>().cast()).bits(),
        );
        super::super::bytes::no_gc(|proof| {
            assert_eq!(super::super::bytes::bytes(value, proof).unwrap(), body)
        });
    }
}

#[test]
fn large_concat_uses_native_store_and_preserves_buffer_identity() {
    let _lock = crate::gc::global_side_table_test_lock();
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    let scope = crate::gc::RuntimeHandleScope::new();
    let len = 5 * 1024 * 1024;
    let part = super::super::js_buffer_alloc(len, 17);
    let part_root = scope.root_raw_mut_ptr(part);
    let array = crate::array::js_array_alloc(1);
    let array_root = scope.root_raw_mut_ptr(array);
    crate::array::js_array_push(array, JSValue::pointer(part.cast()));
    let count = super::super::backing::LIVE_BACKINGS.load(std::sync::atomic::Ordering::SeqCst);
    let concat = super::super::js_buffer_concat(array_root.get_raw_mut_ptr());
    let concat_root = scope.root_raw_mut_ptr(concat);
    assert_eq!(
        super::super::backing::LIVE_BACKINGS.load(std::sync::atomic::Ordering::SeqCst),
        count + 1,
        "concat must create one Native store"
    );
    assert!(
        super::super::header::has_owned_backing(concat as usize),
        "large concat must be Native"
    );
    let owner = super::super::buffer_backing_array_buffer(concat as usize);
    let view = super::super::view::alloc(concat, 1, len as u32 - 1);
    let view_root = scope.root_raw_mut_ptr(view);
    assert_eq!(
        super::super::buffer_backing_array_buffer(view as usize),
        owner
    );
    let collections = crate::gc::byte_store_test_collection_count();
    crate::gc::js_gc_collect();
    assert!(crate::gc::byte_store_test_collection_count() > collections);
    assert_eq!(
        unsafe { *super::super::buffer_data(view_root.get_raw_mut_ptr()) },
        17
    );
    assert_eq!(
        unsafe { (*concat_root.get_raw_mut_ptr::<super::super::BufferHeader>()).length },
        len as u32
    );
    assert!(!part_root
        .get_raw_mut_ptr::<super::super::BufferHeader>()
        .is_null());
}

#[test]
fn every_typed_owner_uses_native_placement_and_header_length() {
    let _lock = crate::gc::global_side_table_test_lock();
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    let scope = crate::gc::RuntimeHandleScope::new();
    for kind in 0..12 {
        let typed = crate::typedarray::typed_array_alloc(kind, 8192);
        let root = scope.root_raw_mut_ptr(typed);
        assert!(
            super::super::header::has_owned_backing(typed as usize),
            "kind {kind} must be Native"
        );
        assert_eq!(unsafe { length(typed as usize) }, 8192);
        let value = crate::value::js_nanbox_pointer(typed as i64);
        super::super::bytes::no_gc(|proof| unsafe {
            let body = super::super::bytes::bytes_mut(value, proof).unwrap();
            assert!(body.iter().all(|&b| b == 0));
            *body.last_mut().unwrap() = 91;
        });
        crate::gc::js_gc_collect();
        let typed = root.get_raw_mut_ptr::<super::super::BufferHeader>();
        assert_eq!(unsafe { length(typed as usize) }, 8192);
        super::super::bytes::no_gc(|proof| {
            let body =
                super::super::bytes::bytes(crate::value::js_nanbox_pointer(typed as i64), proof)
                    .unwrap();
            assert_eq!(body[body.len() - 1], 91);
        });
    }
}

#[test]
fn native_birth_accounts_capacity_without_collecting_and_release_balances_it() {
    let _lock = crate::gc::global_side_table_test_lock();
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    let _policy = crate::gc::ByteStorePolicyTestGuard::new(256);
    // Cross the external allocation step with retained capacity, even though
    // the visible body is small. A collecting birth hook must turn this red.
    let mut body = Vec::with_capacity(16 * 1024 * 1024);
    body.resize(257, 37);
    let capacity = body.capacity();
    let original = body.as_ptr();
    let live = crate::gc::byte_store_test_external_live_bytes();
    let collections = crate::gc::byte_store_test_collection_count();
    let ptr = store_alloc(ARRAY_BUFFER, body.len() as u32, Init::AdoptVec(body));
    assert_eq!(
        crate::gc::byte_store_test_collection_count(),
        collections,
        "birth must not collect"
    );
    assert_eq!(
        crate::gc::byte_store_test_external_live_bytes(),
        live + capacity
    );
    let value = f64::from_bits(JSValue::pointer(ptr.cast()).bits());
    let pin = super::super::bytes::pin(value).unwrap();
    assert_eq!(pin.as_ptr(), original);
    super::super::detach_array_buffer(ptr as usize);
    assert_eq!(
        crate::gc::byte_store_test_external_live_bytes(),
        live + capacity
    );
    assert_eq!(unsafe { *pin.as_ptr() }, 37);
    drop(pin);
    assert_eq!(crate::gc::byte_store_test_external_live_bytes(), live);
}

#[test]
fn uint8_owner_is_native_and_detach_reads_header_length() {
    let _lock = crate::gc::global_side_table_test_lock();
    let ptr = super::super::js_uint8array_alloc(131072);
    assert!(super::super::header::has_owned_backing(ptr as usize));
    assert_eq!(unsafe { length(ptr as usize) }, 131072);
    let value = crate::value::js_nanbox_pointer(ptr as i64);
    super::super::bytes::no_gc(|proof| unsafe {
        let body = super::super::bytes::bytes_mut(value, proof).unwrap();
        body[131071] = 23;
    });
    let owner = super::super::ensure_buffer_ab_alias(ptr as usize);
    super::super::detach_array_buffer(owner);
    assert_eq!(unsafe { length(ptr as usize) }, 0);
    assert!(super::super::bytes::no_gc(|proof| {
        super::super::bytes::bytes(value, proof).is_err()
    }));
}

#[test]
fn collecting_during_native_birth_turns_the_birth_witness_red() {
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "buffer::store::tests::native_birth_accounts_capacity_without_collecting_and_release_balances_it", "--nocapture"])
        .env("PERRY_B4_SABOTAGE", "birth_collect").output().unwrap();
    assert!(String::from_utf8_lossy(&child.stdout).contains("running 1 test"));
    assert!(
        !child.status.success(),
        "collection inside birth must be detected"
    );
}

#[test]
fn generic_byte_copy_has_no_buffer_pool_provenance() {
    let _lock = crate::gc::global_side_table_test_lock();
    let copy = store_alloc(crate::gc::GC_TYPE_BUFFER, 3, Init::Copy(b"abc"));
    assert!(!unsafe { super::super::store::is_view(copy as usize) });
    let (owned_value, _pin) = super::super::bytes::new_bytes(
        super::super::bytes::Brand::Buffer,
        3,
        super::super::bytes::Init::Uninit,
    );
    let addr = JSValue::from_bits(owned_value.to_bits()).as_pointer::<u8>() as usize;
    assert!(!unsafe { super::super::store::is_view(addr) });
    let (pooled_value, _pooled_pin) = super::super::bytes::new_bytes(
        super::super::bytes::Brand::Buffer,
        3,
        super::super::bytes::Init::PoolCopy,
    );
    let addr = JSValue::from_bits(pooled_value.to_bits()).as_pointer::<u8>() as usize;
    assert!(unsafe { super::super::store::is_view(addr) });
}

#[test]
fn native_typed_transfer_preserves_owner_byte_extent() {
    let _lock = crate::gc::global_side_table_test_lock();
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    let scope = crate::gc::RuntimeHandleScope::new();
    for kind in [
        crate::typedarray::KIND_FLOAT64,
        crate::typedarray::KIND_INT32,
    ] {
        let typed = crate::typedarray::typed_array_alloc(kind, 8192);
        let root = scope.root_raw_mut_ptr(typed);
        let byte_len = 8192 * crate::typedarray::elem_size_for_kind(kind);
        let original = unsafe { data(typed as usize) };
        assert!(super::super::header::has_owned_backing(typed as usize));
        unsafe {
            *original = 37;
            *original.add(byte_len - 1) = 91;
            let ab = crate::typedarray_view::js_typed_array_backing_buffer(typed);
            let ab_root = scope.root_raw_mut_ptr(ab);
            let message = crate::thread::serialize_message(
                JSValue::pointer(root.get_raw_mut_ptr::<u8>()).bits(),
                &[ab_root.get_raw_mut_ptr::<u8>() as usize],
                None,
            )
            .unwrap();
            assert_eq!(length(root.get_raw_mut_ptr::<u8>() as usize), 0);
            let received = scope.root_nanbox_u64(
                crate::thread::deserialize_nanbox_on_current_thread(&message),
            );
            drop(message);
            crate::gc::js_gc_collect();
            let addr = JSValue::from_bits(received.get_nanbox_u64()).as_pointer::<u8>() as usize;
            assert_eq!(length(addr), 8192);
            assert_eq!(owner_byte_length(owner(addr)), byte_len);
            assert_eq!(
                data(addr),
                original,
                "Native transfer must preserve the allocation"
            );
            assert_eq!(*data(addr), 37);
            assert_eq!(*data(addr).add(byte_len - 1), 91);
        }
    }
}

#[test]
fn structured_clone_copies_native_typed_owners_before_detach() {
    let _lock = crate::gc::global_side_table_test_lock();
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    let scope = crate::gc::RuntimeHandleScope::new();
    for kind in [
        crate::typedarray::KIND_FLOAT64,
        crate::typedarray::KIND_INT32,
    ] {
        let typed = crate::typedarray::typed_array_alloc(kind, 8192);
        let root = scope.root_raw_mut_ptr(typed);
        let byte_len = 8192 * crate::typedarray::elem_size_for_kind(kind);
        unsafe {
            *data(typed as usize) = 37;
            *data(typed as usize).add(byte_len - 1) = 91;
            let clone = scope.root_nanbox_f64(crate::builtins::js_structured_clone(
                crate::value::js_nanbox_pointer(typed as i64),
            ));
            let addr =
                JSValue::from_bits(clone.get_nanbox_f64().to_bits()).as_pointer::<u8>() as usize;
            assert_ne!(
                addr, typed as usize,
                "structuredClone must make a new typed owner"
            );
            assert!(super::super::header::has_owned_backing(addr));
            let ab = crate::typedarray_view::js_typed_array_backing_buffer(root.get_raw_mut_ptr());
            super::super::detach_array_buffer(ab as usize);
            crate::gc::js_gc_collect();
            let addr =
                JSValue::from_bits(clone.get_nanbox_f64().to_bits()).as_pointer::<u8>() as usize;
            assert_eq!(length(root.get_raw_mut_ptr::<u8>() as usize), 0);
            assert_eq!(length(addr), 8192);
            assert_eq!(owner_byte_length(addr), byte_len);
            assert_eq!(*data(addr), 37);
            assert_eq!(*data(addr).add(byte_len - 1), 91);
        }
    }
}
