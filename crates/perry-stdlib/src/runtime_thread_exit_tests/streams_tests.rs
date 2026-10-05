//! #11471 thread-exit regression tests (streams group).
//!
//! Each test populates a process-global Web Streams / zlib table from a
//! short-lived thread through the real FFI surface, reports whether the entry
//! was present while that thread lived (so the absence asserted after the
//! join is not vacuous), and checks the exiting thread's `Arena::drop`
//! released it.

extern "C" fn probe_thunk(
    _closure: *const perry_runtime::ClosureHeader,
    _this: perry_runtime::closure::JsThis,
) -> f64 {
    0.0
}

fn undef() -> f64 {
    f64::from_bits(0x7FFC_0000_0000_0001)
}

/// A fresh closure on the calling thread's heap, NaN-boxed.
fn closure_value() -> f64 {
    let closure =
        perry_runtime::closure::js_closure_alloc(perry_runtime::fn_info!(probe_thunk, 0), 0);
    f64::from_bits(perry_runtime::JSValue::pointer(closure as *const u8).bits())
}

fn string_value(s: &str) -> f64 {
    let ptr = perry_runtime::js_string_from_bytes(s.as_ptr(), s.len() as u32);
    f64::from_bits(perry_runtime::JSValue::string_ptr(ptr).bits())
}

#[cfg(feature = "bundled-streams")]
mod web_streams {
    use super::*;
    use crate::streams as s;

    const READABLE: u32 = 1;
    const WRITABLE: u32 = 2;
    const TRANSFORM: u32 = 4;
    const READER: u32 = 8;
    const WRITER: u32 = 16;

    #[test]
    fn thread_exit_releases_readable_streams_and_their_readers() {
        let ((with_cb, linked, reader, plain), alive) = std::thread::spawn(|| unsafe {
            // Seeded directly: holds the thread's cancel closure.
            let with_cb =
                s::js_readable_stream_new(undef(), undef(), closure_value(), f64::NAN) as usize;
            // Holds nothing of the thread's heap itself; dies through the
            // link to its reader, whose `closed` promise is the thread's.
            let linked = s::js_readable_stream_new(undef(), undef(), undef(), f64::NAN) as usize;
            let reader = s::js_readable_stream_get_reader(linked as f64) as usize;
            // Holds no heap address and has no links: must survive.
            let plain = s::js_readable_stream_new(undef(), undef(), undef(), f64::NAN) as usize;
            let alive = s::stream_registry_presence_for_test(with_cb) == READABLE
                && s::stream_registry_presence_for_test(linked) == READABLE
                && s::stream_registry_presence_for_test(reader) == READER
                && s::stream_registry_presence_for_test(plain) == READABLE;
            ((with_cb, linked, reader, plain), alive)
        })
        .join()
        .unwrap();

        assert!(alive, "the records must exist while their thread lives");
        assert_eq!(
            s::stream_registry_presence_for_test(with_cb),
            0,
            "a dead thread's readable (cancel closure) outlived its heap"
        );
        assert_eq!(
            s::stream_registry_presence_for_test(reader),
            0,
            "a dead thread's reader (closed promise) outlived its heap"
        );
        assert_eq!(
            s::stream_registry_presence_for_test(linked),
            0,
            "a reader's stream must go with it, or the reader id dangles"
        );
        assert_eq!(
            s::stream_registry_presence_for_test(plain),
            READABLE,
            "a record holding no freed address must be left alone"
        );
    }

    #[test]
    fn thread_exit_releases_writable_streams_and_their_writers() {
        let ((writable, writer), alive) = std::thread::spawn(|| unsafe {
            let writable =
                s::js_writable_stream_new(undef(), closure_value(), undef(), undef(), f64::NAN)
                    as usize;
            let writer = s::js_writable_stream_get_writer(writable as f64) as usize;
            let alive = s::stream_registry_presence_for_test(writable) == WRITABLE
                && s::stream_registry_presence_for_test(writer) == WRITER;
            ((writable, writer), alive)
        })
        .join()
        .unwrap();

        assert!(alive, "the records must exist while their thread lives");
        assert_eq!(
            s::stream_registry_presence_for_test(writable),
            0,
            "a dead thread's writable (callbacks, promises) outlived its heap"
        );
        assert_eq!(
            s::stream_registry_presence_for_test(writer),
            0,
            "a dead thread's writer outlived its heap"
        );
    }

