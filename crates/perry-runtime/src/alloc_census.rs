//! `PERRY_ALLOC_CENSUS` — a sampling profiler for the *Rust* heap.
//!
//! The GC census (`gc::census`) accounts for the arena and the side tables.
//! On the compiled claude-code TUI those two together explain ~115 MB of a
//! 300 MB idle footprint and ~430 MB of a 2 GB peak: everything else is
//! ordinary Rust-heap memory allocated through the `#[global_allocator]`,
//! which nothing in the runtime could attribute. This module wraps that
//! allocator so a run can answer "which call site owns these dirty pages".
//!
//! Off unless `PERRY_ALLOC_CENSUS=<path>` is set, and the enable flag is read
//! once at `gc_init` rather than per allocation.
//!
//! Two kinds of number are recorded:
//!   * exact totals and a power-of-two size-class histogram (allocated bytes,
//!     freed bytes, live bytes) — every allocation counts;
//!   * sampled call sites — one sample per `PERRY_ALLOC_CENSUS_INTERVAL`
//!     bytes allocated (default 1 MiB). A sample records raw return addresses
//!     (`backtrace(3)`, no symbolication, no allocation) and the sampled
//!     pointer, so a later `dealloc` of that pointer can subtract it again.
//!     What remains at dump time is *live* memory attributed to a call site.
//!
//! The dump is one JSON document per signal, plus the main image's load
//! address so the frames can be symbolised offline with `atos -o <bin> -l`.

use std::alloc::{GlobalAlloc, Layout};
use std::cell::Cell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, AtomicU64, AtomicU8, Ordering};
use std::sync::Mutex;

const CLASSES: usize = 48;
const FRAMES: usize = 20;
/// Presence filter: one saturating counter per 16-byte-aligned pointer hash.
/// A `dealloc` only takes the site lock when its slot is non-zero, so the
/// unsampled path costs one relaxed byte load.
const FILTER_BITS: usize = 20;
const FILTER_LEN: usize = 1 << FILTER_BITS;

static ENABLED: AtomicU8 = AtomicU8::new(0);
static SAMPLE_INTERVAL: AtomicU64 = AtomicU64::new(1 << 20);

static TOTAL_ALLOC_BYTES: AtomicU64 = AtomicU64::new(0);
static TOTAL_ALLOC_COUNT: AtomicU64 = AtomicU64::new(0);
static TOTAL_FREE_BYTES: AtomicU64 = AtomicU64::new(0);
static TOTAL_FREE_COUNT: AtomicU64 = AtomicU64::new(0);
static LIVE_BYTES: AtomicI64 = AtomicI64::new(0);
static PEAK_LIVE_BYTES: AtomicI64 = AtomicI64::new(0);

#[allow(clippy::declare_interior_mutable_const)]
const ZERO_U64: AtomicU64 = AtomicU64::new(0);
#[allow(clippy::declare_interior_mutable_const)]
const ZERO_I64: AtomicI64 = AtomicI64::new(0);
#[allow(clippy::declare_interior_mutable_const)]
const ZERO_U8: AtomicU8 = AtomicU8::new(0);

static CLASS_ALLOC_BYTES: [AtomicU64; CLASSES] = [ZERO_U64; CLASSES];
static CLASS_ALLOC_COUNT: [AtomicU64; CLASSES] = [ZERO_U64; CLASSES];
static CLASS_LIVE_BYTES: [AtomicI64; CLASSES] = [ZERO_I64; CLASSES];
static FILTER: [AtomicU8; FILTER_LEN] = [ZERO_U8; FILTER_LEN];

struct SiteStats {
    alloc_bytes: u64,
    alloc_count: u64,
    live_bytes: i64,
    live_count: i64,
}

#[derive(Default)]
struct Sites {
    /// frames -> site id
    ids: HashMap<[usize; FRAMES], u32>,
    /// site id -> frames (for the dump)
    frames: Vec<[usize; FRAMES]>,
    stats: Vec<SiteStats>,
    /// sampled live pointers -> (site id, size)
    live: HashMap<usize, (u32, usize)>,
}

static SITES: Mutex<Option<Sites>> = Mutex::new(None);

crate::perry_thread_local! {
    /// Bytes still to allocate before the next sample. Const-initialised so
    /// the TLS access itself never allocates.
    static CREDIT: Cell<i64> = const { Cell::new(1 << 20) };
    /// Re-entrancy guard: the sampler's own allocations are not sampled.
    static IN_SAMPLER: Cell<bool> = const { Cell::new(false) };
}

