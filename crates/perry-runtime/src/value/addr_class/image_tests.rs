use super::image_checks::*;
use super::*;
use crate::string::StringHeader;

#[test]
fn tracked_classifier_accepts_image_heap_and_malloc_strings() {
    admits_image_heap_and_malloc_strings();
}

#[test]
fn tracked_classifier_rejects_null_forged_and_image_edge_addresses() {
    rejects_null_forged_and_image_edge_addresses();
}

#[test]
fn section_bounds_hold_the_in_section_image() {
    section_holds_the_in_section_image();
}

#[test]
fn counterfeit_images_are_rejected_in_data_stack_and_heap() {
    rejects_counterfeits_in_data_stack_and_heap();
}

/// The layout codegen emits (`string_pool/image.rs`): payload padded to 8,
/// then the atom hash. The runtime's own images share it. Every validator
/// must admit it; a size check that forgets the hash trailer rejects every
/// program literal (`path.join("a", "b")` threw ERR_INVALID_ARG_TYPE).
#[test]
fn image_size_covers_the_atom_hash_trailer() {
    assert_eq!(std::mem::size_of::<Image>(), 48);
    assert_eq!(crate::string::image_string_gc_size(7), 48);
    assert_eq!(crate::string::image_string_gc_size(0), 40);
    let _scope = crate::gc::GcSuppressScope::new();
    unsafe {
        let runtime = crate::object::runtime_layouts::test_image_string();
        assert!(try_read_string_header(runtime).is_some(), "runtime image");
        assert!(
            try_read_string_header(&IMAGE.string).is_some(),
            "fixture image"
        );
    }
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
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
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
