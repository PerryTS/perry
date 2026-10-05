//! zlib state per agent: the main thread's and each `worker_threads`
//! worker's streams, listeners and queued events are kept apart.

use super::{scan_zlib_roots, Statics};
use perry_ffi::gc_register_mutable_root_scanner_named;
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

/// The zlib state of the agent this thread acts for: the main thread's, or a
/// `worker_threads` worker's own. A stream, its listener closures and its
/// queued events live in the heap of the agent that made the stream, so only
/// that agent may pump them; one shared queue let the main thread take a
/// worker's events and call its closures on the wrong heap (#11340 did the
/// same for net). Created on an agent's first use and never freed, like
/// perry-ext-net's per-agent queues.
pub(crate) fn statics() -> &'static Mutex<Statics> {
    thread_local! {
        /// This thread's last (agent, state) pair, so a hot path skips the map.
        static MINE: std::cell::Cell<Option<(u64, &'static Mutex<Statics>)>> =
            const { std::cell::Cell::new(None) };
    }
    let agent = perry_ffi::agent_post::current_agent();
    if let Some((cached, state)) = MINE.with(std::cell::Cell::get) {
        if cached == agent {
            return state;
        }
    }
    static ALL: std::sync::OnceLock<Mutex<HashMap<u64, &'static Mutex<Statics>>>> =
        std::sync::OnceLock::new();
    let state = *ALL
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entry(agent)
        .or_insert_with(|| {
            Box::leak(Box::new(Mutex::new(Statics {
                streams: HashMap::new(),
                listeners: HashMap::new(),
                pending: VecDeque::new(),
                next_id: 0x60000,
            })))
        });
    MINE.with(|mine| mine.set(Some((agent, state))));
    state
}

thread_local! {
    /// Root scanners are registered per thread: each thread's collector scans
    /// its own agent's closures.
    static ZLIB_GC_REGISTERED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Register the GC root scanner on this thread. Listener closures live only in
/// the `listeners` map; without rooting them a GC between `.on()` and the
/// deferred dispatch would free the closure (same hazard perry-ext-net guards).
pub(super) fn ensure_gc_scanner_registered() {
    if !ZLIB_GC_REGISTERED.with(|done| done.replace(true)) {
        gc_register_mutable_root_scanner_named("perry-ext-zlib", scan_zlib_roots);
    }
}
