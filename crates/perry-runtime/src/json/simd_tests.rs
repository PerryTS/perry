use super::*;

fn check(bytes: &[u8]) {
    // The oracles describe the three contracts independently of the bit tricks.
    let parse = bytes
        .iter()
        .position(|&b| b == b'"' || b == b'\\' || b < 32);
    let escape = bytes
        .iter()
        .position(|&b| b == b'"' || b == b'\\' || b < 32 || b == 237);
    let quotes = bytes.iter().position(|&b| b == b'"' || b == b'\\');
    assert_eq!(find_string_terminator(bytes), parse, "parse {bytes:?}");
    assert_eq!(find_string_escape(bytes), escape, "escape {bytes:?}");
    assert_eq!(find_quote_or_backslash(bytes), quotes, "quotes {bytes:?}");
    assert_eq!(
        find_word::<true, false>(bytes),
        parse,
        "word parse {bytes:?}"
    );
    assert_eq!(
        find_word::<true, true>(bytes),
        escape,
        "word escape {bytes:?}"
    );
    assert_eq!(
        find_word::<false, false>(bytes),
        quotes,
        "word quotes {bytes:?}"
    );
}

#[test]
fn every_byte_at_vector_word_and_tail_boundaries() {
    for len in [0, 1, 2, 7, 8, 9, 15, 16, 17, 23, 24, 31, 32, 33, 63, 64, 65] {
        for alignment in [0, 1, 7, 15] {
            let mut storage = vec![b'a'; alignment + len + 16];
            // A scanner must ignore even special bytes immediately past the slice.
            storage[alignment + len..].fill(b'"');
            check(&storage[alignment..alignment + len]);
            for index in 0..len {
                for byte in 0..=255u8 {
                    storage[alignment + index] = byte;
                    check(&storage[alignment..alignment + len]);
                }
                storage[alignment + index] = b'a';
            }
        }
    }
}

#[test]
fn adjacent_byte_borrows_and_unsigned_controls() {
    for left in 0..=255u8 {
        for right in 0..=255u8 {
            check(&[left, right, 32, 33, 0x80, 0xFF, b'a', b'b']);
            check(&[b'a', b'b', 0x80, 0xFF, 32, 33, left, right]);
        }
    }
}

#[test]
fn random_byte_strings_match_scalar_contracts() {
    let mut state = 0x92D6_482F_8017_AECBu64;
    for n in 0..12000 {
        let mut storage = vec![0; 19 + n % 257];
        for byte in &mut storage {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *byte = state as u8;
        }
        check(&storage[n % 19..]);
    }
}
