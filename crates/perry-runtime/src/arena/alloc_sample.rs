//! `PERRY_ALLOC_SITE_SAMPLE=<bytes>`: byte-proportional allocation-site
//! sampling for the GC arena — WHAT allocates, by call chain and object type.
//!
//! The question it answers: a 3,300-character streamed reply in the compiled
//! claude-code TUI pushes ~890 MB through the nursery (86 copying minors)
//! while node allocates a small fraction of that for the same interaction.
//! Nothing in the runtime could say which JS operation, or which runtime
//! helper under it, produced the volume. This does, the way V8's sampling
//! heap profiler does: every `<bytes>` of arena allocation, capture the
//! native return-address chain of the allocation that crossed the boundary.
//! Each sample stands for `<bytes>` of allocation, so a site's share of the
//! samples is its share of the bytes, independent of its object size mix.
//!
//! Coverage:
//!
//! * every runtime allocation path — [`super::arena_alloc_gc`],
//!   `arena_alloc_gc_no_collect`, the old-gen births, the longlived arena —
//!   decrements a per-thread countdown (one relaxed atomic load when the
//!   sampler is off, the only cost the default build pays);
//! * the codegen inline bump allocator never enters the runtime, so while
//!   sampling is on the mirrored `InlineArenaState.size` is capped at
//!   `offset + <bytes left until the next sample>` ([`inline_limit`]), and
//!   every site that writes the inline offset back to its block charges the
//!   inline bytes allocated since the last sync to the SAME countdown
//!   ([`note_inline_sync`]). One countdown for both paths is what makes the
//!   weighting exact: capping at `offset + interval` on every resync (the
//!   first cut) let a loop that interleaves runtime and inline allocations
//!   push the cap ahead forever — 29 samples for 29 MB of inline objects.
//!   Inert when off — the cap is the real block size.
//!
//! Report: `[alloc-site] …` lines after each copying minor and at process
//! exit — totals, bytes by object type, and the top sites as an
//! innermost-first chain resolved to JS display names where a frame is
//! compiled user code (`crate::error::describe_chain`), else the linker
//! symbol. Cumulative since process start.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Sampling interval in bytes; 0 = off. Written once at `gc_init`.
static INTERVAL: AtomicUsize = AtomicUsize::new(0);

/// The interval a bare `=1`/`on` selects; pinned by the knob test.
pub(crate) const DEFAULT_INTERVAL_BYTES: usize = 64 * 1024;
const DEPTH: usize = 6;
const TYPE_SLOTS: usize = 32;

#[derive(Default, Clone)]
struct Site {
    samples: u64,
    sampled_bytes: u64,
    by_type: [u32; TYPE_SLOTS],
}

#[derive(Default)]
struct Table {
    sites: HashMap<[usize; DEPTH], Site>,
    samples: u64,
    sampled_bytes: u64,
    by_type: [u64; TYPE_SLOTS],
    inline_trips: u64,
}

crate::perry_thread_local! {
    static UNTIL: Cell<usize> = const { Cell::new(0) };
    static TABLE: RefCell<Table> = RefCell::new(Table::default());
}

/// Read `PERRY_ALLOC_SITE_SAMPLE` once (from `gc_init`). A bare `1` or an
/// unparsable value selects the default interval.
pub(crate) fn init_from_env() {
    let raw = std::env::var("PERRY_ALLOC_SITE_SAMPLE").ok();
    let interval = parse_interval(raw.as_deref());
    if interval == 0 {
        return;
    }
    INTERVAL.store(interval, Ordering::Relaxed);
    eprintln!("[alloc-site] sampling every {interval} bytes of arena allocation");
}

/// The knob's value semantics, as a pure function so the OFF state can be
/// pinned by `gc/tests/env_knob_parse.rs` without touching the process
/// environment (the shared GC-knob vocabulary, #7991): the boolean spellings
/// read through [`crate::gc::env_flag_from_value`] — `1`/`on`/`true`/`yes`
/// select [`DEFAULT_INTERVAL_BYTES`], every OFF spelling and every typo read
/// as OFF; an integer ≥ 2 is the interval in bytes, floored at
/// [`MIN_INTERVAL_BYTES`] so a stray small value cannot turn every allocation
/// into a stack walk.
pub(crate) fn parse_interval(raw: Option<&str>) -> usize {
    if crate::gc::env_flag_from_value(raw) {
        return DEFAULT_INTERVAL_BYTES;
    }
    raw.and_then(|r| r.trim().parse::<usize>().ok())
        .filter(|&v| v >= 2)
        .map_or(0, |v| v.max(MIN_INTERVAL_BYTES))
}

/// Smallest interval an explicit integer can select.
pub(crate) const MIN_INTERVAL_BYTES: usize = 256;

/// A runtime-path allocation of `total` bytes (header included) of
/// `obj_type` is about to happen.
#[inline(always)]
pub(crate) fn note(total: usize, obj_type: u8) {
    let interval = INTERVAL.load(Ordering::Relaxed);
    if interval == 0 {
        return;
    }
    note_slow(total, obj_type, interval);
}

/// Charge `bytes` to the countdown; true when a sample is due (the countdown
/// is then re-armed with a full interval).
#[inline]
fn countdown(bytes: usize, interval: usize) -> bool {
    UNTIL.with(|u| {
        let left = u.get();
        if left > bytes {
            u.set(left - bytes);
            false
        } else {
            u.set(interval);
            true
        }
    })
}

