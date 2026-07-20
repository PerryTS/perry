use super::transform::{run_web_compression_codec, split_utf8_prefix};
use super::*;

/// #6602: the id-allocator tests below mutate shared pool state (quarantine
/// limit, free list) — serialize them so cargo's parallel test threads don't
/// interleave allocations between a retire and its recycle assertion.
static ALLOCATOR_TEST_SERIAL: Mutex<()> = Mutex::new(());

fn serial_guard() -> std::sync::MutexGuard<'static, ()> {
    ALLOCATOR_TEST_SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn alloc_closed_readable() -> usize {
    let id = alloc_readable(0, 0, 0, 1.0);
    let mut g = READABLE_STREAMS.lock().unwrap();
    g.get_mut(&id).unwrap().state = ReadableState::Closed;
    drop(g);
    id
}

#[test]
fn stream_ids_live_outside_pointer_tag_small_handle_band() {
    // Allocates from the shared pools — serialize so this can't steal the
    // free-list id the recycle test below asserts on.
    let _serial = serial_guard();
    let id = next_stream_id();
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
fn terminal_stream_ids_recycle_after_quarantine_eviction() {
    let _serial = serial_guard();
    idalloc::test_reset_pools();
    idalloc::set_quarantine_limit_for_test(2);
    let ids: Vec<usize> = (0..3)
        .map(|_| {
            let id = alloc_closed_readable();
            idalloc::retire_readable_terminal(id);
            id
        })
        .collect();
    idalloc::set_quarantine_limit_for_test(idalloc::DEFAULT_QUARANTINE);
    // The third retire overflowed the quarantine of 2: the oldest id lost its
    // registry entry (eviction really cleans up)…
    assert!(
        !READABLE_STREAMS.lock().unwrap().contains_key(&ids[0]),
        "evicted id must leave the registry"
    );
    assert!(
        READABLE_STREAMS.lock().unwrap().contains_key(&ids[2]),
        "quarantined id keeps its registry entry"
    );
    // …and the allocator hands it back, oldest first, before any fresh id.
    let reused = next_stream_id();
    assert_eq!(reused, ids[0], "evicted id recycles FIFO");
    assert!((STREAM_HANDLE_ID_START..STREAM_HANDLE_ID_END).contains(&reused));
    // With the free list drained the allocator is back on fresh ids.
    let fresh = alloc_readable(0, 0, 0, 1.0);
    assert_ne!(fresh, reused);
}

#[test]
fn retire_is_idempotent_per_id() {
    let _serial = serial_guard();
    idalloc::test_reset_pools();
    idalloc::set_quarantine_limit_for_test(idalloc::DEFAULT_QUARANTINE);
    let id = alloc_closed_readable();
    idalloc::retire_readable_terminal(id);
    idalloc::retire_readable_terminal(id);
    // The pipe-lock path must also skip it: it carries no ownership mark.
    idalloc::retire_pipe_lock_id(id);
    assert_eq!(
        idalloc::test_pool_occurrences(id),
        1,
        "an id may sit in the pools at most once — twice means double-alloc"
    );
}

#[test]
fn live_stream_ids_never_enter_the_pool() {
    let _serial = serial_guard();
    idalloc::test_reset_pools();
    let id = alloc_readable(0, 0, 0, 1.0);
    idalloc::retire_readable_terminal(id); // Readable → not terminal
    idalloc::retire_pipe_lock_id(id); // no ownership mark → guarded
    assert_eq!(idalloc::test_pool_occurrences(id), 0);
    {
        // Closed but undrained (slow consumer with queued chunks) stays live.
        let mut g = READABLE_STREAMS.lock().unwrap();
        let s = g.get_mut(&id).unwrap();
        s.state = ReadableState::Closed;
        s.push_chunk(TAG_UNDEFINED, 1.0);
    }
    idalloc::retire_readable_terminal(id);
    assert_eq!(idalloc::test_pool_occurrences(id), 0);
}

#[test]
fn pipe_lock_ids_retire_once_via_ownership_mark() {
    let _serial = serial_guard();
    idalloc::test_reset_pools();
    let id = idalloc::next_pipe_lock_id();
    idalloc::retire_pipe_lock_id(id);
    idalloc::retire_pipe_lock_id(id); // stale duplicate release: mark gone
    assert_eq!(idalloc::test_pool_occurrences(id), 1);
    // A live registered id is untouchable through the pipe path even though
    // it, too, has no ownership mark.
    let live = alloc_readable(0, 0, 0, 1.0);
    idalloc::retire_pipe_lock_id(live);
    assert_eq!(idalloc::test_pool_occurrences(live), 0);
}

#[test]
fn attached_reader_dies_with_terminal_stream_but_not_before() {
    let _serial = serial_guard();
    idalloc::test_reset_pools();
    let stream = alloc_readable(0, 0, 0, 1.0);
    let reader = next_stream_id();
    READERS.lock().unwrap().insert(
        reader,
        ReaderData {
            stream_handle: stream,
            locked: true,
            closed_promise: 0x2345_6780 as *mut Promise,
            is_byob: false,
        },
    );
    {
        let mut g = READABLE_STREAMS.lock().unwrap();
        let s = g.get_mut(&stream).unwrap();
        s.reader_handle = Some(reader);
        s.state = ReadableState::Closed;
    }
    // A still-locked reader does not retire on its own…
    idalloc::retire_reader_if_released(reader);
    assert_eq!(idalloc::test_pool_occurrences(reader), 0);
    // …but goes down with its terminal stream.
    idalloc::retire_readable_terminal(stream);
    assert_eq!(idalloc::test_pool_occurrences(stream), 1);
    assert_eq!(idalloc::test_pool_occurrences(reader), 1);
}

#[test]
fn root_scanner_emits_callbacks_chunks_and_promises() {
    // Clears READABLE_STREAMS wholesale — serialize against the allocator
    // tests, whose assertions read that registry.
    let _serial = serial_guard();
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
