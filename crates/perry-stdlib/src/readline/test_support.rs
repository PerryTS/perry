use super::*;
use std::sync::{Mutex, MutexGuard};

static TEST_LOCK: Mutex<()> = Mutex::new(());
thread_local! {
    pub(super) static DATA_COUNT: RefCell<usize> = const { RefCell::new(0) };
    pub(super) static KEYPRESS_NAMES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

pub(super) fn test_inject_line(line: &str) {
    PENDING_LINES.lock().unwrap().push(line.to_string());
}

pub(super) fn test_inject_chunk(chunk: &[u8]) {
    PENDING_DATA.lock().unwrap().push(chunk.to_vec());
}

extern "C" fn count_data_callback(_closure: *const ClosureHeader, _chunk: f64) -> f64 {
    DATA_COUNT.with(|count| *count.borrow_mut() += 1);
    undefined()
}

pub(super) fn data_counter_callback() -> i64 {
    js_closure_alloc(count_data_callback as *const u8, 0) as i64
}

extern "C" fn count_readable_callback(_closure: *const ClosureHeader) -> f64 {
    DATA_COUNT.with(|count| *count.borrow_mut() += 1);
    undefined()
}

pub(super) fn readable_counter_callback() -> i64 {
    js_closure_alloc(count_readable_callback as *const u8, 0) as i64
}

extern "C" fn record_keypress_callback(
    _closure: *const ClosureHeader,
    _seq: f64,
    key_obj: f64,
) -> f64 {
    let name = object_field(key_obj, b"name")
        .map(value_to_string)
        .unwrap_or_default();
    KEYPRESS_NAMES.with(|names| names.borrow_mut().push(name));
    undefined()
}

pub(super) fn keypress_recorder_callback() -> i64 {
    js_closure_alloc(record_keypress_callback as *const u8, 0) as i64
}

pub(super) fn event_name(name: &str) -> *mut StringHeader {
    js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

pub(super) fn reset() -> MutexGuard<'static, ()> {
    let guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    DATA_COUNT.with(|count| *count.borrow_mut() = 0);
    KEYPRESS_NAMES.with(|names| names.borrow_mut().clear());
    QUESTION_CALLBACK.with(|c| *c.borrow_mut() = None);
    LINE_CALLBACK.with(|c| *c.borrow_mut() = None);
    CLOSE_CALLBACK.with(|c| *c.borrow_mut() = None);
    if let Ok(mut v) = DATA_CALLBACKS.lock() {
        v.clear();
    }
    if let Ok(mut v) = KEYPRESS_CALLBACKS.lock() {
        v.clear();
    }
    if let Ok(mut v) = READABLE_CALLBACKS.lock() {
        v.clear();
    }
    PENDING_LINES.lock().unwrap().clear();
    PENDING_DATA.lock().unwrap().clear();
    PENDING_ESCAPE.lock().unwrap().clear();
    EOF_REACHED.store(false, Ordering::Release);
    READABLE_EOF_NOTIFIED.store(false, Ordering::Release);
    STDIN_PAUSED.store(false, Ordering::Release);
    STDIN_REFED.store(true, Ordering::Release);
    STDIN_DESTROYED.store(false, Ordering::Release);
    CLOSE_FIRED.with(|f| *f.borrow_mut() = false);
    RAW_MODE.store(false, Ordering::Release);
    STDIN_DATA_FLOWING.store(false, Ordering::Release);
    READLINE_INTERFACES.with(|interfaces| interfaces.borrow_mut().clear());
    NEXT_READLINE_HANDLE.with(|next| *next.borrow_mut() = 2);
    READER_STARTED.store(false, Ordering::Release);
    guard
}