unsafe extern "C" {
    fn backtrace(array: *mut *mut core::ffi::c_void, size: core::ffi::c_int) -> core::ffi::c_int;
}

#[inline]
fn class_of(size: usize) -> usize {
    (usize::BITS - size.max(1).leading_zeros()) as usize % CLASSES
}

#[inline]
fn filter_slot(ptr: usize) -> usize {
    // The low four bits are always zero for mimalloc's alignment; mix the rest.
    let h = (ptr >> 4).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    (h >> (64 - FILTER_BITS)) & (FILTER_LEN - 1)
}

#[inline]
fn enabled() -> bool {
    match ENABLED.load(Ordering::Relaxed) {
        2 => true,
        1 => false,
        _ => init_from_env(),
    }
}

/// Read the switch with `getenv(3)` rather than `std::env::var`, so the very
/// first allocation of the process can decide: `std::env::var` allocates, and
/// an allocator that allocates to answer "am I recording?" recurses. Startup
/// is exactly where the interesting retention is, so waiting for `gc_init`
/// would leave the biggest table unattributed.
#[cold]
fn init_from_env() -> bool {
    unsafe extern "C" {
        fn getenv(name: *const core::ffi::c_char) -> *const core::ffi::c_char;
    }
    const NAME: &[u8] = b"PERRY_ALLOC_CENSUS\0";
    const INTERVAL: &[u8] = b"PERRY_ALLOC_CENSUS_INTERVAL\0";
    // SAFETY: both names are NUL-terminated literals; `getenv` returns a
    // borrowed pointer into the environment block and allocates nothing.
    let on = unsafe { !getenv(NAME.as_ptr() as *const core::ffi::c_char).is_null() };
    if on {
        // SAFETY: as above; the value is a NUL-terminated C string.
        let raw = unsafe { getenv(INTERVAL.as_ptr() as *const core::ffi::c_char) };
        if !raw.is_null() {
            let mut n: u64 = 0;
            let mut i = 0isize;
            loop {
                // SAFETY: walking a NUL-terminated C string.
                let c = unsafe { *raw.offset(i) } as u8;
                if !c.is_ascii_digit() {
                    break;
                }
                n = n.saturating_mul(10).saturating_add((c - b'0') as u64);
                i += 1;
            }
            if n >= 4096 {
                SAMPLE_INTERVAL.store(n, Ordering::Relaxed);
                let _ = CREDIT.try_with(|c| c.set(n as i64));
            }
        }
    }
    ENABLED.store(if on { 2 } else { 1 }, Ordering::Relaxed);
    on
}

/// Called from `gc_init` so a run that never allocates before then still
/// reports a decided state; the allocator decides for itself otherwise.
pub(crate) fn alloc_census_init() {
    let _ = enabled();
}

pub fn alloc_census_path() -> Option<String> {
    std::env::var("PERRY_ALLOC_CENSUS").ok()
}

#[inline]
fn record_alloc(ptr: *mut u8, size: usize) {
    let c = class_of(size);
    CLASS_ALLOC_BYTES[c].fetch_add(size as u64, Ordering::Relaxed);
    CLASS_ALLOC_COUNT[c].fetch_add(1, Ordering::Relaxed);
    CLASS_LIVE_BYTES[c].fetch_add(size as i64, Ordering::Relaxed);
    TOTAL_ALLOC_BYTES.fetch_add(size as u64, Ordering::Relaxed);
    TOTAL_ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
    let live = LIVE_BYTES.fetch_add(size as i64, Ordering::Relaxed) + size as i64;
    PEAK_LIVE_BYTES.fetch_max(live, Ordering::Relaxed);
    let due = CREDIT
        .try_with(|c| {
            let v = c.get() - size as i64;
            if v <= 0 {
                c.set(SAMPLE_INTERVAL.load(Ordering::Relaxed) as i64);
                true
            } else {
                c.set(v);
                false
            }
        })
        .unwrap_or(false);
    if due {
        sample(ptr, size);
    }
}

