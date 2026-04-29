//! Chaos mode: randomly fire registered callbacks at a configurable interval.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

static CHAOS_RUNNING: AtomicBool = AtomicBool::new(false);
static EVENTS_FIRED: AtomicU64 = AtomicU64::new(0);
static START_TIME: Mutex<Option<Instant>> = Mutex::new(None);

extern "C" {
    fn perry_geisterhand_get_registry_json(out_len: *mut usize) -> *mut u8;
    fn perry_geisterhand_free_string(ptr: *mut u8, len: usize);
    fn perry_geisterhand_get_closure(handle: i64, callback_kind: u8) -> f64;
    fn perry_geisterhand_queue_action(closure_f64: f64);
    fn perry_geisterhand_queue_action1(closure_f64: f64, arg: f64);
}

/// Parsed widget entry from registry JSON
#[derive(serde::Deserialize)]
struct WidgetEntry {
    handle: i64,
    widget_type: u8,
    callback_kind: u8,
}

pub fn start(interval_ms: u64, _seed: Option<u64>) {
    if CHAOS_RUNNING.swap(true, Ordering::SeqCst) {
        return; // Already running
    }
    EVENTS_FIRED.store(0, Ordering::SeqCst);
    if let Ok(mut t) = START_TIME.lock() {
        *t = Some(Instant::now());
    }

    std::thread::spawn(move || {
        let interval = std::time::Duration::from_millis(interval_ms);

        while CHAOS_RUNNING.load(Ordering::SeqCst) {
            // Get current registry
            let mut len: usize = 0;
            let ptr = unsafe { perry_geisterhand_get_registry_json(&mut len) };
            if ptr.is_null() || len == 0 {
                std::thread::sleep(interval);
                continue;
            }
            let json = unsafe {
                let s = String::from_utf8_lossy(std::slice::from_raw_parts(ptr, len)).into_owned();
                perry_geisterhand_free_string(ptr, len);
                s
            };

            let widgets: Vec<WidgetEntry> = match serde_json::from_str(&json) {
                Ok(w) => w,
                Err(_) => {
                    std::thread::sleep(interval);
                    continue;
                }
            };

            if widgets.is_empty() {
                std::thread::sleep(interval);
                continue;
            }

            // Pick a random widget
            let idx = (rand::random::<usize>()) % widgets.len();
            let widget = &widgets[idx];

            let closure =
                unsafe { perry_geisterhand_get_closure(widget.handle, widget.callback_kind) };

            if closure != 0.0 {
                match widget.widget_type {
                    0 => {
                        // Button — fire onClick (no args)
                        unsafe {
                            perry_geisterhand_queue_action(closure);
                        }
                    }
                    1 => {
                        // TextField — generate random string
                        let len = 5 + (rand::random::<usize>() % 16);
                        let text: String = (0..len)
                            .map(|_| {
                                let c = rand::random::<u8>() % 62;
                                if c < 10 {
                                    (b'0' + c) as char
                                } else if c < 36 {
                                    (b'a' + c - 10) as char
                                } else {
                                    (b'A' + c - 36) as char
                                }
                            })
                            .collect();
                        extern "C" {
                            fn js_string_from_bytes(ptr: *const u8, len: usize) -> *mut u8;
                            fn js_nanbox_string(ptr: i64) -> f64;
                        }
                        let bytes = text.as_bytes();
                        let str_ptr = unsafe { js_string_from_bytes(bytes.as_ptr(), bytes.len()) };
                        let nanboxed = unsafe { js_nanbox_string(str_ptr as i64) };
                        unsafe {
                            perry_geisterhand_queue_action1(closure, nanboxed);
                        }
                    }
                    2 => {
                        // Slider — random f64 in 0.0..1.0
                        let value: f64 = rand::random::<f64>();
                        unsafe {
                            perry_geisterhand_queue_action1(closure, value);
                        }
                    }
                    3 => {
                        // Toggle — random bool
                        let tag = if rand::random::<bool>() {
                            0x7FFC_0000_0000_0004u64 // TAG_TRUE
                        } else {
                            0x7FFC_0000_0000_0003u64 // TAG_FALSE
                        };
                        unsafe {
                            perry_geisterhand_queue_action1(closure, f64::from_bits(tag));
                        }
                    }
                    4 => {
                        // Picker — random index 0..9
                        let idx = (rand::random::<usize>() % 10) as f64;
                        unsafe {
                            perry_geisterhand_queue_action1(closure, idx);
                        }
                    }
                    _ => {
                        // Menu, shortcut, table — just fire with no args
                        unsafe {
                            perry_geisterhand_queue_action(closure);
                        }
                    }
                }
                EVENTS_FIRED.fetch_add(1, Ordering::SeqCst);
            }

            std::thread::sleep(interval);
        }
    });
}

pub fn stop() {
    CHAOS_RUNNING.store(false, Ordering::SeqCst);
}

pub fn status() -> String {
    let running = CHAOS_RUNNING.load(Ordering::SeqCst);
    let events = EVENTS_FIRED.load(Ordering::SeqCst);
    let uptime = START_TIME
        .lock()
        .ok()
        .and_then(|t| t.map(|t| t.elapsed().as_secs()))
        .unwrap_or(0);
    format!(
        r#"{{"running":{},"events_fired":{},"uptime_secs":{}}}"#,
        running, events, uptime
    )
}
