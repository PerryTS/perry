//! #8546 multi-image hosts: two images in one process with different static
//! row counts S give the same static ordinal to different layouts. A
//! runtime-crate memo static is process-shared, so an answer one image learned
//! for a static word must never answer the other image's receiver.
use crate::codegen_abi::{PerryLayoutImageV1, PerryStaticLayoutRequestV1};
use crate::object::layout_probe::site_memo;

/// One runtime fixed-key site, shared by both images' threads.
fn site() -> crate::runtime_state_key::NamedStateKey {
    crate::runtime_state_key!(b"f1_multi_image_x")
}

/// Enter a fresh image with `static_rows` rows, publish `names` at the static
/// ordinal `word` under `identity`, and birth one object of it whose slots hold
/// `values`.
unsafe fn image_object(
    static_rows: u32,
    identity: u32,
    word: u32,
    names: &[&[u8]],
    values: &[f64],
) -> *mut crate::object::ObjectHeader {
    crate::object::class_image::enter_current_thread_image();
    crate::object::class_image::bind_layout_image(PerryLayoutImageV1 {
        static_rows,
        ..PerryLayoutImageV1::RUNTIME
    });
    let keys = crate::object::static_shapes::canonical_keys_for_names(names);
    let count = names.len() as u32;
    let mut request = PerryStaticLayoutRequestV1 {
        shape_id: identity,
        layout_word: word,
    };
    let published = crate::object::static_shapes::js_object_layout_for_class_keys_static(
        keys.arr() as u64,
        count,
        count,
        0,
        &mut request,
        0,
    );
    assert_eq!(published, word, "static ordinal published at its row");
    // An ordinary object carrying exactly this static row as its layout, as a
    // compiled birth image stamps it (the record holds keys and prototype).
    let obj = crate::object::js_object_alloc(0, 4);
    (*obj).parent_class_id = word;
    let slots = obj
        .cast::<u8>()
        .add(std::mem::size_of::<crate::object::ObjectHeader>())
        .cast::<u64>();
    for (i, v) in values.iter().enumerate() {
        *slots.add(i) = v.to_bits();
    }
    obj
}

unsafe fn key_bits_and_hash(name: &[u8]) -> (u64, u64) {
    let hash = crate::object::key_bytes_hash(name.as_ptr(), name.len());
    let atom = crate::string::js_string_pool_atom(name.as_ptr(), name.len() as u32, hash, 0);
    (
        crate::value::STRING_TAG | atom as u64,
        crate::string::property_atom_hash(atom as *const crate::StringHeader),
    )
}

#[test]
fn two_images_with_different_static_rows_share_no_runtime_memo_answer() {
    // Row 1500 is a static ordinal (>= the native catalog, < both S) of both
    // images, naming a different layout in each.
    let word = 0x8000_0000u32 | 1500;
    let base = crate::object::shapes::SHAPE_ID_BASE;
    std::thread::spawn(move || unsafe {
        let _no_move = crate::gc::GcSuppressScope::new();
        let obj = image_object(2048, base + 0x7f31, word, &[b"f1_multi_image_x"], &[1.0]);
        // Image A learns its answer for `word` into the site's view of the
        // process-shared memo, as a collecting entrance does.
        let memo = site().memo.unwrap()();
        if !memo.is_null() {
            let (key, hash) = key_bits_and_hash(b"f1_multi_image_x");
            site_memo::prime(
                crate::object::shapes::ordinary_dir_addr(),
                obj,
                key,
                hash,
                &*memo,
            );
        }
        assert_eq!(site().read_object(obj), 1.0);
    })
    .join()
    .unwrap();
    std::thread::spawn(move || unsafe {
        let _no_move = crate::gc::GcSuppressScope::new();
        let obj = image_object(
            3072,
            base + 0x7f32,
            word,
            &[b"f1_multi_image_y", b"f1_multi_image_x"],
            &[2.0, 3.0],
        );
        // The same word names image B's own layout here: no answer image A
        // learned may be returned for it.
        let memo = site().memo.unwrap()();
        if !memo.is_null() {
            match site_memo::lookup(crate::object::shapes::ordinary_dir_addr(), obj, &*memo) {
                None => {}
                Some(site_memo::Answer::Data(bits)) => {
                    assert_eq!(f64::from_bits(bits), 3.0, "cross-image memo answer")
                }
                Some(site_memo::Answer::Getter(_)) => panic!("cross-image memo answer"),
            }
        }
        assert_eq!(site().read_object(obj), 3.0);
    })
    .join()
    .unwrap();
}

#[test]
fn the_primary_image_keeps_its_runtime_memo() {
    // A thread without an image of its own resolves to the process's first
    // image, the owner of the runtime-crate memo storage.
    std::thread::spawn(|| {
        let _ = crate::object::class_image::current_image_id();
        assert!(
            !site().memo.unwrap()().is_null(),
            "the primary image keeps its memo"
        );
    })
    .join()
    .unwrap();
    // The primary exists now, so this thread enters a second image.
    std::thread::spawn(|| {
        crate::object::class_image::enter_current_thread_image();
        assert!(
            site().memo.unwrap()().is_null(),
            "a second image owns no shared memo"
        );
    })
    .join()
    .unwrap();
}
