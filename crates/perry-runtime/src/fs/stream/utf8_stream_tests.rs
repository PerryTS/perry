use super::*;
use crate::gc::RuntimeHandleScope;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

thread_local! {
    static EVENTS: RefCell<Vec<(&'static str, usize)>> = const { RefCell::new(Vec::new()) };
}

extern "C" fn record_write(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    bytes: f64,
) -> f64 {
    EVENTS.with(|events| events.borrow_mut().push(("write", bytes as usize)));
    undefined_value()
}

macro_rules! event_callback {
    ($name:ident, $event:literal) => {
        extern "C" fn $name(_closure: *const ClosureHeader, _this: crate::closure::JsThis) -> f64 {
            EVENTS.with(|events| events.borrow_mut().push(($event, 0)));
            undefined_value()
        }
    };
}
event_callback!(record_drain, "drain");
event_callback!(record_finish, "finish");
event_callback!(record_close, "close");

struct OwnedFile {
    directory: PathBuf,
    path: PathBuf,
    fd: i32,
}

impl OwnedFile {
    fn remove(self) {
        assert!(
            !fd_is_registered(self.fd),
            "the stream must close its real file"
        );
        std::fs::remove_file(self.path).unwrap();
        std::fs::remove_dir(self.directory).unwrap();
    }
}

fn new_stream<'a>(
    scope: &'a RuntimeHandleScope,
    sync: bool,
    min_length: usize,
    periodic_flush: usize,
) -> (crate::gc::RuntimeHandle<'a>, OwnedFile) {
    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "perry-utf8stream-{}-{timestamp}-{}",
        std::process::id(),
        NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
    ));
    // Atomic creation refuses an existing path: no fixture adopts old data.
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("output.txt");
    let path_value = scope.root_nanbox_f64(string_value_str(path.to_str().unwrap()));
    let flags = scope.root_nanbox_f64(string_value_str("w"));
    let fd = crate::fs::js_fs_open_sync(path_value.get_nanbox_f64(), flags.get_nanbox_f64());
    assert!(fd.is_finite() && fd >= 0.0);
    let options = scope.root_nanbox_f64(object_value(crate::object::js_object_alloc(0, 0)));
    set_object_field_rooted(scope, &options, b"fd", fd);
    set_object_field_rooted(scope, &options, b"sync", bool_value(sync));
    set_object_field_rooted(scope, &options, b"minLength", min_length as f64);
    set_object_field_rooted(scope, &options, b"maxWrite", min_length.max(7) as f64 + 1.0);
    set_object_field_rooted(scope, &options, b"periodicFlush", periodic_flush as f64);
    let stream = scope.root_nanbox_f64(js_fs_utf8_stream_new(options.get_nanbox_f64()));
    (
        stream,
        OwnedFile {
            directory,
            path,
            fd: fd as i32,
        },
    )
}

fn listen(
    scope: &RuntimeHandleScope,
    stream: &crate::gc::RuntimeHandle<'_>,
    event: &str,
    info: *const crate::closure::JsFunctionInfo,
) {
    let event = scope.root_nanbox_f64(string_value_str(event));
    let callback = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        js_closure_alloc(info, 0) as i64,
    ));
    js_fs_utf8_stream_on(
        stream.get_nanbox_f64(),
        event.get_nanbox_f64(),
        callback.get_nanbox_f64(),
    );
}

fn assert_events(expected: &[(&str, usize)]) {
    EVENTS.with(|events| assert_eq!(events.borrow().as_slice(), expected));
}

fn initialize() {
    crate::gc::gc_init();
    crate::object::static_shapes::js_shape_run_static_seed();
    EVENTS.with(|events| events.borrow_mut().clear());
}

#[test]
fn synchronous_writes_coalesce_drain_on_the_next_tick() {
    let _lock = crate::gc::global_side_table_test_lock();
    initialize();
    let scope = RuntimeHandleScope::new();
    let (stream, file) = new_stream(&scope, true, 1, 0);
    listen(&scope, &stream, "write", crate::fn_info!(record_write, 1));
    listen(&scope, &stream, "drain", crate::fn_info!(record_drain, 0));
    for text in ["a", "b"] {
        let text = scope.root_nanbox_f64(string_value_str(text));
        let accepted = js_fs_utf8_stream_write(stream.get_nanbox_f64(), text.get_nanbox_f64());
        assert_eq!(accepted.to_bits(), crate::value::TAG_TRUE);
    }
    assert_eq!(std::fs::read_to_string(&file.path).unwrap(), "ab");
    assert_events(&[("write", 1), ("write", 1)]);
    crate::builtins::js_drain_queued_microtasks();
    assert_events(&[("write", 1), ("write", 1), ("drain", 0)]);
    crate::builtins::js_drain_queued_microtasks();
    assert_events(&[("write", 1), ("write", 1), ("drain", 0)]);
    js_fs_utf8_stream_destroy(stream.get_nanbox_f64());
    file.remove();
}

