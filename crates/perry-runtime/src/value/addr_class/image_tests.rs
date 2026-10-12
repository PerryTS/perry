use super::*;
use crate::{gc::GcHeader, string::StringHeader};

#[repr(C, align(8))]
struct Image {
    gc: GcHeader,
    string: StringHeader,
    bytes: [u8; 8],
}
#[used]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_strings"
)]
static IMAGE: Image = Image {
    gc: GcHeader {
        obj_type: crate::gc::GC_TYPE_STRING,
        gc_flags: 0x15,
        _reserved: 0x494d,
        size: 36,
    },
    string: StringHeader {
        utf16_len: 7,
        byte_len: 7,
        capacity: 7,
        refcount: 0,
        flags: 4,
    },
    bytes: *b"literal\0",
};

#[test]
fn tracked_classifier_accepts_image_heap_and_malloc_strings() {
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

#[test]
fn tracked_classifier_rejects_null_forged_and_image_edge_addresses() {
    for addr in [
        0,
        1,
        FETCH_HANDLE_BAND_START,
        0x0000_0347_0000_0000,
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
    let fake = Image {
        gc: GcHeader {
            obj_type: crate::gc::GC_TYPE_STRING,
            gc_flags: 0x15,
            _reserved: 0x494d,
            size: 36,
        },
        string: StringHeader {
            utf16_len: 7,
            byte_len: 7,
            capacity: 7,
            refcount: 0,
            flags: 4,
        },
        bytes: *b"literal\0",
    };
    assert!(unsafe { try_read_tracked_gc_header(&fake.string as *const _ as usize) }.is_none());
}

#[test]
fn native_string_validators_use_canonical_classifier() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = std::process::Command::new("python3")
        .arg(root.join("scripts/string_validator_inventory.py"))
        .output()
        .expect("source invariant requires Python 3");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn string_classifier_checks_type_layout_and_empty_singleton() {
    let _scope = crate::gc::GcSuppressScope::new();
    unsafe {
        assert!(try_read_string_header(&IMAGE.string).is_some());
        assert!(try_read_string_header(crate::string::js_get_empty_string()).is_some());
        assert!(try_read_string_header(std::ptr::null()).is_none());
        let object = crate::object::js_object_alloc(0, 0);
        assert!(try_read_string_header(object.cast()).is_none());
        let string = crate::string::js_string_from_bytes(b"valid".as_ptr(), 5);
        let saved = (*string).capacity;
        (*string).capacity = u32::MAX;
        assert!(try_read_string_header(string).is_none());
        (*string).capacity = saved;
        assert!(try_read_string_header(string).is_some());
    }
}

pub(super) fn image_string() -> *const StringHeader {
    &IMAGE.string
}
