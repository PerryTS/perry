//! Geisterhand: In-process input fuzzer for Perry UI applications.
//!
//! Embeds a lightweight HTTP server that exposes registered widget callbacks
//! and allows programmatic input firing (click, type, slide, toggle) and
//! chaos-mode random fuzzing.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

mod chaos;
mod server;

static RUNNING: AtomicBool = AtomicBool::new(false);

/// HWND map: widget handle → Win32 HWND (as usize).
/// Stored here (not in perry-runtime) because perry-runtime may be linked twice
/// (from perry-stdlib and trimmed UI lib), creating separate static instances.
/// This crate is only linked once.
static HWND_MAP: Mutex<Option<HashMap<i64, usize>>> = Mutex::new(None);

/// Store a Win32 HWND for a widget handle. Called from perry-ui-windows during widget creation.
#[no_mangle]
pub extern "C" fn perry_geisterhand_store_hwnd(handle: i64, hwnd: usize) {
    if let Ok(mut map) = HWND_MAP.lock() {
        let m = map.get_or_insert_with(HashMap::new);
        m.insert(handle, hwnd);
    }
}

/// Retrieve the Win32 HWND for a widget handle. Called from the server's /type endpoint.
#[no_mangle]
pub extern "C" fn perry_geisterhand_lookup_hwnd(handle: i64) -> usize {
    match HWND_MAP.lock() {
        Ok(map) => map
            .as_ref()
            .and_then(|m| m.get(&handle).copied())
            .unwrap_or(0),
        Err(_) => 0,
    }
}

/// Start the geisterhand HTTP server on a background thread.
/// Called from compiled binary's main() when --enable-geisterhand was used.
#[no_mangle]
pub extern "C" fn perry_geisterhand_start(port: i32) {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return; // Already running
    }
    let port = if port <= 0 { 7676 } else { port as u16 };
    std::thread::spawn(move || {
        server::run_server(port);
    });
    eprintln!("[geisterhand] listening on http://127.0.0.1:{}", port);
}