    #[test]
    fn thread_exit_releases_transform_streams_and_their_side_tables() {
        let ((transform, readable, writable), alive) = std::thread::spawn(|| unsafe {
            let transform =
                s::js_transform_stream_new(undef(), closure_value(), undef(), undef(), undef());
            let readable = s::js_transform_stream_readable(transform) as usize;
            let writable = s::js_transform_stream_writable(transform) as usize;
            // readableStrategy undefined => highWaterMark 0 => initial
            // backpressure: the write job parks in TRANSFORM_BACKPRESSURED_JOBS
            // (keyed by the readable) and counts in TRANSFORM_PENDING_WRITES.
            let writer = s::js_writable_stream_get_writer(writable as f64);
            s::js_writer_write(writer, string_value("chunk-11471"));
            let transform = transform as usize;
            let alive = s::stream_registry_presence_for_test(transform) == TRANSFORM
                && s::stream_registry_presence_for_test(readable) == READABLE
                && s::stream_registry_presence_for_test(writable) == WRITABLE
                && s::transform_side_tables_hold_for_test(writable)
                && s::transform_side_tables_hold_for_test(readable);
            ((transform, readable, writable), alive)
        })
        .join()
        .unwrap();

        assert!(alive, "the records must exist while their thread lives");
        for (id, what) in [
            (transform, "transform"),
            (readable, "readable side"),
            (writable, "writable side"),
        ] {
            assert_eq!(
                s::stream_registry_presence_for_test(id),
                0,
                "a dead thread's {what} outlived its heap"
            );
            assert!(
                !s::transform_side_tables_hold_for_test(id),
                "a dead thread's transform side-table entry for its {what} outlived its heap"
            );
        }
    }

    #[test]
    fn thread_exit_releases_parked_byob_reads() {
        let (stream, alive) = std::thread::spawn(|| unsafe {
            let stream = s::js_readable_stream_new_with_source_type(
                undef(),
                undef(),
                undef(),
                f64::NAN,
                string_value("bytes"),
            );
            let reader = s::js_readable_stream_get_byob_reader(stream);
            let view = perry_runtime::buffer::js_uint8array_alloc(8);
            let view = f64::from_bits(perry_runtime::JSValue::pointer(view as *const u8).bits());
            s::js_reader_read_with_view(reader, view);
            let stream = stream as usize;
            (stream, s::byob_pending_for_test(stream))
        })
        .join()
        .unwrap();

        assert!(alive, "the BYOB read must be parked while its thread lives");
        assert!(
            !s::byob_pending_for_test(stream),
            "a dead thread's parked BYOB read (promise, view) outlived its heap"
        );
        assert_eq!(s::stream_registry_presence_for_test(stream), 0);
    }

    #[test]
    fn thread_exit_releases_stream_expando_values() {
        const KEY: &str = "__perry_11471_expando_probe";
        let (stream, alive) = std::thread::spawn(|| unsafe {
            let stream = s::js_readable_stream_new(undef(), undef(), undef(), f64::NAN) as usize;
            let value = f64::from_bits(
                perry_runtime::JSValue::pointer(perry_runtime::js_array_alloc(0) as *const u8)
                    .bits(),
            );
            s::stream_expando_set_hook(stream, KEY.as_ptr(), KEY.len(), value);
            (stream, s::stream_expando_get(stream, KEY).is_some())
        })
        .join()
        .unwrap();

        assert!(alive, "the expando must exist while its thread lives");
        assert!(
            s::stream_expando_get(stream, KEY).is_none(),
            "a dead thread's expando value outlived its heap"
        );
        assert_eq!(
            s::stream_registry_presence_for_test(stream),
            READABLE,
            "only the offending property goes, not a stream holding nothing freed"
        );
    }
}

#[cfg(feature = "compression-gzip")]
#[test]
fn thread_exit_releases_zlib_streams_listeners_and_events() {
    use crate::zlib as z;
    let (handle, alive) = std::thread::spawn(|| unsafe {
        let handle = z::js_zlib_create_gzip(undef());
        let cb = perry_runtime::js_nanbox_get_pointer(closure_value());
        z::zlib_stream_on(handle, string_value("data"), cb);
        // `.end()` queues the stream's Data/End events on the global queue.
        z::zlib_stream_end(handle, undef());
        let (stream, listeners, events) = z::zlib_tables_for_test(handle);
        (handle, stream && listeners == 1 && events > 0)
    })
    .join()
    .unwrap();

    assert!(
        alive,
        "the zlib records must exist while their thread lives"
    );
    assert_eq!(
        z::zlib_tables_for_test(handle),
        (false, 0, 0),
        "a dead thread's zlib stream, listener or queued events outlived its heap"
    );
}

/// A retired worker agent's zlib tables go with it, including a stream that
/// names nothing in its heap, which no freed-range check would ever drop.
#[cfg(feature = "compression-gzip")]
#[test]
fn agent_retirement_releases_the_agents_zlib_tables() {
    use crate::zlib as z;
    let (agent, while_alive) = std::thread::spawn(|| {
        let agent = perry_runtime::agent::enter_worker_agent();
        unsafe { z::js_zlib_create_gzip(undef()) };
        let while_alive = z::zlib_agent_stream_count_for_test(agent);
        perry_runtime::agent::retire_agent(agent);
        (agent, while_alive)
    })
    .join()
    .unwrap();
    assert_eq!(
        while_alive,
        Some(1),
        "the stream must be in its agent's tables while the agent lives"
    );
    assert_eq!(
        z::zlib_agent_stream_count_for_test(agent),
        None,
        "a retired agent's zlib tables outlived it"
    );
}
