//! Image classifier checks shared by the unit tests (`image_tests.rs`) and the
//! wasm self-test.
//!
//! `perry-runtime`'s own test target does not build for wasm32, so on wasm the
//! same checks run inside a real linked program: an archive built with the
//! `wasm-image-selftest` feature runs [`run_all`] at startup, where the
//! linker-synthesized `__start_/__stop_perry_strings` and the section holding
//! the program's own literals are the production ones.
use super::*;
use crate::{gc::GcHeader, string::StringHeader};

#[repr(C, align(8))]
#[allow(dead_code)]
pub(super) struct Image {
    pub(super) gc: GcHeader,
    pub(super) string: StringHeader,
    pub(super) bytes: [u8; 8],
    pub(super) hash: u64,
}

/// A string image with a correct marker, layout and NUL: what codegen emits
/// for the literal `"literal"`.
pub(super) const fn literal_image() -> Image {
    Image {
        gc: GcHeader {
            obj_type: crate::gc::GC_TYPE_STRING,
            gc_flags: 0x15,
            _reserved: 0x494d,
            size: crate::string::image_string_gc_size(7) as u32,
        },
        string: StringHeader {
            utf16_len: 7,
            byte_len: 7,
            capacity: 7,
            refcount: 0,
            flags: 4,
        },
        bytes: *b"literal\0",
        hash: 0,
    }
}

#[used]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_strings"
)]
pub(super) static IMAGE: Image = literal_image();

/// The same bytes in ordinary static data (on wasm: a data segment below
/// `__data_end`, where the stack also lives with `--stack-first`).
#[used]
static COUNTERFEIT_DATA: Image = literal_image();

pub(super) fn admits_image_heap_and_malloc_strings() {
    let _scope = crate::gc::GcSuppressScope::new();
    unsafe {
        let image = &IMAGE.string as *const _ as usize;
        assert!(
            try_read_tracked_gc_header_of_type(image, crate::gc::GC_TYPE_STRING).is_some(),
            "image string must be admitted"
        );
        let heap = crate::string::js_string_from_bytes(b"heap string".as_ptr(), 11) as usize;
        assert!(try_read_tracked_gc_header_of_type(heap, crate::gc::GC_TYPE_STRING).is_some());
        let malloc = crate::gc::gc_malloc(32, crate::gc::GC_TYPE_STRING);
        // GC_STORE_AUDIT(POINTER_FREE): Test malloc StringHeader initialized only with scalar lengths/refcount/flags under GcSuppressScope; no managed children.
        (malloc as *mut StringHeader).write(StringHeader {
            utf16_len: 7,
            byte_len: 7,
            capacity: 7,
            refcount: 1,
            flags: 4,
        });
        assert!(
            try_read_tracked_gc_header_of_type(malloc as usize, crate::gc::GC_TYPE_STRING)
                .is_some()
        );
    }
}

pub(super) fn rejects_null_forged_and_image_edge_addresses() {
    for addr in [
        0,
        1,
        FETCH_HANDLE_BAND_START,
        usize::MAX >> 4,
        usize::MAX,
        &IMAGE as *const _ as usize,
        &IMAGE.bytes as *const _ as usize + 7,
    ] {
        assert!(
            unsafe { try_read_tracked_gc_header_of_type(addr, crate::gc::GC_TYPE_STRING) }
                .is_none(),
            "forged {addr:#x}"
        );
    }
    // A readable counterfeit marker outside the dedicated image section is
    // also rejected: recognizing the marker alone is not ownership proof.
    let fake = literal_image();
    assert!(unsafe { try_read_tracked_gc_header(&fake.string as *const _ as usize) }.is_none());
}

/// Program literals are admitted by the exact bounds of the linker-owned
/// section. On wasm this runs against the real `__start_/__stop_perry_strings`
/// the linker synthesizes: a link that does not define them fails to build, and
/// one that defines them wrongly fails here.
pub(super) fn section_holds_the_in_section_image() {
    let (start, end) = super::image::bounds();
    let image = &IMAGE as *const Image as usize;
    assert!(start < end, "perry_strings section is empty");
    assert!(
        start <= image && image + std::mem::size_of::<Image>() <= end,
        "image {image:#x} outside section {start:#x}..{end:#x}"
    );
    assert_eq!(start % 8, 0);
    let ordinary = &COUNTERFEIT_DATA as *const Image as usize;
    assert!(ordinary < start || ordinary >= end);
    let _scope = crate::gc::GcSuppressScope::new();
    assert!(unsafe { try_read_string_header(&IMAGE.string) }.is_some());
}

/// A counterfeit with a correct marker is rejected wherever it lives: data,
/// stack and heap. Nothing below `__data_end` is a string image.
pub(super) fn rejects_counterfeits_in_data_stack_and_heap() {
    #[cfg(target_family = "wasm")]
    {
        unsafe extern "C" {
            static __data_end: u8;
        }
        let data_end = std::ptr::addr_of!(__data_end) as usize;
        assert!(
            (&COUNTERFEIT_DATA as *const Image as usize) < data_end,
            "the data counterfeit must sit below __data_end to prove anything"
        );
    }
    let _scope = crate::gc::GcSuppressScope::new();
    let stack = literal_image();
    let heap = Box::new(literal_image());
    for (place, image) in [
        ("data", &COUNTERFEIT_DATA),
        ("stack", &stack),
        ("heap", &*heap),
    ] {
        let string = &image.string as *const StringHeader;
        assert!(
            unsafe { try_read_tracked_gc_header(string as usize) }.is_none(),
            "{place} counterfeit admitted by the tracked classifier"
        );
        assert!(
            unsafe { try_read_string_header(string) }.is_none(),
            "{place} counterfeit admitted as a string"
        );
    }
}

/// Run every check on this thread; a failed assertion aborts the program.
#[cfg(all(feature = "wasm-image-selftest", target_family = "wasm"))]
pub(super) fn run_all() {
    let checks: [(&str, fn()); 4] = [
        (
            "admits_image_heap_and_malloc_strings",
            admits_image_heap_and_malloc_strings,
        ),
        (
            "rejects_null_forged_and_image_edge_addresses",
            rejects_null_forged_and_image_edge_addresses,
        ),
        (
            "section_holds_the_in_section_image",
            section_holds_the_in_section_image,
        ),
        (
            "rejects_counterfeits_in_data_stack_and_heap",
            rejects_counterfeits_in_data_stack_and_heap,
        ),
    ];
    for (name, check) in checks {
        check();
        eprintln!("perry wasm image selftest: ok {name}");
    }
    let (start, end) = super::image::bounds();
    eprintln!(
        "perry wasm image selftest: {} checks passed, perry_strings {start:#x}..{end:#x} ({} bytes)",
        checks.len(),
        end - start
    );
}
