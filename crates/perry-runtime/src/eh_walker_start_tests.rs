use super::*;
use std::sync::atomic::Ordering;

#[test]
fn function_start_uses_direct_tables_and_real_gimli_for_unusual_encoding() {
    let _loader = crate::test_support::loaded_objects_test_lock();
    let pc = function_start_uses_direct_tables_and_real_gimli_for_unusual_encoding as *const ()
        as u64
        + 1;
    let walker = Walker::build().expect("walker");
    let mut image = walker
        .images
        .into_iter()
        .find(|image| (image.image.text_addr..image.image.text_end).contains(&pc))
        .expect("test image");
    let expected = gimli_function_start(&image, pc).expect("test FDE");
    let before = FUNCTION_START_GIMLI_FALLBACKS.load(Ordering::Relaxed);
    assert_eq!(
        direct_function_start(&image.image, pc),
        Some(Some(expected))
    );
    assert_eq!(image_function_start(&image, pc), Some(expected));
    assert_eq!(
        FUNCTION_START_GIMLI_FALLBACKS.load(Ordering::Relaxed),
        before
    );
    let (start, fde) = hdr_function(&image.image.cfi_tables(), pc)
        .unwrap()
        .unwrap();
    // Valid DWARF, deliberately outside the direct decoder's sdata4 table:
    // absolute 64-bit pointers and an unsigned 64-bit entry count.
    let mut header = vec![1, 0, 4, 4];
    header.extend_from_slice(&image.image.eh_frame_addr.to_le_bytes());
    header.extend_from_slice(&1_u64.to_le_bytes());
    header.extend_from_slice(&start.to_le_bytes());
    header.extend_from_slice(&fde.to_le_bytes());
    let header = Box::leak(header.into_boxed_slice());
    image.image.hdr_addr = header.as_ptr() as u64;
    image.image.hdr_bytes = header;
    image.bases = image.bases.clone().set_eh_frame_hdr(image.image.hdr_addr);
    image.image.hdr = Some(
        gimli::EhFrameHdr::new(header, NativeEndian)
            .parse(&image.bases, 8)
            .expect("unusual header"),
    );
    assert_eq!(
        direct_function_start(&image.image, pc),
        None,
        "must decline the unusual encoding"
    );
    assert_eq!(image_function_start(&image, pc), Some(expected));
    assert_eq!(
        FUNCTION_START_GIMLI_FALLBACKS.load(Ordering::Relaxed),
        before + 1,
        "real gimli fallback must execute"
    );
    eprintln!("direct lookup matched gimli; unusual 64-bit header executed exactly one real gimli fallback");
}
