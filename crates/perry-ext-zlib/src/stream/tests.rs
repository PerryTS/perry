use super::*;

fn run(codec: Codec, input: &[u8], chunk_size: usize, write_size: usize) -> Vec<u8> {
    assert_eq!(
        perry_runtime::native_payload_stream_abi::js_perry_stream_abi_layout(),
        ns::layout_digest()
    );
    let mut payload = driver::Payload::new(codec, Compression::default(), chunk_size).unwrap();
    let mut result = Vec::new();
    for chunk in input.chunks(write_size) {
        let mut offset = 0;
        for _ in 0..1_000_000 {
            let op = ns::StepIn {
                op: ns::StreamOp::WRITE,
                flush_kind: 0,
                input: unsafe { chunk.as_ptr().add(offset) },
                len: chunk.len() - offset,
            };
            let mut out = ns::StepOut {
                consumed: 0,
                out: std::ptr::null(),
                out_len: 0,
                status: ns::StepStatus::NEED_INPUT,
                code: 0,
                external_bytes: 0,
            };
            payload.step(&op, &mut out);
            assert_ne!(out.status, ns::StepStatus::ERROR, "codec failed");
            assert!(out.out_len <= chunk_size);
            assert_eq!(out.external_bytes, payload.external_bytes());
            if out.out_len > 0 {
                result
                    .extend_from_slice(unsafe { std::slice::from_raw_parts(out.out, out.out_len) });
            }
            offset += out.consumed;
            if out.status == ns::StepStatus::NEED_INPUT {
                break;
            }
        }
        assert_eq!(offset, chunk.len());
    }
    for _ in 0..1_000_000 {
        let op = ns::StepIn {
            op: ns::StreamOp::FINAL,
            flush_kind: 0,
            input: std::ptr::null(),
            len: 0,
        };
        let mut out = ns::StepOut {
            consumed: 0,
            out: std::ptr::null(),
            out_len: 0,
            status: ns::StepStatus::NEED_INPUT,
            code: 0,
            external_bytes: 0,
        };
        payload.step(&op, &mut out);
        assert_ne!(out.status, ns::StepStatus::ERROR, "final failed");
        if out.out_len > 0 {
            result.extend_from_slice(unsafe { std::slice::from_raw_parts(out.out, out.out_len) });
        }
        if out.status == ns::StepStatus::ENDED {
            return result;
        }
    }
    panic!("codec never finished");
}
#[test]
fn all_codecs_roundtrip_with_one_bounded_output_per_step() {
    let bytes: Vec<_> = (0..100000).map(|i| (i % 251) as u8).collect();
    for (enc, dec) in [
        (Codec::Gzip, Codec::Gunzip),
        (Codec::Deflate, Codec::Inflate),
        (Codec::DeflateRaw, Codec::InflateRaw),
        (Codec::Gzip, Codec::Unzip),
        (Codec::BrotliCompress, Codec::BrotliDecompress),
        (Codec::ZstdCompress, Codec::ZstdDecompress),
    ] {
        let compressed = run(enc, &bytes, 64, 1024);
        assert_eq!(run(dec, &compressed, 64, 7), bytes);
    }
}
#[test]
fn gzip_members_and_byte_split_headers_survive_steps() {
    let mut compressed = crate::gzip_bytes(b"first").unwrap();
    compressed.extend(crate::gzip_bytes(b"second").unwrap());
    assert_eq!(run(Codec::Gunzip, &compressed, 64, 1), b"firstsecond");
    assert_eq!(run(Codec::Unzip, &compressed, 64, 1), b"firstsecond");
}
#[test]
fn gzip_stream_and_one_shot_preserve_the_node_os_byte_and_payload() {
    let input = b"hello hello hello hello hello world";
    assert_eq!(
        run(Codec::Gzip, input, 64, 1024),
        crate::gzip_bytes(input).unwrap()
    );
}
#[test]
fn bomb_can_suspend_after_one_output_step() {
    let compressed = crate::gzip_bytes(&vec![65; 100_000_000]).unwrap();
    let mut p = driver::Payload::new(Codec::Gunzip, Compression::default(), 1024).unwrap();
    let op = ns::StepIn {
        op: ns::StreamOp::WRITE,
        flush_kind: 0,
        input: compressed.as_ptr(),
        len: compressed.len(),
    };
    let mut out = ns::StepOut {
        consumed: 0,
        out: std::ptr::null(),
        out_len: 0,
        status: ns::StepStatus::NEED_INPUT,
        code: 0,
        external_bytes: 0,
    };
    p.step(&op, &mut out);
    assert_eq!(out.out_len, 1024);
    assert!(out.consumed < compressed.len());
    assert!(
        out.external_bytes < 100000,
        "no native input or output queue is retained"
    );
}

