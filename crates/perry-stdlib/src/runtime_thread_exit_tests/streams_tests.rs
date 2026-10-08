//! #11471 thread-exit regression tests (streams group).
//!
//! Web Streams tests populate a process-global table from a
//! short-lived thread through the real FFI surface, reports whether the entry
//! was present while that thread lived (so the absence asserted after the
//! join is not vacuous), and checks the exiting thread's `Arena::drop`
//! released it.

thread_local! {
    static CALLBACK_COUNT: std::cell::RefCell<Option<std::sync::Arc<std::sync::atomic::AtomicUsize>>> = const { std::cell::RefCell::new(None) };
}
extern "C" fn probe_thunk(
    _closure: *const perry_runtime::ClosureHeader,
    _this: perry_runtime::closure::JsThis,
) -> f64 {
    CALLBACK_COUNT.with(|count| {
        if let Some(count) = &*count.borrow() {
            count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    });
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
mod zlib_payloads {
    use super::*;
    use std::ffi::c_void;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    struct Watched {
        resource: *mut c_void,
        drop: unsafe extern "C" fn(*mut c_void, *mut c_void),
        dropped: Arc<AtomicUsize>,
    }
    unsafe extern "C" fn drop_watched(resource: *mut c_void, _: *mut c_void) {
        let watched = Box::from_raw(resource as *mut Watched);
        (watched.drop)(watched.resource, std::ptr::null_mut());
        watched.dropped.fetch_add(1, Ordering::SeqCst);
    }
    static WATCH: perry_runtime::native_payload::PayloadVTable =
        perry_runtime::native_payload::PayloadVTable {
            drop: drop_watched,
            stream: None,
        };
    extern "C" {
        fn js_nm_install_zlib();
    }
    fn queued_codec(retire: bool) -> Arc<AtomicUsize> {
        let dropped = Arc::new(AtomicUsize::new(0));
        let count = dropped.clone();
        let callbacks = Arc::new(AtomicUsize::new(0));
        let worker_callbacks = callbacks.clone();
        std::thread::spawn(move || unsafe {
            CALLBACK_COUNT.with(|count| *count.borrow_mut() = Some(worker_callbacks));
            let agent = perry_runtime::agent::enter_worker_agent();
            perry_runtime::gc::gc_init();
            js_nm_install_zlib();
            let scope = perry_runtime::gc::RuntimeHandleScope::new();
            let owner = scope.root_nanbox_f64(perry_ext_zlib::js_zlib_create_gzip(undef()));
            let callback = scope.root_nanbox_f64(closure_value());
            let data = scope.root_nanbox_f64(string_value("data"));
            perry_runtime::node_stream::js_node_stream_method_on(
                perry_runtime::js_nanbox_get_pointer(owner.get_nanbox_f64()),
                data.get_nanbox_f64(),
                callback.get_nanbox_f64(),
            );
            let input = scope.root_nanbox_f64(string_value("queued gzip write"));
            perry_runtime::node_stream::js_node_stream_method_end3(
                perry_runtime::js_nanbox_get_pointer(owner.get_nanbox_f64()),
                input.get_nanbox_f64(),
                undef(),
                undef(),
            );
            // A JS probe in the same immediate queue observes an erroneous
            // teardown drain even when the finalized codec's step is inert.
            perry_runtime::timer::js_set_immediate_callback(perry_runtime::js_nanbox_get_pointer(
                callback.get_nanbox_f64(),
            ));
            assert_eq!(perry_runtime::timer::js_immediate_has_pending(), 1);
            // Wrap only the native destructor after queuing. The real codec
            // is freed by its original vtable; no GC address leaves the worker.
            let obj = perry_runtime::JSValue::from_bits(owner.get_nanbox_u64())
                .as_pointer::<perry_runtime::object::ObjectHeader>();
            // `native_state` -> the stream's state record -> its payload cell.
            let cell = perry_runtime::native_payload::payload_cell_of_word((*(*obj).meta).native_state)
                .expect("a zlib stream has a payload cell");
            assert!((*cell).external_bytes > 100000);
            let original =
                &*((*cell).finalizer as *const perry_runtime::native_payload::PayloadVTable);
            let watched = Box::new(Watched {
                resource: (*cell).resource_ptr,
                drop: original.drop,
                dropped: count.clone(),
            });
            (*cell).resource_ptr = Box::into_raw(watched).cast();
            (*cell).finalizer = (&WATCH as *const perry_runtime::native_payload::PayloadVTable)
                .cast_mut()
                .cast();
            assert_eq!(count.load(Ordering::SeqCst), 0);
            if std::env::var("PERRY_TEST_ZLIB_TEARDOWN_SABOTAGE").as_deref()
                == Ok("drain_after_finalize")
            {
                perry_runtime::native_handle::js_native_handle_dispose(f64::from_bits(
                    perry_runtime::JSValue::pointer(cell.cast()).bits(),
                ));
                perry_runtime::timer::js_event_loop_check_phase();
            }
            if retire {
                perry_runtime::agent::retire_agent(agent);
            }
        })
        .join()
        .unwrap();
        assert_eq!(
            callbacks.load(Ordering::SeqCst),
            0,
            "worker teardown never invokes JS"
        );
        // The main agent remains usable after the worker's heap retires.
        perry_runtime::gc::gc_init();
        assert!(!perry_runtime::buffer::js_buffer_alloc(16, 0).is_null());
        dropped
    }
    #[test]
    fn thread_exit_releases_a_real_codec_with_a_step_queued() {
        assert_eq!(queued_codec(false).load(Ordering::SeqCst), 1);
    }
    #[test]
    fn retired_worker_heap_releases_a_real_codec_with_a_step_queued() {
        assert_eq!(queued_codec(true).load(Ordering::SeqCst), 1);
    }
    #[test]
    fn draining_after_codec_finalize_turns_worker_witness_red() {
        let witness = "runtime_thread_exit_tests::streams_tests::zlib_payloads::retired_worker_heap_releases_a_real_codec_with_a_step_queued";
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", witness, "--nocapture"])
            .env("PERRY_TEST_ZLIB_TEARDOWN_SABOTAGE", "drain_after_finalize")
            .output()
            .unwrap();
        let log = format!(
            "{}{}",
            String::from_utf8_lossy(&child.stdout),
            String::from_utf8_lossy(&child.stderr)
        );
        assert!(
            !child.status.success() && log.contains("worker teardown never invokes JS"),
            "{log}"
        );
    }
}