#[test]
fn teardown_before_the_next_tick_suppresses_drain() {
    let _lock = crate::gc::global_side_table_test_lock();
    initialize();
    for end in [true, false] {
        EVENTS.with(|events| events.borrow_mut().clear());
        let scope = RuntimeHandleScope::new();
        let (stream, file) = new_stream(&scope, true, 1, 0);
        listen(&scope, &stream, "write", crate::fn_info!(record_write, 1));
        listen(&scope, &stream, "drain", crate::fn_info!(record_drain, 0));
        listen(&scope, &stream, "finish", crate::fn_info!(record_finish, 0));
        listen(&scope, &stream, "close", crate::fn_info!(record_close, 0));
        let text = scope.root_nanbox_f64(string_value_str("a"));
        js_fs_utf8_stream_write(stream.get_nanbox_f64(), text.get_nanbox_f64());
        if end {
            js_fs_utf8_stream_end(stream.get_nanbox_f64(), undefined_value());
        } else {
            js_fs_utf8_stream_destroy(stream.get_nanbox_f64());
        }
        let expected: &[(&str, usize)] = if end {
            &[("write", 1), ("finish", 0), ("close", 0)]
        } else {
            &[("write", 1), ("close", 0)]
        };
        assert_events(expected);
        crate::builtins::js_drain_queued_microtasks();
        assert_events(expected);
        assert_eq!(std::fs::read_to_string(&file.path).unwrap(), "a");
        file.remove();
    }
}

#[test]
fn asynchronous_mode_does_not_add_a_synchronous_drain_tick() {
    let _lock = crate::gc::global_side_table_test_lock();
    initialize();
    let scope = RuntimeHandleScope::new();
    let (stream, file) = new_stream(&scope, false, 1, 0);
    listen(&scope, &stream, "write", crate::fn_info!(record_write, 1));
    listen(&scope, &stream, "drain", crate::fn_info!(record_drain, 0));
    let text = scope.root_nanbox_f64(string_value_str("a"));
    js_fs_utf8_stream_write(stream.get_nanbox_f64(), text.get_nanbox_f64());
    UTF8_STREAM_REGISTRY.with(|registry| {
        let registry = registry.borrow();
        let state = registry
            .values()
            .find(|state| state.object_value.to_bits() == stream.get_nanbox_f64().to_bits())
            .unwrap();
        assert!(!state.drain_scheduled);
    });
    crate::builtins::js_drain_queued_microtasks();
    assert_events(&[("write", 1), ("drain", 0)]);
    assert_eq!(std::fs::read_to_string(&file.path).unwrap(), "a");
    js_fs_utf8_stream_destroy(stream.get_nanbox_f64());
    crate::builtins::js_drain_queued_microtasks();
    file.remove();
}

#[test]
fn periodic_interval_flushes_without_a_reference_and_is_cancelled_on_destroy() {
    let _lock = crate::gc::global_side_table_test_lock();
    initialize();
    let scope = RuntimeHandleScope::new();
    let timers_before = crate::timer::active_timeout_resource_count();
    let (stream, file) = new_stream(&scope, true, 1024, 50);
    listen(&scope, &stream, "write", crate::fn_info!(record_write, 1));
    listen(&scope, &stream, "close", crate::fn_info!(record_close, 0));
    assert_eq!(
        crate::timer::active_timeout_resource_count(),
        timers_before + 1
    );
    let timer = UTF8_STREAM_REGISTRY.with(|registry| {
        registry
            .borrow()
            .values()
            .find(|state| state.object_value.to_bits() == stream.get_nanbox_f64().to_bits())
            .unwrap()
            .periodic_flush_timer
            .unwrap()
    });
    assert_eq!(crate::timer::js_timer_has_ref(timer), 0);
    let text = scope.root_nanbox_f64(string_value_str("a"));
    js_fs_utf8_stream_write(stream.get_nanbox_f64(), text.get_nanbox_f64());
    assert_events(&[]);
    assert_eq!(std::fs::read_to_string(&file.path).unwrap(), "");
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut fired = 0;
    while std::fs::read_to_string(&file.path).unwrap().is_empty() {
        assert!(
            Instant::now() < deadline,
            "the live interval never flushed its byte"
        );
        std::thread::sleep(Duration::from_millis(1));
        fired += crate::timer::js_interval_timer_tick();
    }
    assert!(fired > 0, "the timer pump must actually execute a callback");
    assert_events(&[("write", 1)]);
    assert_eq!(std::fs::read_to_string(&file.path).unwrap(), "a");
    js_fs_utf8_stream_destroy(stream.get_nanbox_f64());
    assert_events(&[("write", 1), ("close", 0)]);
    assert_eq!(crate::timer::active_timeout_resource_count(), timers_before);
    crate::timer::js_interval_timer_tick();
    assert_events(&[("write", 1), ("close", 0)]);
    file.remove();
}
