//! B1's live witnesses and one child-process sabotage per contract.
use super::super::*;
use super::support::*;
use crate::buffer::{
    self,
    bytes::{self, Brand, Init},
};
use crate::value::JSValue;
use std::sync::atomic::Ordering;
fn bits(p: *const buffer::BufferHeader) -> f64 {
    f64::from_bits(ptr_bits(p as usize))
}
fn guard(slots: u32) -> CopyingNurseryTestGuard {
    let guard = CopyingNurseryTestGuard::new(slots);
    gc_register_named_mutable_root_scanner("pinned", crate::gc::pin::scan_pinned_object_roots_mut);
    guard
}

#[test]
fn pinned_native_call_survives_a_moving_collection() {
    let _guard = guard(1);
    let _force = ForcedEvacuationTestGuard::on();
    let (value, pin) = bytes::new_bytes(Brand::Buffer, 1024, Init::Zero);
    unsafe {
        std::ptr::write_bytes(pin.as_mut_ptr(), 37, pin.len());
    }
    let owner = JSValue::from_bits(value.to_bits()).as_pointer::<u8>();
    let young = young_leaf();
    js_shadow_slot_set(0, string_bits(young));
    let before = gc_total_collection_count();
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_ne!(
        js_shadow_slot_get(0) & crate::value::POINTER_MASK,
        young as u64,
        "a live young object must really move during the pinned call"
    );
    assert!(gc_total_collection_count() > before);
    // Full marking must find the byte owner solely through its pin.
    clear_marks();
    clear_mark_seeds();
    let valid = build_valid_pointer_set();
    mark_mutable_registered_roots(&valid);
    assert_marked_user_ptr(owner as usize, "byte owner rooted by pin");
    clear_marks();
    clear_mark_seeds();
    unsafe {
        assert!(std::slice::from_raw_parts(pin.as_ptr(), pin.len())
            .iter()
            .all(|b| *b == 37));
    }
    assert_eq!(bytes::pin(value).unwrap().as_ptr(), pin.as_ptr());
}

#[test]
fn detach_defers_native_free_until_the_last_pin() {
    let _guard = guard(0);
    let before = buffer::LIVE_BACKINGS.load(Ordering::SeqCst);
    let buffer = buffer::js_array_buffer_new(1024 * 1024);
    let first = bytes::pin(bits(buffer)).unwrap();
    let second = bytes::pin(bits(buffer)).unwrap();
    unsafe {
        *first.as_mut_ptr() = 91;
    }
    buffer::detach_array_buffer(buffer as usize);
    assert!(buffer::is_detached_buffer(buffer as usize));
    assert!(bytes::no_gc(
        |scope| bytes::bytes(bits(buffer), scope).is_err()
    ));
    assert_eq!(
        buffer::LIVE_BACKINGS.load(Ordering::SeqCst),
        before + 1,
        "detach must retain the allocation while native code owns pins"
    );
    drop(first);
    assert_eq!(buffer::LIVE_BACKINGS.load(Ordering::SeqCst), before + 1);
    unsafe {
        assert_eq!(*second.as_ptr(), 91);
    }
    drop(second);
    assert_eq!(buffer::LIVE_BACKINGS.load(Ordering::SeqCst), before);
}

#[test]
fn native_backed_alloc_buffer_consumer_preserves_the_pointer_word() {
    let _guard = guard(0);
    let input = [0x25_u8; 8];
    let _native = bytes::NativeCopyTestGuard::new();
    // This is the same C ABI consumer that perry-ffi::alloc_buffer calls.
    // A test-only allocator fixture selects native storage without changing B3.
    let value = unsafe {
        crate::native_payload_abi::js_perry_bytes_copy(
            Brand::Buffer as u32,
            input.as_ptr(),
            input.len(),
        )
    };
    let cell = JSValue::from_bits(value.to_bits()).as_pointer::<buffer::BufferHeader>();
    assert!(buffer::is_foreign_backed_buffer(cell as usize));
    let ptr_word = unsafe { buffer::store::raw_link(cell as usize) };
    assert_ne!(
        ptr_word,
        u64::from_ne_bytes(input) as usize,
        "payload must never overwrite the traced link word"
    );
    bytes::no_gc(|scope| assert_eq!(bytes::bytes(value, scope).unwrap(), &input));
}

