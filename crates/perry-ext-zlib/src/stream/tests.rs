use super::*;

fn state(codec: Codec, chunk_size: usize, hwm: usize) -> ZlibStreamState {
    let driver = Driver::new(codec, chunk_size, hwm, 65536);
    ZlibStreamState {
        async_id: 0,
        codec,
        level: Compression::default(),
        codec_state: if driver.is_decoder() {
            None
        } else {
            make_codec_state(codec)
        },
        ended: false,
        wrote_data: false,
        bytes_written: 0,
        pipes: Vec::new(),
        driver,
        destroyed: false,
        readable_ended: false,
        finished: false,
        error: None,
        end_callbacks: Vec::new(),
    }
}
fn input(codec: Codec, bytes: &[u8]) -> Vec<u8> {
    run_codec(codec, bytes).unwrap()
}
#[test]
fn decoder_suspends_at_hwm_and_resumes_without_losing_bytes() {
    let expected = vec![65; 2_000_000];
    let compressed = input(Codec::Gzip, &expected);
    let mut s = state(Codec::Gunzip, 1024, 2048);
    s.driver.work.push_back(Work::Write(compressed, 0, 0));
    s.driver.work.push_back(Work::End);
    for _ in 0..10 {
        s.driver
            .produce(&mut s.codec_state, 0, &mut s.bytes_written)
            .unwrap();
    }
    assert_eq!(s.driver.output_bytes, 2048);
    assert!(!s.driver.done, "finish must wait for consumer progress");
    let mut out = Vec::new();
    while !s.driver.done || !s.driver.output.is_empty() {
        while let Some(chunk) = s.driver.output.pop_front() {
            assert!(chunk.len() <= 1024);
            s.driver.output_bytes -= chunk.len();
            out.extend(chunk);
        }
        s.driver
            .produce(&mut s.codec_state, 0, &mut s.bytes_written)
            .unwrap();
    }
    assert_eq!(out, expected);
}
#[test]
fn gzip_members_and_byte_split_headers_survive_suspension() {
    let mut compressed = input(Codec::Gzip, b"first");
    compressed.extend(input(Codec::Gzip, b"second"));
    let mut s = state(Codec::Gunzip, 64, 1);
    for byte in compressed {
        s.driver.work.push_back(Work::Write(vec![byte], 0, 0));
    }
    s.driver.work.push_back(Work::End);
    let mut out = Vec::new();
    for _ in 0..1000 {
        s.driver
            .produce(&mut s.codec_state, 0, &mut s.bytes_written)
            .unwrap();
        while let Some(chunk) = s.driver.output.pop_front() {
            s.driver.output_bytes -= chunk.len();
            out.extend(chunk);
        }
        if s.driver.done {
            break;
        }
    }
    assert!(s.driver.done);
    assert_eq!(out, b"firstsecond");
}
#[test]
fn all_decoders_roundtrip_with_bounded_output() {
    let expected = vec![17; 100000];
    for (encoder, decoder) in [
        (Codec::Deflate, Codec::Inflate),
        (Codec::DeflateRaw, Codec::InflateRaw),
        (Codec::Gzip, Codec::Unzip),
        (Codec::BrotliCompress, Codec::BrotliDecompress),
        (Codec::ZstdCompress, Codec::ZstdDecompress),
    ] {
        let mut s = state(decoder, 64, 64);
        s.driver
            .work
            .push_back(Work::Write(input(encoder, &expected), 0, 0));
        s.driver.work.push_back(Work::End);
        let mut out = Vec::new();
        for _ in 0..10000 {
            s.driver
                .produce(&mut s.codec_state, 0, &mut s.bytes_written)
                .unwrap();
            while let Some(chunk) = s.driver.output.pop_front() {
                assert!(chunk.len() <= 64);
                s.driver.output_bytes -= chunk.len();
                out.extend(chunk);
            }
            if s.driver.done {
                break;
            }
        }
        assert!(s.driver.done);
        assert_eq!(out, expected);
    }
}
#[test]
fn writes_drain_native_output_before_their_callback_without_end() {
    let expected = vec![42; 300000];
    for (encoder, decoder) in [
        (Codec::Gzip, Codec::Gunzip),
        (Codec::Deflate, Codec::Inflate),
        (Codec::DeflateRaw, Codec::InflateRaw),
        (Codec::BrotliCompress, Codec::BrotliDecompress),
        (Codec::ZstdCompress, Codec::ZstdDecompress),
    ] {
        let compressed = input(encoder, &expected);
        let mut s = state(decoder, 64, 64);
        s.driver.input_bytes = compressed.len();
        s.driver.work.push_back(Work::Write(compressed, 0, 777));
        let mut out = Vec::new();
        let mut completed = false;
        for _ in 0..10000 {
            let events = s
                .driver
                .produce(&mut s.codec_state, 0, &mut s.bytes_written)
                .unwrap();
            completed |= events.iter().any(|e| matches!(e, ZlibEvent::Callback(777)));
            while let Some(chunk) = s.driver.output.pop_front() {
                s.driver.output_bytes -= chunk.len();
                out.extend(chunk);
            }
            if completed {
                break;
            }
        }
        assert!(completed);
        assert_eq!(out, expected);
        assert_eq!(s.driver.input_bytes, 0);
    }
}

#[test]
fn short_output_does_not_retain_full_chunk_allocations() {
    let mut s = state(Codec::Gunzip, 65536, 512);
    for byte in input(Codec::Gzip, &vec![65; 100000]) {
        s.driver.work.push_back(Work::Write(vec![byte], 0, 0));
    }
    s.driver.work.push_back(Work::End);
    for _ in 0..10000 {
        s.driver
            .produce(&mut s.codec_state, 0, &mut s.bytes_written)
            .unwrap();
        if s.driver.output_bytes >= 512 {
            break;
        }
    }
    assert!(s.driver.output_bytes >= 512);
    let retained: usize = s.driver.output.iter().map(Vec::capacity).sum();
    assert!(
        retained <= 512 + 65536,
        "pending output retained {retained} bytes"
    );
}
