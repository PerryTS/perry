use super::*;
use crate::codegen_abi::{NativeFunctionInfo, FN_HAS_METADATA, FN_HAS_NAME, FN_HAS_SOURCE};
#[repr(C)]
struct EntryImage {
    info: JsFunctionInfo,
    name_delta: i32,
    name_len: u32,
    identity: FunctionMetadataIdentity,
    name: [u8; 24],
    source: [u8; 48],
}
#[repr(C)]
struct Image {
    owner: FunctionMetadataOwner,
    entries: [i32; 2],
    records: [EntryImage; 2],
}
fn image(code: usize) -> Box<Image> {
    let entry = |code| EntryImage {
        info: unsafe { JsFunctionInfo::from_code(code as *const u8, 0) }
            .with_flags(FN_HAS_METADATA | FN_HAS_NAME | FN_HAS_SOURCE),
        name_delta: 0,
        name_len: 5,
        identity: FunctionMetadataIdentity {
            owner: std::ptr::null(),
            ordinal: 0,
        },
        name: [0; 24],
        source: [0; 48],
    };
    let mut image = Box::new(Image {
        owner: FunctionMetadataOwner {
            entries: std::ptr::null(),
            count: 2,
            flags: 0,
            state: std::sync::atomic::AtomicPtr::new(std::ptr::null_mut()),
        },
        entries: [0; 2],
        records: [entry(code), entry(code + 256)],
    });
    image.owner.entries = image.entries.as_ptr();
    let owner = &image.owner as *const _;
    for (ordinal, record) in image.records.iter_mut().enumerate() {
        record.name[..5].copy_from_slice(if ordinal == 0 { b"alpha" } else { b"bravo" });
        let source = if ordinal == 0 {
            b"function a(){}"
        } else {
            b"function b(){}"
        };
        record.source[..source.len()].copy_from_slice(source);
        let info = &record.info as *const _ as usize;
        record.name_delta = (record.name.as_ptr() as isize - info as isize) as i32;
        record.info.versioned_captures = source.len() as u32;
        record.info.versioned_boxed_mask =
            (record.source.as_ptr() as isize - info as isize) as i64 as u64;
        record.identity = FunctionMetadataIdentity {
            owner,
            ordinal: ordinal as u32,
        };
        image.entries[ordinal] = (info as isize - image.owner.entries as isize) as i32;
    }
    image
}
fn registered(info: &JsFunctionInfo) -> Option<String> {
    Record::for_info(info)?.with_name(str::to_owned)
}
#[test]
fn owner_and_ordinal_ignore_code_layout_and_duplicate_spelling() {
    for code in [0x4a01_0000, 0x6b12_0400] {
        let mut image = image(code);
        image.records[1].name[..5].copy_from_slice(b"alpha");
        let first = Record::for_info(&image.records[0].info).unwrap();
        let second = Record::for_info(&image.records[1].info).unwrap();
        assert_eq!(first.ordinal, 0);
        assert_eq!(second.ordinal, 1);
        first.write_name(b"override", false);
        assert_eq!(
            registered(&image.records[0].info).as_deref(),
            Some("override")
        );
        assert_eq!(registered(&image.records[1].info).as_deref(), Some("alpha"));
        assert_eq!(first.source().as_deref(), Some("function a(){}"));
        assert_eq!(second.source().as_deref(), Some("function b(){}"));
    }
}
#[test]
fn owned_name_copies_and_invalid_utf8_falls_back_to_borrowed() {
    let image = image(0x4a02_0000);
    let info = &image.records[0].info;
    let record = Record::for_info(info).unwrap();
    let mut bytes = b"mutable".to_vec();
    record.write_name(&bytes, false);
    bytes.fill(b'x');
    assert_eq!(registered(info).as_deref(), Some("mutable"));
    record.write_name(b"\xff", false);
    assert_eq!(registered(info).as_deref(), Some("alpha"));
    record.write_name(b"inferred", true);
    assert_eq!(registered(info).as_deref(), Some("alpha"));
    assert_eq!(
        state(&image.owner).unwrap().overrides.lock().unwrap().len(),
        1
    );
}
#[test]
fn inference_copies_first_valid_name_and_preserves_invalid_override_fallback() {
    static OWNER: FunctionMetadataOwner = FunctionMetadataOwner::native();
    let native = NativeFunctionInfo::new(
        unsafe { JsFunctionInfo::from_code(0x4a03_0000 as *const u8, 0) },
        &OWNER,
    );
    let record = Record::for_info(&native.info).unwrap();
    record.write_name(b"\xff", false);
    record.write_name("世界".as_bytes(), true);
    record.write_name(b"later", true);
    assert_eq!(registered(&native.info).as_deref(), Some("世界"));
    assert_eq!(state(&OWNER).unwrap().overrides.lock().unwrap().len(), 1);
}
#[test]
fn native_aliases_share_only_explicit_canonical_owners() {
    static SHARED: FunctionMetadataOwner = FunctionMetadataOwner::native();
    static OTHER: FunctionMetadataOwner = FunctionMetadataOwner::native();
    let body = unsafe { JsFunctionInfo::from_code(0x4a04_0000 as *const u8, 0) };
    let alias = NativeFunctionInfo::new(body, &SHARED);
    let declaration = NativeFunctionInfo::new(body, &SHARED);
    let same_thunk = NativeFunctionInfo::new(body, &OTHER);
    Record::for_info(&declaration.info)
        .unwrap()
        .write_name(b"shared", false);
    assert_eq!(registered(&alias.info).as_deref(), Some("shared"));
    assert_eq!(registered(&same_thunk.info), None);
    assert!(
        with_bundle_function_names(|names| names.contains_key(&(body.code as usize)))
            .is_some_and(|present| !present)
    );
}
#[test]
fn closure_info_name_and_registered_override_keep_distinct_precedence() {
    let image = image(0x4a05_0000);
    let info = &image.records[0].info;
    let closure = crate::closure::ClosureHeader {
        capture_count: 0,
        shape_id: crate::codegen_abi::LayoutWord(crate::codegen_abi::FUNCTION_LAYOUT_WORD),
        info,
        props: std::ptr::null_mut(),
    };
    Record::for_info(info)
        .unwrap()
        .write_name(b"override", false);
    assert_eq!(registered(info).as_deref(), Some("override"));
    assert_eq!(
        unsafe { crate::builtins::registered_name_for_closure(&closure) }.as_deref(),
        Some("override")
    );
    assert_eq!(
        unsafe { crate::builtins::function_name_for_closure(&closure) }.as_deref(),
        Some("alpha")
    );
    assert!(unsafe { crate::builtins::function_name_matches_closure(&closure, b"alpha") });
}
#[test]
fn source_overrides_keep_copy_fallback_kind_and_info_precedence() {
    let image = image(0x4a06_0000);
    let info = &image.records[0].info;
    let record = Record::for_info(info).unwrap();
    let mut source = b"function override(){}".to_vec();
    record.write_source(&source, true);
    source.fill(b'x');
    assert_eq!(record.source().as_deref(), Some("function override(){}"));
    assert!(record.ordinary(false));
    assert_eq!(
        crate::builtins::function_source_for_info_or_native(info),
        "function a(){}"
    );
    record.write_source(b"\xff", false);
    assert_eq!(record.source().as_deref(), Some("function a(){}"));
    assert!(!record.ordinary(false));
}
#[test]
fn publication_does_not_allocate_per_entry_and_repeated_init_is_idempotent() {
    let image = Box::leak(image(0x4a07_0000));
    unsafe {
        js_register_function_metadata(&image.owner, 1);
    }
    assert!(image.owner.state.load(Ordering::Acquire).is_null());
    let count = owners().lock().unwrap().len();
    unsafe {
        js_register_function_metadata(&image.owner, 1);
    }
    assert_eq!(owners().lock().unwrap().len(), count);
    assert_eq!(
        with_bundle_function_names(|names| names.predecessor(0x4a07_0010).unwrap().1.to_vec()),
        Some(b"alpha".to_vec())
    );
}
#[test]
fn unloadable_image_bytes_survive_unload_and_address_reuse() {
    let image = image(0x4a08_0000);
    unsafe {
        js_register_function_metadata(&image.owner, 0);
    }
    drop(image);
    assert_eq!(
        source_for_code(0x4a08_0000).as_deref(),
        Some("function a(){}")
    );
    assert_eq!(name_for_code(0x4a08_0000).as_deref(), Some("alpha"));
    let mut replacement = self::image(0x4a08_0000);
    replacement.records[0].name[..5].copy_from_slice(b"again");
    unsafe {
        js_register_function_metadata(&replacement.owner, 0);
    }
    drop(replacement);
    assert_eq!(name_for_code(0x4a08_0000).as_deref(), Some("again"));
}

#[test]
fn inference_before_unloadable_publication_does_not_skip_image_copy() {
    let image = image(0x4a09_0000);
    let record = Record::for_info(&image.records[0].info).unwrap();
    record.write_name(b"early override", false);
    unsafe {
        js_register_function_metadata(&image.owner, 0);
    }
    assert_eq!(
        registered(&image.records[0].info).as_deref(),
        Some("early override")
    );
    drop(image);
    assert_eq!(
        source_for_code(0x4a09_0000).as_deref(),
        Some("function a(){}")
    );
    assert_eq!(name_for_code(0x4a09_0000).as_deref(), Some("alpha"));
}
