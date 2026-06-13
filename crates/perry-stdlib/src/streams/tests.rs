use super::transform::{run_web_compression_codec, split_utf8_prefix};
use super::*;

#[test]
fn stream_ids_live_outside_pointer_tag_small_handle_band() {
    let id = next_id(&NEXT_STREAM_ID);
    assert!(
        (STREAM_HANDLE_ID_START..STREAM_HANDLE_ID_END).contains(&id),
        "stream id {id:#x} must stay in the raw numeric stream band"
    );
    assert!(
        id >= perry_runtime::value::addr_class::HANDLE_BAND_MAX,
        "stream id {id:#x} must not overlap pointer-tagged small handles"
    );
}

#[test]
fn root_scanner_emits_callbacks_chunks_and_promises() {
    {
        let mut readable = READABLE_STREAMS.lock().unwrap();
        readable.clear();
        readable.insert(
            1,
            ReadableStreamData {
                state: ReadableState::Errored,
                chunks: VecDeque::from([0x7FFD_0000_0000_1234]),
                chunk_sizes: VecDeque::from([1.0]),
                queue_total_size: 1.0,
                pending_reads: VecDeque::from([0x2345_6780 as *mut Promise]),
                start_cb: 0x3456_7890,
                pull_cb: 0,
                cancel_cb: 0,
                high_water_mark: 1.0,
                strategy_size_cb: 0,
                is_byte_stream: false,
                pull_returns_byte_chunk: false,
                pulling: false,
                started: false,
                reader_handle: None,
                error_value: 0x7FFF_0000_0000_4567,
                pending_error_after_chunks: None,
                canceled: false,
            },
        );
    }

    let mut emitted = Vec::new();
    scan_stream_roots(&mut |value| emitted.push(value.to_bits()));

    assert!(emitted.contains(&0x7FFD_0000_0000_1234));
    assert!(emitted.contains(&(0x7FFD_0000_0000_0000 | 0x2345_6780)));
    assert!(emitted.contains(&(0x7FFD_0000_0000_0000 | 0x3456_7890)));
    assert!(emitted.contains(&0x7FFF_0000_0000_4567));
    READABLE_STREAMS.lock().unwrap().clear();
}

#[test]
fn web_compression_formats_round_trip() {
    let input = b"hello stream/web compression";
    for format in [
        WebCompressionFormat::Gzip,
        WebCompressionFormat::Deflate,
        WebCompressionFormat::DeflateRaw,
        WebCompressionFormat::Brotli,
    ] {
        let compressed = run_web_compression_codec(format, false, input).unwrap();
        assert!(!compressed.is_empty());
        let decompressed = run_web_compression_codec(format, true, &compressed).unwrap();
        assert_eq!(decompressed, input);
    }
}

#[test]
fn utf8_split_prefix_tracks_incomplete_sequence() {
    assert_eq!(split_utf8_prefix(&[0x68, 0xc3]).unwrap(), (1, true));
    assert_eq!(split_utf8_prefix(&[0xc3, 0xa9]).unwrap(), (2, false));
    assert!(split_utf8_prefix(&[0xff]).is_err());
}
