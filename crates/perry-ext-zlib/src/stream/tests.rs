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
