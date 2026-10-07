//! The byte-cell admission (`header::byte_cell_type`) against a word that is
//! not a GC cell but whose preceding word starts with a byte-family type byte.
//!
//! A pointer-tagged native value can point at such memory: the residual
//! prototype RegExp test met one with `0x61`, size 0 and a `link` of
//! `0x4_0000_0000` at `p - 8`, and a recognizer that trusted the type byte
//! followed that link to `0x3_ffff_fff8`. Every recognizer that reads a byte
//! cell's link, bag or extent must refuse the fake: it goes through the one
//! admission, whose allocator proof the fake cannot pass.
//!
//! Sabotage: `PERRY_B4_SABOTAGE=byte_cell_proof` admits a byte-family type
//! byte without the proof, and the first assertion turns red.
//! `PERRY_B4_SABOTAGE=byte_cell_region` drops the process-wide arm, and the
//! cross-thread test turns red: another thread's tracked metadata cannot see
//! this thread's cells.

use crate::codegen_abi::BYTES_TYPE_VIEW;
use crate::gc::{
    GC_TYPE_BUFFER, GC_TYPE_BUFFER_ARRAY_BUFFER, GC_TYPE_BUFFER_DATA_VIEW,
    GC_TYPE_BUFFER_UINT8ARRAY,
};

/// A link word that names unmapped memory, as in the original fault.
const WILD_LINK: u64 = 0x4_0000_0000;

/// A non-GC block whose first word reads as a `GcHeader` naming `obj_type`
/// with size 0, and whose payload words all hold `WILD_LINK`. Returns the
/// block and the address one header past its start.
fn fake_byte_cell(obj_type: u8) -> (Box<[u64; 8]>, usize) {
    let mut block = Box::new([WILD_LINK; 8]);
    // GcHeader { obj_type, gc_flags: 0, _reserved: 0, size: 0 }.
    block[0] = u64::from(obj_type);
    let addr = &block[1] as *const u64 as usize;
    (block, addr)
}

fn typed_array_view_type() -> u8 {
    // The type byte of the original fault: an Int8Array-family view.
    0x61
}

fn fake_types() -> Vec<u8> {
    vec![
        typed_array_view_type(),
        typed_array_view_type() & !BYTES_TYPE_VIEW,
        GC_TYPE_BUFFER,
        GC_TYPE_BUFFER | BYTES_TYPE_VIEW,
        GC_TYPE_BUFFER_UINT8ARRAY | BYTES_TYPE_VIEW,
        GC_TYPE_BUFFER_ARRAY_BUFFER,
        GC_TYPE_BUFFER_DATA_VIEW | BYTES_TYPE_VIEW,
    ]
}

#[test]
fn a_non_gc_word_with_a_byte_family_type_byte_is_not_a_byte_cell() {
    for obj_type in fake_types() {
        assert!(crate::gc::is_byte_family_type(obj_type), "{obj_type:#x}");
        let (block, addr) = fake_byte_cell(obj_type);
        // The fixture must reach the type byte: a plain header read sees the
        // byte-family type, so only the allocator proof can refuse it.
        let plain =
            unsafe { crate::value::addr_class::try_read_gc_header(addr) }.map(|h| h.obj_type);
        assert_eq!(
            plain,
            Some(obj_type),
            "fixture header unreadable for {obj_type:#x}"
        );

        assert_eq!(
            super::header::byte_cell_type(addr),
            None,
            "{obj_type:#x} admitted"
        );
        assert!(!super::header::is_owned_byte_cell(addr));
        assert_eq!(super::header::buffer_family_type(addr), None);
        assert!(!super::is_registered_buffer(addr));
        assert_eq!(crate::typedarray::lookup_typed_array_kind(addr), None);
        assert!(!crate::typedarray::is_offheap_sidetable_alloc(addr));
        assert!(!super::is_detached_buffer(addr));
        assert_eq!(super::admitted_u8_read(addr, 0), None);
        assert!(!super::admitted_u8_write(addr, 0, 7));
        assert!(!crate::native_arena::is_native_typed_view(
            addr as *const crate::typedarray::TypedArrayHeader
        ));
        assert_eq!(
            crate::object::prototype_chain::object_static_prototype(addr),
            None
        );
        let boxed = crate::value::js_nanbox_pointer(addr as i64);
        assert!(matches!(
            super::bytes::span(boxed, false),
            Err(super::bytes::NotBytes::Foreign)
        ));
        assert!(!matches!(
            crate::typedarray::classify_element_read_receiver(addr as u64),
            crate::typedarray::ElementReadReceiver::TypedArray(_)
        ));
        // Nothing wrote through the fake.
        assert!(block[1..].iter().all(|&w| w == WILD_LINK));
        drop(block);
    }
}

#[test]
fn a_real_byte_cell_passes_the_same_admission() {
    let buf = super::buffer_alloc(4);
    let addr = buf as usize;
    assert_eq!(super::header::byte_cell_type(addr), Some(GC_TYPE_BUFFER));
    assert!(super::is_registered_buffer(addr));
    assert!(super::header::is_owned_byte_cell(addr));
}

/// A thread with an arena of its own admits a cell another thread allocated,
/// and still refuses the fake: the proof is the process-wide region registry,
/// not the probing thread's allocator.
#[test]
fn another_thread_admits_this_threads_cell_and_refuses_the_fake() {
    let scope = crate::gc::RuntimeHandleScope::new();
    let buf = super::buffer_alloc(16);
    let _buf = scope.root_raw_mut_ptr(buf);
    let addr = buf as usize;
    let (block, fake) = fake_byte_cell(GC_TYPE_BUFFER | BYTES_TYPE_VIEW);
    let fake_addr = fake;
    let seen = std::thread::spawn(move || {
        // Give this thread its own arena first.
        let own = super::buffer_alloc(16) as usize;
        assert_eq!(super::header::byte_cell_type(own), Some(GC_TYPE_BUFFER));
        (
            super::header::byte_cell_type(addr),
            super::header::byte_cell_type(fake_addr),
        )
    })
    .join()
    .expect("probe thread");
    assert_eq!(seen, (Some(GC_TYPE_BUFFER), None));
    drop(block);
}

/// Words are classified by tag before any header is read: a number (a numeric
/// fd, a denormal whose bits look like a raw pointer) is never a byte cell,
/// and a pointer-tagged or raw buffer address is.
#[test]
fn words_are_classified_by_tag_before_the_admission() {
    let scope = crate::gc::RuntimeHandleScope::new();
    let buf = super::buffer_alloc(16);
    let _buf = scope.root_raw_mut_ptr(buf);
    let addr = buf as usize;
    let tagged = crate::value::js_nanbox_pointer(addr as i64).to_bits();
    assert_eq!(
        super::header::byte_cell_of_word(tagged),
        Some((addr, GC_TYPE_BUFFER))
    );
    assert_eq!(
        super::header::byte_cell_of_word(addr as u64),
        Some((addr, GC_TYPE_BUFFER))
    );
    assert_eq!(super::header::byte_cell_of_word(3.0f64.to_bits()), None);
    let denormal = 1e-310f64.to_bits();
    assert_eq!(
        denormal >> 48,
        0,
        "fixture: a denormal has the raw-pointer shape"
    );
    assert_eq!(super::header::byte_word_address(denormal), None);
    let (block, fake) = fake_byte_cell(GC_TYPE_BUFFER);
    assert_eq!(super::header::byte_word_address(fake as u64), None);
    drop(block);
}
