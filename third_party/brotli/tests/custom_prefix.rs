extern crate brotli;
extern crate core;

use brotli::enc::encode::{BrotliEncoderOperation as Op, BrotliEncoderStateStruct};
use brotli::enc::{BrotliEncoderParams, StandardAlloc};
use brotli::{Allocator, SliceWrapperMut};
use std::io::Read;

// The same private search seam is exercised with small, exact physical rings.
#[path = "../src/enc/backward_references/custom_prefix.rs"]
mod custom_prefix;
use custom_prefix::CustomPrefix;

fn encode(input: &[u8], dictionary: &[u8], quality: i32) -> Vec<u8> {
    let params = BrotliEncoderParams {
        quality,
        lgwin: 22,
        ..Default::default()
    };
    let mut output = Vec::new();
    brotli::BrotliCompressCustomIoCustomDict(
        &mut brotli::IoReaderWrapper(&mut &input[..]),
        &mut brotli::IoWriterWrapper(&mut output),
        &mut [0; 4096],
        &mut [0; 4096],
        &params,
        StandardAlloc::default(),
        &mut |_, _, _, _| (),
        dictionary,
        std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "Unexpected EOF"),
    )
    .unwrap();
    output
}

fn decode(wire: &[u8], dictionary: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut alloc = StandardAlloc::default();
    let mut memory = alloc.alloc_cell(dictionary.len());
    memory.slice_mut().copy_from_slice(dictionary);
    let mut output = Vec::new();
    brotli::Decompressor::new_with_custom_dict(wire, 4096, memory).read_to_end(&mut output)?;
    Ok(output)
}

fn bytes(length: usize) -> Vec<u8> {
    let mut seed = 123456789u32;
    (0..length)
        .map(|_| {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 24) as u8
        })
        .collect()
}

fn stream(
    input: &[u8],
    dictionary: &[u8],
    quality: i32,
    window: i32,
    chunk: usize,
    flush: bool,
) -> Vec<u8> {
    let mut state = BrotliEncoderStateStruct::new(StandardAlloc::default());
    state.params.quality = quality;
    state.params.lgwin = window;
    state.set_custom_dictionary(dictionary.len(), dictionary);
    let mut output = Vec::new();
    let mut total = Some(0);
    for input in input.chunks(chunk) {
        let mut available = input.len();
        let mut offset = 0;
        loop {
            let mut buffer = [0; 4096];
            let mut remaining = buffer.len();
            let mut written = 0;
            assert!(state.compress_stream(
                if flush {
                    Op::BROTLI_OPERATION_FLUSH
                } else {
                    Op::BROTLI_OPERATION_PROCESS
                },
                &mut available,
                input,
                &mut offset,
                &mut remaining,
                &mut buffer,
                &mut written,
                &mut total,
                &mut |_, _, _, _| (),
            ));
            output.extend_from_slice(&buffer[..written]);
            if available == 0 && !state.has_more_output() {
                break;
            }
        }
    }
    while !state.is_finished() {
        let mut buffer = [0; 4096];
        let mut remaining = buffer.len();
        let mut written = 0;
        assert!(state.compress_stream(
            Op::BROTLI_OPERATION_FINISH,
            &mut 0,
            &[],
            &mut 0,
            &mut remaining,
            &mut buffer,
            &mut written,
            &mut total,
            &mut |_, _, _, _| (),
        ));
        output.extend_from_slice(&buffer[..written]);
    }
    brotli::enc::encode::BrotliEncoderDestroyInstance(&mut state);
    output
}

#[test]
fn custom_prefix_sizes_and_quality_controls_roundtrip() {
    for size in [0, 1, 2, 3, 123, 127, 128, 129] {
        let dictionary = bytes(size);
        let input = if size < 4 {
            b"short input control".to_vec()
        } else {
            dictionary.repeat(3)
        };
        for quality in [9, 10, 11, BrotliEncoderParams::default().quality] {
            let wire = encode(&input, &dictionary, quality);
            assert_eq!(
                decode(&wire, &dictionary).unwrap(),
                input,
                "size={size},q={quality}"
            );
            if size >= 123 {
                assert!(
                    decode(&wire, &[]).map_or(true, |out| out != input),
                    "dictionary lookup must be live, size={},q={}",
                    size,
                    quality
                );
                let wrong = vec![0; dictionary.len()];
                assert!(decode(&wire, &wrong).map_or(true, |out| out != input));
            }
        }
    }
}

#[test]
fn custom_prefix_long_tail_and_repeated_prefix_are_live() {
    for dictionary in [bytes(500), b"abcXYZ".repeat(30)] {
        let input = dictionary[dictionary.len() - 100..].repeat(4);
        for quality in [10, 11] {
            let wire = encode(&input, &dictionary, quality);
            assert_eq!(decode(&wire, &dictionary).unwrap(), input);
            assert!(decode(&wire, &[]).map_or(true, |out| out != input));
        }
    }
}

#[test]
fn custom_prefix_short_input_chunks_and_flush_are_safe() {
    let dictionary = bytes(129);
    for length in [0, 1, 2, 3, 17, 129] {
        let input = dictionary[..length].to_vec();
        for quality in [10, 11] {
            for flush in [false, true] {
                let wire = stream(&input, &dictionary, quality, 10, 3, flush);
                assert_eq!(decode(&wire, &dictionary).unwrap(), input);
            }
        }
    }
}

