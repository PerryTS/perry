use super::*;

#[test]
fn traced_flag_refresh_clears_only_obsolete_mark_obligations() {
    let cases = [
        (GC_TYPE_OBJECT, GC_FLAG_ARENA, 0, false, false),
        (
            GC_TYPE_OBJECT,
            GC_FLAG_ARENA | GC_FLAG_PINNED,
            0,
            true,
            true,
        ),
        (
            GC_TYPE_OBJECT,
            GC_FLAG_ARENA | GC_FLAG_FORWARDED,
            0,
            true,
            false,
        ),
        (GC_TYPE_OBJECT, 0, 0, true, false),
        (
            GC_TYPE_ARRAY,
            GC_FLAG_ARENA,
            GC_ARRAY_RAW_F64_LAYOUT,
            true,
            false,
        ),
        (
            GC_TYPE_ARRAY,
            GC_FLAG_ARENA,
            GC_ARRAY_RAW_F64_HOLES,
            true,
            false,
        ),
    ];
    // Exercise starts in the first word, a later word and a later chunk.
    for offset in [
        0,
        64 << CENSUS_START_ALIGN_SHIFT,
        (64 * CENSUS_BITMAP_CHUNK_WORDS) << CENSUS_START_ALIGN_SHIFT,
    ] {
        for (obj_type, flags, reserved, obligation, premarked) in cases {
            let extent = offset + GC_HEADER_SIZE;
            let mut storage = vec![0u64; extent.div_ceil(8)];
            let data = storage.as_mut_ptr() as usize;
            let header = (data + offset) as *mut GcHeader;
            // SAFETY: storage is aligned and contains this complete header;
            // the helper reads only the exact recorded header and its flags.
            unsafe {
                header.write(GcHeader {
                    obj_type,
                    gc_flags: flags | GC_FLAG_MARKED,
                    _reserved: reserved,
                    size: GC_HEADER_SIZE as u32,
                });
                begin_recording();
                let mut builder = AdoptableBlockBuilder::begin(
                    data,
                    extent,
                    extent,
                    crate::arena::PromotionLiveness::Marked,
                );
                builder.note(header, offset);
                let mut block = builder.block.take().expect("recording must be live");
                assert!(
                    block.flag_obligation && block.premarked,
                    "premise: the recorded trace mark is an obligation"
                );
                (*header).gc_flags &= !GC_FLAG_MARKED;
                block.refresh_traced_header_facts(data);
                assert_eq!(
                    (block.flag_obligation, block.premarked),
                    (obligation, premarked),
                    "offset={offset} flags={flags} reserved={reserved}"
                );
                discard();
            }
        }
    }
}
