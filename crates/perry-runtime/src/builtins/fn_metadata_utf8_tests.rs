use super::decode_registered_bytes;

#[test]
fn metadata_decoding_matches_checked_utf8_at_offsets_and_word_boundaries() {
    // Exercise every byte value in every slot of short inputs, across all
    // alignments and both sides of a machine-word boundary. This validates the
    // unsafe ASCII conversion against the checked contract, not a header hint.
    for offset in 0..16 {
        for len in 0..=4 * std::mem::size_of::<usize>() + 1 {
            let mut storage = vec![b'a'; offset + len + 16];
            let range = offset..offset + len;
            assert_eq!(
                decode_registered_bytes(&storage[range.clone()]),
                std::str::from_utf8(&storage[range.clone()]).ok()
            );
            for position in 0..len {
                for byte in 0..=255u8 {
                    storage[offset + position] = byte;
                    let bytes = &storage[range.clone()];
                    assert_eq!(
                        decode_registered_bytes(bytes),
                        std::str::from_utf8(bytes).ok(),
                        "offset={offset} len={len} position={position} byte={byte}"
                    );
                }
                storage[offset + position] = b'a';
            }
        }
    }
    for bytes in [
        "recovered世界".as_bytes(),
        "é".as_bytes(),
        "🦀".as_bytes(),
        &[0xc0, 0xaf],
        &[0xed, 0xa0, 0x80],
        &[0xf0, 0x9f],
        &[0xff],
        b"ASCII\0name",
    ] {
        assert_eq!(
            decode_registered_bytes(bytes),
            std::str::from_utf8(bytes).ok()
        );
    }
}
