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
/// i32 words per record in this target's records section.
const WORDS: usize = FUNCTION_RECORD_BYTES / 4;
#[repr(C)]
struct Image {
    owner: FunctionMetadataOwner,
    /// The image's records section: one record per info, as the link keeps it.
    section: [i32; 3 * WORDS],
    bounds: FunctionRecordBounds,
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
            count: 2,
            flags: 0,
            state: std::sync::atomic::AtomicPtr::new(std::ptr::null_mut()),
            bounds: 0,
        },
        section: [0; 3 * WORDS],
        bounds: FunctionRecordBounds {
            start: std::ptr::null(),
            stop: std::ptr::null(),
            sentinel: std::ptr::null(),
        },
        records: [entry(code), entry(code + 256)],
    });
    let start = image.section.as_ptr().cast::<u8>();
    image.bounds = FunctionRecordBounds {
        start,
        stop: start.wrapping_add(3 * FUNCTION_RECORD_BYTES),
        sentinel: start.wrapping_add(2 * FUNCTION_RECORD_BYTES),
    };
    let field = &image.owner.bounds as *const i32 as isize;
    image.owner.bounds = (&image.bounds as *const FunctionRecordBounds as isize - field) as i32;
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
        let word = &image.section[ordinal * WORDS] as *const i32 as isize;
        image.section[ordinal * WORDS] = (info as isize - word) as i32;
    }
    image
}
/// A record with the info it was made from, so the tests read like the
/// callers in `fn_metadata.rs`.
#[derive(Clone, Copy)]
struct Bound<'a> {
    record: Record<'a>,
    info: &'a JsFunctionInfo,
    ordinal: u32,
}
fn bound(info: &JsFunctionInfo) -> Option<Bound<'_>> {
    let record = Record::for_info(info)?;
    Some(Bound {
        record,
        info,
        ordinal: record.ordinal,
    })
}
impl Bound<'_> {
    fn with_name<R>(self, read: impl FnOnce(&str) -> R) -> Option<R> {
        self.record.with_name(self.info, read)
    }
    fn write_name(self, bytes: &[u8], infer: bool) -> bool {
        self.record.write_name(self.info, bytes, infer)
    }
    fn source(self) -> Option<String> {
        self.record.source(self.info)
    }
    fn ordinary(self, _fallback: bool) -> bool {
        self.record.ordinary(self.info)
    }
    fn write_source(self, bytes: &[u8], ordinary: bool) {
        self.record.write_source(bytes, ordinary)
    }
}
fn registered(info: &JsFunctionInfo) -> Option<String> {
    bound(info)?.with_name(str::to_owned)
}
#[test]
fn owner_and_ordinal_ignore_code_layout_and_duplicate_spelling() {
    for code in [0x4a01_0000, 0x6b12_0400] {
        let mut image = image(code);
        image.records[1].name[..5].copy_from_slice(b"alpha");
        let first = bound(&image.records[0].info).unwrap();
        let second = bound(&image.records[1].info).unwrap();
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
    let record = bound(info).unwrap();
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
    let record = bound(&native.info).unwrap();
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
    bound(&declaration.info)
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
    bound(info).unwrap().write_name(b"override", false);
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
    let record = bound(info).unwrap();
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
    // No payload is allocated: the state word holds only the owner's
    // registration index (CKI S2).
    assert!(state(&image.owner).is_none());
    assert_ne!(registration(&image.owner), 0);
    let count = images().lock().unwrap().len();
    unsafe {
        js_register_function_metadata(&image.owner, 1);
    }
    assert_eq!(images().lock().unwrap().len(), count);
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
    let record = bound(&image.records[0].info).unwrap();
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
#[test]
fn publication_reports_success_and_copies_the_exact_bytes() {
    let image = image(0x4a09_0000);
    let info = &image.records[0].info;
    let mut bytes = b"published-name".to_vec();
    assert!(crate::builtins::register_function_name(info, &bytes));
    bytes.fill(b'x');
    assert_eq!(registered(info).as_deref(), Some("published-name"));
    assert!(crate::builtins::register_function_name(
        info,
        b"replacement"
    ));
    assert_eq!(registered(info).as_deref(), Some("replacement"));
}
#[test]
fn invalid_or_empty_publication_cannot_prove_a_reflected_name() {
    let image = image(0x4a0a_0000);
    let info = &image.records[0].info;
    assert!(!crate::builtins::register_function_name(info, b""));
    // Publication succeeds without proving UTF-8; the read falls back to the
    // borrowed image name, so callers must not treat success as a name proof.
    assert!(crate::builtins::register_function_name(info, &[0xff]));
    assert_eq!(registered(info).as_deref(), Some("alpha"));
}
/// CKI S2: a ConstFn body enters shape identity's fold as its owner's
/// registration index and its ordinal, never as its info's address. Two images
/// give two owners; one entry spelled by two info copies is one word.
#[test]
fn constfn_identity_word_is_owner_index_and_ordinal() {
    let first = Box::leak(image(0x4a0c_0000));
    let second = Box::leak(image(0x4a0d_0000));
    unsafe {
        js_register_function_metadata(&first.owner, 1);
        js_register_function_metadata(&second.owner, 1);
    }
    let word = |info: &JsFunctionInfo| info_identity_word(info as *const _ as usize as u64);
    let a0 = word(&first.records[0].info);
    let a1 = word(&first.records[1].info);
    let b0 = word(&second.records[0].info);
    assert_eq!(a0 as u32, 0);
    assert_eq!(a1 as u32, 1);
    assert_ne!(a0 >> 32, 0, "a registered owner names its index");
    assert_ne!(a0, b0, "another owner, another body");
    assert_eq!(a0 >> 32, a1 >> 32, "one owner");
    // The same entry read through a copy of its record at another address.
    let copy = Box::leak(Box::new(EntryImage {
        info: first.records[0].info,
        name_delta: 0,
        name_len: 0,
        identity: first.records[0].identity,
        name: [0; 24],
        source: [0; 48],
    }));
    assert_eq!(word(&copy.info), a0);
}

/// The ordered index answers exactly what the descriptor scans answered, and a
/// different view (another owner set) never reuses another view's index.
#[test]
fn ordered_name_index_matches_the_scan_and_follows_the_owner_set() {
    let entries: Vec<(usize, &[u8])> = (0..500usize)
        .map(|i| {
            (
                0x1000 + (i * 7919) % 5000 * 16,
                if i % 2 == 0 {
                    &b"even"[..]
                } else {
                    &b"odd"[..]
                },
            )
        })
        .collect();
    let names = test_bundle_names(&entries);
    for ip in (0..0x16000usize).step_by(13) {
        assert_eq!(
            names.predecessor(ip),
            names.scan_predecessor(ip),
            "ip {ip:#x}"
        );
    }
    for (code, _) in &entries {
        assert!(names.contains_key(code));
        assert!(!names.contains_key(&(code + 1)));
    }
    assert_eq!(
        names.first_key_value().map(|e| e.0),
        entries.iter().map(|e| e.0).min()
    );
    assert_eq!(
        names.last_key_value().map(|e| e.0),
        entries.iter().map(|e| e.0).max()
    );
    let other = test_bundle_names(&[(0x10, b"other")]);
    assert_eq!(other.len(), 1);
    assert_eq!(other.predecessor(0x20), Some((0x10, &b"other"[..])));
    assert_eq!(names.len(), entries.len());
}

#[cfg(feature = "alloc-census")]
#[test]
fn name_lookup_allocates_nothing_on_first_and_repeated_queries() {
    let (_, control) = crate::alloc_census::test_count_allocations(|| {
        std::hint::black_box(Vec::<u8>::with_capacity(16));
    });
    assert!(
        control > 0,
        "the allocator observer must see a real allocation"
    );

    let names = test_bundle_names(&[(0x1000, b"alpha"), (0x1100, b"bravo")]);
    let (hit, allocations) =
        crate::alloc_census::test_count_allocations(|| names.predecessor(0x1101));
    assert_eq!(hit, Some((0x1100, &b"bravo"[..])));
    assert_eq!(allocations, 0, "first lookup must not build a sorted copy");
    let (_, allocations) = crate::alloc_census::test_count_allocations(|| {
        for ip in 0x1000..0x1200 {
            std::hint::black_box(names.predecessor(ip));
        }
    });
    assert_eq!(
        allocations, 0,
        "repeated lookups must not allocate owner keys"
    );
    let image = Box::leak(image(0x1000));
    let images = [ImageRecords::Image(&image.bounds)];
    let names = BundleFunctionNames(&images);
    let (hit, allocations) =
        crate::alloc_census::test_count_allocations(|| names.predecessor(0x1101));
    assert_eq!(hit, Some((0x1100, &b"bravo"[..])));
    assert_eq!(allocations, 0, "static record lookup must not allocate");
}

#[cfg(all(unix, not(target_vendor = "apple")))]
#[test]
fn linker_ordered_records_have_a_sentinel_and_reject_broken_order() {
    let mut fixture = image(0x1000);
    let valid = |bounds: &FunctionRecordBounds| {
        let codes: Vec<_> = unsafe { image_infos(bounds) }
            .map(|info| info.code as usize)
            .collect();
        let sentinel = unsafe { (bounds.sentinel as *const i32).read() };
        sentinel == 0 && codes.windows(2).all(|pair| pair[0] <= pair[1])
    };
    assert!(valid(&fixture.bounds));
    assert_eq!(
        ordered_records::predecessor(&fixture.bounds, 0x1101),
        Some((0x1100, &b"bravo"[..]))
    );
    // Exchange code addresses without changing records or names: the order
    // assertion must reject a malformed emitted section, not silently sort it.
    fixture.records[0].info.code = 0x1100 as *const u8;
    fixture.records[1].info.code = 0x1000 as *const u8;
    assert!(!valid(&fixture.bounds));
    fixture.records[0].info.code = 0x1000 as *const u8;
    fixture.records[1].info.code = 0x1000 as *const u8;
    assert!(valid(&fixture.bounds));
    assert_eq!(
        ordered_records::predecessor(&fixture.bounds, 0x1000),
        Some((0x1000, &b"bravo"[..]))
    );
    fixture.section[2 * WORDS] = 1;
    assert_ne!(unsafe { (fixture.bounds.sentinel as *const i32).read() }, 0);
}