#[cold]
fn sample(ptr: *mut u8, size: usize) {
    if IN_SAMPLER.try_with(Cell::get).unwrap_or(true) {
        return;
    }
    let _ = IN_SAMPLER.try_with(|g| g.set(true));
    let mut raw = [core::ptr::null_mut::<core::ffi::c_void>(); FRAMES + 4];
    // SAFETY: `backtrace(3)` fills a caller-owned array of at most `len`
    // frame pointers and returns how many it wrote.
    let n = unsafe { backtrace(raw.as_mut_ptr(), (FRAMES + 4) as core::ffi::c_int) };
    let mut frames = [0usize; FRAMES];
    // Drop the profiler's own frames (this fn + record_alloc + alloc).
    let skip = 3usize;
    let n = n.max(0) as usize;
    for i in 0..FRAMES {
        frames[i] = if i + skip < n {
            raw[i + skip] as usize
        } else {
            0
        };
    }
    if let Ok(mut guard) = SITES.lock() {
        {
            let s = guard.get_or_insert_with(Sites::default);
            let next = s.frames.len() as u32;
            let id = *s.ids.entry(frames).or_insert(next);
            if id == next {
                s.frames.push(frames);
                s.stats.push(SiteStats {
                    alloc_bytes: 0,
                    alloc_count: 0,
                    live_bytes: 0,
                    live_count: 0,
                });
            }
            let st = &mut s.stats[id as usize];
            st.alloc_bytes += size as u64;
            st.alloc_count += 1;
            st.live_bytes += size as i64;
            st.live_count += 1;
            s.live.insert(ptr as usize, (id, size));
        }
    }
    let slot = filter_slot(ptr as usize);
    let cur = FILTER[slot].load(Ordering::Relaxed);
    if cur < u8::MAX {
        FILTER[slot].store(cur + 1, Ordering::Relaxed);
    }
    let _ = IN_SAMPLER.try_with(|g| g.set(false));
}

#[inline]
fn record_free(ptr: *mut u8, size: usize) {
    let c = class_of(size);
    CLASS_LIVE_BYTES[c].fetch_sub(size as i64, Ordering::Relaxed);
    TOTAL_FREE_BYTES.fetch_add(size as u64, Ordering::Relaxed);
    TOTAL_FREE_COUNT.fetch_add(1, Ordering::Relaxed);
    LIVE_BYTES.fetch_sub(size as i64, Ordering::Relaxed);
    let slot = filter_slot(ptr as usize);
    if FILTER[slot].load(Ordering::Relaxed) == 0 {
        return;
    }
    unsample(ptr, slot);
}

#[cold]
fn unsample(ptr: *mut u8, slot: usize) {
    if IN_SAMPLER.try_with(Cell::get).unwrap_or(true) {
        return;
    }
    let _ = IN_SAMPLER.try_with(|g| g.set(true));
    if let Ok(mut guard) = SITES.lock() {
        if let Some(s) = guard.as_mut() {
            if let Some((id, size)) = s.live.remove(&(ptr as usize)) {
                let st = &mut s.stats[id as usize];
                st.live_bytes -= size as i64;
                st.live_count -= 1;
                let cur = FILTER[slot].load(Ordering::Relaxed);
                if cur > 0 && cur < u8::MAX {
                    FILTER[slot].store(cur - 1, Ordering::Relaxed);
                }
            }
        }
    }
    let _ = IN_SAMPLER.try_with(|g| g.set(false));
}

/// The `#[global_allocator]` wrapper. When the census is off (the only state
/// a shipped program is ever in unless the env var is set) every method is
/// the inner allocator's plus one relaxed byte load.
pub struct CensusAlloc<A: GlobalAlloc>(pub A);

unsafe impl<A: GlobalAlloc> GlobalAlloc for CensusAlloc<A> {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { self.0.alloc(layout) };
        if enabled() && !p.is_null() {
            record_alloc(p, layout.size());
        }
        p
    }
    #[inline]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { self.0.alloc_zeroed(layout) };
        if enabled() && !p.is_null() {
            record_alloc(p, layout.size());
        }
        p
    }
    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if enabled() && !ptr.is_null() {
            record_free(ptr, layout.size());
        }
        unsafe { self.0.dealloc(ptr, layout) }
    }
    #[inline]
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if enabled() && !ptr.is_null() {
            record_free(ptr, layout.size());
        }
        let p = unsafe { self.0.realloc(ptr, layout, new_size) };
        if enabled() && !p.is_null() {
            record_alloc(p, new_size);
        }
        p
    }
}

fn main_image_load_address() -> usize {
    unsafe extern "C" {
        fn _dyld_get_image_header(index: u32) -> *const core::ffi::c_void;
    }
    // SAFETY: image 0 is the main executable; the call takes no pointer.
    (unsafe { _dyld_get_image_header(0) }) as usize
}