#[cold]
#[inline(never)]
fn note_slow(total: usize, obj_type: u8, interval: usize) {
    if countdown(total, interval) {
        sample(total, obj_type, false);
    }
}

/// A site is writing the inline bump offset back to its arena block:
/// `inline_offset - block_offset` bytes were allocated by the compiled fast
/// path since the last sync. Charge them to the shared countdown.
#[inline(always)]
pub(crate) fn note_inline_sync(block_offset: usize, inline_offset: usize) {
    let interval = INTERVAL.load(Ordering::Relaxed);
    if interval == 0 || inline_offset <= block_offset {
        return;
    }
    note_inline_slow(inline_offset - block_offset, interval);
}

#[cold]
#[inline(never)]
fn note_inline_slow(bytes: usize, interval: usize) {
    if countdown(bytes, interval) {
        // The inline allocator only births class instances (`GC_TYPE_OBJECT`
        // with a per-site header image); the size charged is the whole burst.
        sample(bytes, crate::gc::GC_TYPE_OBJECT, true);
    }
}

/// While sampling, cap the mirrored inline block limit at the bytes left
/// before the next sample, so the compiled fast path returns to the runtime
/// (`js_inline_arena_slow_alloc`, whose write-back charges the burst) exactly
/// when a sample is due. Identity when off.
#[inline(always)]
pub(crate) fn inline_limit(offset: usize, block_size: usize) -> usize {
    let interval = INTERVAL.load(Ordering::Relaxed);
    if interval == 0 {
        return block_size;
    }
    let left = UNTIL.with(Cell::get).max(1);
    block_size.min(offset.saturating_add(left))
}

fn sample(total: usize, obj_type: u8, inline_trip: bool) {
    let mut pcs = [0usize; crate::error::MAX_CAPTURED_FRAMES];
    let n = crate::error::capture_ips(&mut pcs);
    // Frame 0 is the return into this sampler; frame 1 is the allocation
    // helper (or, on the inline path, the write-back site), and the chain
    // walks out to the compiled JS function that owns the allocation.
    let mut key = [0usize; DEPTH];
    for (slot, pc) in key.iter_mut().zip(&pcs[1.min(n)..n]) {
        *slot = *pc;
    }
    let t = (obj_type as usize).min(TYPE_SLOTS - 1);
    TABLE.with(|table| {
        let Ok(mut table) = table.try_borrow_mut() else {
            return;
        };
        table.samples += 1;
        table.sampled_bytes += total as u64;
        table.by_type[t] += 1;
        if inline_trip {
            table.inline_trips += 1;
        }
        let site = table.sites.entry(key).or_default();
        site.samples += 1;
        site.sampled_bytes += total as u64;
        site.by_type[t] += 1;
    });
}

fn type_name(t: usize) -> &'static str {
    crate::gc::gc_type_info(t as u8).map_or("?", |i| i.name)
}

/// Print the cumulative histogram. `label` names the occasion.
pub(crate) fn report(label: &str) {
    let interval = INTERVAL.load(Ordering::Relaxed);
    if interval == 0 {
        return;
    }
    TABLE.with(|table| {
        let Ok(table) = table.try_borrow() else {
            return;
        };
        if table.samples == 0 {
            return;
        }
        let est_total = table.samples * interval as u64;
        eprintln!(
            "[alloc-site] {label}: interval={interval} samples={} est_bytes={est_total} inline_trips={} sites={}",
            table.samples,
            table.inline_trips,
            table.sites.len()
        );
        let mut types: Vec<(usize, u64)> = table
            .by_type
            .iter()
            .enumerate()
            .filter(|(_, &c)| c > 0)
            .map(|(t, &c)| (t, c))
            .collect();
        types.sort_by_key(|&(_, c)| std::cmp::Reverse(c));
        let mut line = String::from("[alloc-site]   by-type:");
        for (t, c) in types {
            line.push_str(&format!(
                " {}={}MB",
                type_name(t),
                c * interval as u64 / (1024 * 1024)
            ));
        }
        eprintln!("{line}");
        let mut sites: Vec<(&[usize; DEPTH], &Site)> = table.sites.iter().collect();
        sites.sort_by_key(|(_, s)| std::cmp::Reverse(s.samples));
        for (key, s) in sites.iter().take(30) {
            let n = key.iter().position(|&p| p == 0).unwrap_or(DEPTH);
            let mut top_types: Vec<(usize, u32)> = s
                .by_type
                .iter()
                .enumerate()
                .filter(|(_, &c)| c > 0)
                .map(|(t, &c)| (t, c))
                .collect();
            top_types.sort_by_key(|&(_, c)| std::cmp::Reverse(c));
            let types: Vec<String> = top_types
                .iter()
                .take(3)
                .map(|(t, c)| format!("{}:{}%", type_name(*t), *c as u64 * 100 / s.samples))
                .collect();
            eprintln!(
                "[alloc-site]   est_bytes={} samples={} mean_obj={} types={} site={}",
                s.samples * interval as u64,
                s.samples,
                s.sampled_bytes / s.samples,
                types.join(","),
                crate::error::describe_chain(&key[..n], 5)
            );
        }
    });
}