#[test]
fn every_current_byte_placement_and_view_resolves_the_canonical_window() {
    let _guard = guard(2);
    let ab = buffer::js_array_buffer_new(16);
    {
        buffer::js_buffer_set(ab, 4, 77);
    }
    let u8view = buffer::js_buffer_slice(ab, 4, 8);
    let dv = buffer::store::new_view(GC_TYPE_BUFFER_DATA_VIEW, ab as usize, 4, 4, false);
    let ta = crate::typedarray_view::js_typed_array_view(
        crate::typedarray::KIND_INT32 as i32,
        bits(ab),
        4.0,
        1.0,
    );
    let mut foreign = [77, 0, 0, 0];
    let foreign_cell = buffer::buffer_alloc_foreign(foreign.as_mut_ptr(), 4);
    let shared = crate::shared_sab::alloc_shared_sab(4);
    {
        buffer::js_buffer_set(shared, 0, 77);
    }
    let inline = bytes::from_slice(Brand::Buffer, &foreign);
    for value in [
        bits(u8view),
        bits(dv),
        bits(foreign_cell),
        bits(shared),
        inline,
        f64::from_bits(ptr_bits(ta as usize)),
    ] {
        bytes::no_gc(|scope| assert_eq!(bytes::bytes(value, scope).unwrap(), &foreign));
        if value.to_bits() == bits(foreign_cell).to_bits() {
            assert!(matches!(
                bytes::pin(value),
                Err(bytes::NotBytes::UnstableForeign)
            ));
            continue;
        }
        let pin = bytes::pin(value).unwrap();
        assert_eq!(pin.len(), 4);
        unsafe {
            assert_eq!(*pin.as_ptr(), 77);
        }
    }
    let inline_ta = crate::typedarray::typed_array_alloc(crate::typedarray::KIND_INT16, 3);
    let value = f64::from_bits(ptr_bits(inline_ta as usize));
    bytes::no_gc(|scope| assert_eq!(bytes::bytes(value, scope).unwrap().len(), 6));
    let pin = bytes::pin(value).unwrap();
    let header = unsafe { (inline_ta as *const u8).sub(GC_HEADER_SIZE) as *mut GcHeader };
    assert!(!unsafe { crate::gc::pin::pin_constrains_copying_minor_for_tests(header) });
    assert_eq!(pin.len(), 6);
}

#[test]
fn no_gc_byte_allocation_is_rejected() {
    if std::env::var_os("PERRY_B1_NO_GC_PROBE").is_some() {
        bytes::no_gc(|_| {
            bytes::from_slice(Brand::Buffer, b"allocation");
        });
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "gc::tests::buffer_bytes::no_gc_byte_allocation_is_rejected",
            "--nocapture",
        ])
        .env("PERRY_B1_NO_GC_PROBE", "1")
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "debug no_gc allocation guard must reject allocation"
    );
}

#[test]
fn process_shared_pins_leave_the_global_header_read_only() {
    let _guard = guard(0);
    let shared = crate::shared_sab::alloc_shared_sab(32);
    let shared = crate::shared_sab::shared_store_owner(shared as usize).unwrap()
        as *mut buffer::BufferHeader;
    let value = bits(shared);
    let header = unsafe { crate::gc::header_from_trusted_user_ptr(shared.cast()) };
    let before = unsafe { ((*header).gc_flags, (*header)._reserved) };
    let pin = bytes::pin(value).unwrap();
    assert_eq!(
        unsafe { ((*header).gc_flags, (*header)._reserved) },
        before,
        "a process-global SAB header must not carry mutable byte-pin state"
    );
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let threads: Vec<_> = (0..2)
        .map(|_| {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                for _ in 0..128 {
                    let pin = bytes::pin(value).unwrap();
                    assert_eq!(pin.len(), 32);
                    unsafe { assert_eq!(*pin.as_ptr(), 0) };
                }
            })
        })
        .collect();
    barrier.wait();
    for thread in threads {
        thread.join().unwrap();
    }
    drop(pin);
    assert_eq!(unsafe { ((*header).gc_flags, (*header)._reserved) }, before);
}

#[test]
fn each_b1_sabotage_turns_its_live_witness_red() {
    for (fault, witness) in [
        (
            "owner_root",
            "pinned_native_call_survives_a_moving_collection",
        ),
        (
            "detach_free",
            "detach_defers_native_free_until_the_last_pin",
        ),
        (
            "inline_copy",
            "native_backed_alloc_buffer_consumer_preserves_the_pointer_word",
        ),
        ("no_gc_assert", "no_gc_byte_allocation_is_rejected"),
        ("arena_free", "native_arena_dispose_defers_free_until_unpin"),
        (
            "shared_pin_write",
            "process_shared_pins_leave_the_global_header_read_only",
        ),
        (
            "view_window",
            "every_current_byte_placement_and_view_resolves_the_canonical_window",
        ),
        (
            "tls_output",
            "tls::b1_output_tests::certificate_raw_output_preserves_der",
        ),
    ] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                &if witness.contains("::") {
                    witness.to_string()
                } else {
                    format!("gc::tests::buffer_bytes::{witness}")
                },
                "--nocapture",
            ])
            .env("PERRY_B1_SABOTAGE", fault)
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("running 1 test"),
            "sabotage must run its named witness"
        );
        assert!(
            !output.status.success(),
            "sabotage {fault} left {witness} green"
        );
        eprintln!("B1 sabotage {fault}: RED ({})", output.status);
    }
}

#[test]
fn native_arena_dispose_defers_free_until_unpin() {
    let _guard = guard(0);
    let owner = crate::native_arena::js_native_arena_alloc(16);
    let view = crate::native_arena::js_native_arena_view(
        owner as u64,
        crate::typedarray::KIND_UINT32 as i32,
        4,
        2,
    );
    let value = f64::from_bits(ptr_bits(view as usize));
    let pin = bytes::pin(value).unwrap();
    unsafe {
        *pin.as_mut_ptr() = 62;
    }
    crate::native_arena::js_native_arena_dispose(owner as u64);
    assert!(buffer::is_detached_buffer(owner as usize));
    assert!(
        !unsafe { buffer::store::owner_data(owner as usize) }.is_null(),
        "dispose freed pinned native arena bytes"
    );
    unsafe {
        assert_eq!(*pin.as_ptr(), 62);
    }
    drop(pin);
    assert!(
        unsafe { buffer::store::owner_data(owner as usize) }.is_null(),
        "last unpin must pay the deferred release"
    );
}