/// Append one JSON document to `PERRY_ALLOC_CENSUS`.
pub(crate) fn alloc_census_dump(label: &str) {
    if !enabled() {
        return;
    }
    let Some(path) = alloc_census_path() else {
        return;
    };
    let _ = IN_SAMPLER.try_with(|g| g.set(true));
    let mut out = String::with_capacity(1 << 16);
    out.push_str("{\"perry_alloc_census\":1,\"label\":\"");
    out.push_str(label);
    out.push_str("\",\"load_address\":");
    out.push_str(&main_image_load_address().to_string());
    out.push_str(",\"sample_interval\":");
    out.push_str(&SAMPLE_INTERVAL.load(Ordering::Relaxed).to_string());
    out.push_str(",\"totals\":{\"alloc_bytes\":");
    out.push_str(&TOTAL_ALLOC_BYTES.load(Ordering::Relaxed).to_string());
    out.push_str(",\"alloc_count\":");
    out.push_str(&TOTAL_ALLOC_COUNT.load(Ordering::Relaxed).to_string());
    out.push_str(",\"free_bytes\":");
    out.push_str(&TOTAL_FREE_BYTES.load(Ordering::Relaxed).to_string());
    out.push_str(",\"free_count\":");
    out.push_str(&TOTAL_FREE_COUNT.load(Ordering::Relaxed).to_string());
    out.push_str(",\"live_bytes\":");
    out.push_str(&LIVE_BYTES.load(Ordering::Relaxed).to_string());
    out.push_str(",\"peak_live_bytes\":");
    out.push_str(&PEAK_LIVE_BYTES.load(Ordering::Relaxed).to_string());
    out.push_str("},\"classes\":[");
    for c in 0..CLASSES {
        let ab = CLASS_ALLOC_BYTES[c].load(Ordering::Relaxed);
        let lb = CLASS_LIVE_BYTES[c].load(Ordering::Relaxed);
        if ab == 0 && lb == 0 {
            continue;
        }
        if !out.ends_with('[') {
            out.push(',');
        }
        out.push_str("{\"class\":");
        out.push_str(&c.to_string());
        out.push_str(",\"alloc_bytes\":");
        out.push_str(&ab.to_string());
        out.push_str(",\"alloc_count\":");
        out.push_str(&CLASS_ALLOC_COUNT[c].load(Ordering::Relaxed).to_string());
        out.push_str(",\"live_bytes\":");
        out.push_str(&lb.to_string());
        out.push('}');
    }
    out.push_str("],\"sites\":[");
    if let Ok(guard) = SITES.lock() {
        if let Some(s) = guard.as_ref() {
            let mut order: Vec<usize> = (0..s.stats.len()).collect();
            order.sort_by_key(|&i| -(s.stats[i].live_bytes.max(s.stats[i].alloc_bytes as i64)));
            for i in order.into_iter().take(400) {
                let st = &s.stats[i];
                if !out.ends_with('[') {
                    out.push(',');
                }
                out.push_str("{\"alloc_bytes\":");
                out.push_str(&st.alloc_bytes.to_string());
                out.push_str(",\"alloc_count\":");
                out.push_str(&st.alloc_count.to_string());
                out.push_str(",\"live_bytes\":");
                out.push_str(&st.live_bytes.to_string());
                out.push_str(",\"live_count\":");
                out.push_str(&st.live_count.to_string());
                out.push_str(",\"frames\":[");
                for (k, f) in s.frames[i].iter().enumerate() {
                    if *f == 0 {
                        break;
                    }
                    if k > 0 {
                        out.push(',');
                    }
                    out.push_str(&f.to_string());
                }
                out.push_str("]}");
            }
        }
    }
    out.push_str("]}\n");
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = f.write_all(out.as_bytes());
        let _ = f.flush();
    }
    let _ = IN_SAMPLER.try_with(|g| g.set(false));
}

/// mimalloc's own view of the process, appended to the run's stderr. Answers
/// "is this memory in use, or free and unpurged" — the census above cannot,
/// because it only sees what the program asked for.
pub(crate) fn mimalloc_stats_print() {
    #[cfg(all(target_pointer_width = "64", feature = "alloc-mimalloc"))]
    {
        unsafe extern "C" {
            fn mi_stats_print(out: *mut core::ffi::c_void);
        }
        // SAFETY: mimalloc's own reporting entry; a null sink means stderr.
        unsafe { mi_stats_print(core::ptr::null_mut()) };
    }
}