#[test]
fn custom_prefix_window_truncation_expiry_and_ring_wrap_roundtrip() {
    let dictionary = bytes(3000);
    let mut input = dictionary[dictionary.len() - 200..].repeat(3);
    // lgwin10 retains1008 prefix bytes; its physical ring is131072 bytes.
    // This crosses both history expiry and a real ring wrap, without huge input.
    input.extend((0..140000).map(|i| (i % 251) as u8));
    for quality in [10, 11] {
        let wire = stream(&input, &dictionary, quality, 10, 1024, true);
        assert_eq!(decode(&wire, &dictionary).unwrap(), input);
    }
}

#[test]
fn custom_prefix_search_is_bounded_and_nearest_first() {
    let data = vec![b'a'; 260];
    let mut prefix = CustomPrefix::new(129);
    let mut matches = Vec::new();
    prefix.find_matches(&data, usize::MAX, 129, 130, 1000, |distance, length| {
        matches.push((distance, length))
    });
    assert_eq!(
        matches.len(),
        126,
        "search actually examined the unindexed tail"
    );
    assert_eq!(matches.first(), Some(&(2, 2)));
    assert_eq!(matches.last(), Some(&(127, 127)));
    assert!(matches.windows(2).all(|pair| pair[0].0 < pair[1].0));
    for (distance, length) in matches {
        assert!(length <= distance && length <= 127);
    }
}

#[test]
fn custom_prefix_search_respects_break_input_and_physical_bounds() {
    let data = [b'a'; 16];
    for (position, mask, max_length) in [(8, 15, 100), (15, 15, 100), (8, 15, 2), (31, 63, 100)] {
        let mut prefix = CustomPrefix::new(5);
        prefix.find_matches(
            &data,
            mask,
            position,
            max_length,
            100,
            |distance, length| {
                assert!(length <= max_length);
                assert!(length <= 5 - (position - distance));
                assert!(length <= data.len() - (position & mask));
            },
        );
    }
    for size in [0, 1] {
        let mut prefix = CustomPrefix::new(size);
        prefix.find_matches(&data, 15, 8, 8, 100, |_, _| {
            panic!("absent prefix must not search")
        });
    }
}

#[test]
fn custom_prefix_expiration_is_permanent_before_position_wrap() {
    let data = [b'a'; 64];
    let mut prefix = CustomPrefix::new(5);
    let mut live = 0;
    prefix.find_matches(&data, 63, 19, 2, 16, |_, _| live += 1);
    assert_eq!(live, 1, "last usable two-byte prefix match is live");
    prefix.expire_before(21, 16);
    for position in [21, 5] {
        prefix.find_matches(&data, 63, position, 2, 16, |_, _| {
            panic!("expired prefix revived")
        });
    }
    let mut prefix = CustomPrefix::new(5);
    prefix.expire_before(1u64 << 33, 16);
    prefix.find_matches(&data, 63, 5, 2, 16, |_, _| {
        panic!("unwrapped expiry revived")
    });
}

// Captured from the checksum-qualified, unpatched Brotli9 library. These are
// literal wire oracles: the test never reads Git history or rebuilds its oracle.
#[test]
fn custom_prefix_no_dictionary_preserves_stock_hq_wire() {
    let lorem = b"Lorem ipsum dolor sit amet, consectetur adipiscing elit. Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.";
    let cases = [
        ("lorem", 9, "1bb30000c4665baab7ee4cc96648ec61ee36017dc0310ed8271e08aeed65b761630ccfb295f6ca6d0e7e358eda07141be2d6075646d96aae990d517eacc578602ffb84ac6f5368c238e5094aeab9c15854ea44631a37a2c17a3e195c9e10c8cb7fe404"),
        ("lorem", 10, "1bb300d88c94abed68c93435a69b46d31af6e3c65d6a3802aff65b05879c907f07908023cc21d880638d3c964447bba9c380f69d68ff9aab8605fbc19597abd97360aeadbb941ff853bfe0223abc40cd8d723463d78172c9e5938de91949b04522c2beb44f97ffd80b"),
        ("lorem", 11, "1bb300e88d94abed68c93435a69b7abde11f3d7fa96104a6dda682434ec8bf0024e0300804830d38b52cd06d5a4ececf80de9a48fad7941a16ec0b9f2cee66cf0f73bdaaadbcc147bd405674f8013515c7317b2f506eb97cb231dd8a44b0754784dd694fba7c630b"),
        ("random", 9, "8b2f8036e086578b5f498865206d48544ab145faf78adb8c89719db36466f349003d5695767ff40de902b7c46981f1033fc943b3c65767992570fd97a30d895b6c30383ae9761e2c22f30210acb3eec3327912a2937cba82175253495142d09697a4f103"),
        ("random", 10, "8b2f8036e086578b5f498865206d48544ab145faf78adb8c89719db36466f349003d5695767ff40de902b7c46981f1033fc943b3c65767992570fd97a30d895b6c30383ae9761e2c22f30210acb3eec3327912a2937cba82175253495142d09697a4f103"),
        ("random", 11, "8b2f8036e086578b5f498865206d48544ab145faf78adb8c89719db36466f349003d5695767ff40de902b7c46981f1033fc943b3c65767992570fd97a30d895b6c30383ae9761e2c22f30210acb3eec3327912a2937cba82175253495142d09697a4f103"),
        ("repeat", 9, "1bb3000024c242b1405a"),
        ("repeat", 10, "1bb300f825c242b140a005"),
        ("repeat", 11, "1bb300f825c242b140a005"),
    ];
    for (name, quality, expected) in cases {
        let input = match name {
            "lorem" => lorem.to_vec(),
            "random" => bytes(96),
            "repeat" => vec![b'a'; 180],
            _ => unreachable!(),
        };
        let wire = encode(&input, &[], quality);
        let hex: String = wire.iter().map(|value| format!("{:02x}", value)).collect();
        assert_eq!(hex, expected, "stock wire changed: {},q={}", name, quality);
    }
}