fn brotli_dictionary_wire(quality: i32) {
    let dictionary = b"Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.";
    let input = b"Lorem ipsum dolor sit amet, consectetur adipiscing elit. Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.";
    let compressed = brotli_codec::compress(input, Some(dictionary), quality).unwrap();
    assert_eq!(
        brotli_codec::decompress(&compressed, Some(dictionary.to_vec()), usize::MAX, false)
            .unwrap(),
        input,
        "quality {quality}: {}-byte dictionary wire must restore the original input",
        compressed.len(),
    );
    assert!(
        brotli_codec::decompress(&compressed, None, usize::MAX, false).is_err(),
        "quality {quality}: this Node-qualified input must reference its dictionary (wire: {} bytes)",
        compressed.len(),
    );
}

#[test]
fn brotli_dictionary_quality_nine_wire_interoperates() {
    brotli_dictionary_wire(9);
}

/// Node's `brotliCompressSync(text, { params: { LARGE_WINDOW: 1, LGWIN: 30, QUALITY: 5 } })`.
const LARGE_WINDOW_BROTLI: &[u8] = &[
    0x11, 0x5e, 0xb4, 0x00, 0x00, 0x10, 0xb7, 0xb1, 0x75, 0x1d, 0xa7, 0x7b, 0x6c, 0x09, 0x91, 0x60,
    0x81, 0x43, 0x4e, 0x0f, 0xea, 0x5f, 0x20, 0x61, 0x82, 0x54, 0x5b, 0x74, 0x80, 0xa4, 0x10, 0x4f,
    0xa5, 0x0e, 0x30, 0xfe, 0x11, 0x74, 0x58, 0xff, 0x4d, 0x8c, 0x6a, 0x5b, 0xd2, 0xc0, 0x00,
];
const LARGE_WINDOW_TEXT: &[u8] = b"large window brotli is not content-encoding br";

/// Writes `input` then finishes; the error is the Node error code.
fn step_all(payload: &mut driver::Payload, input: &[u8]) -> Result<Vec<u8>, String> {
    let mut result = Vec::new();
    let mut offset = 0;
    let mut final_phase = false;
    for _ in 0..1_000_000 {
        let op = ns::StepIn {
            op: if final_phase {
                ns::StreamOp::FINAL
            } else {
                ns::StreamOp::WRITE
            },
            flush_kind: 0,
            input: if final_phase {
                std::ptr::null()
            } else {
                unsafe { input.as_ptr().add(offset) }
            },
            len: if final_phase { 0 } else { input.len() - offset },
        };
        let mut out = ns::StepOut {
            consumed: 0,
            out: std::ptr::null(),
            out_len: 0,
            status: ns::StepStatus::NEED_INPUT,
            code: 0,
            external_bytes: 0,
        };
        payload.step(&op, &mut out);
        if out.status == ns::StepStatus::ERROR {
            return Err(payload.error_details(out.code).1);
        }
        if out.out_len > 0 {
            result.extend_from_slice(unsafe { std::slice::from_raw_parts(out.out, out.out_len) });
        }
        offset += out.consumed;
        if out.status == ns::StepStatus::NEED_INPUT {
            if final_phase {
                return Ok(result);
            }
            final_phase = true;
        } else if final_phase && out.status == ns::StepStatus::ENDED {
            return Ok(result);
        }
    }
    panic!("codec did not settle");
}

/// RFC 7932 caps the window at 16 MiB; a large-window stream decodes only
/// after the BROTLI_DECODER_PARAM_LARGE_WINDOW opt-in, which `reset` keeps.
#[test]
fn brotli_large_window_requires_opt_in() {
    let rejected = brotli_codec::decompress(LARGE_WINDOW_BROTLI, None, usize::MAX, false);
    assert!(
        matches!(&rejected, Err(brotli_codec::DecodeError::Decode(code)) if code == "ERR__ERROR_FORMAT_WINDOW_BITS"),
        "{rejected:?}"
    );
    assert_eq!(
        brotli_codec::decompress(LARGE_WINDOW_BROTLI, None, usize::MAX, true).unwrap(),
        LARGE_WINDOW_TEXT
    );

    let mut strict =
        driver::Payload::new(Codec::BrotliDecompress, Compression::default(), 4096).unwrap();
    assert_eq!(
        step_all(&mut strict, LARGE_WINDOW_BROTLI).unwrap_err(),
        "ERR__ERROR_FORMAT_WINDOW_BITS"
    );
    let mut large =
        driver::Payload::new(Codec::BrotliDecompress, Compression::default(), 4096).unwrap();
    large.allow_brotli_large_window();
    assert_eq!(
        step_all(&mut large, LARGE_WINDOW_BROTLI).unwrap(),
        LARGE_WINDOW_TEXT
    );
    large.reset().unwrap();
    assert_eq!(
        step_all(&mut large, LARGE_WINDOW_BROTLI).unwrap(),
        LARGE_WINDOW_TEXT
    );
}
