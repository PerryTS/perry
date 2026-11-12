//! LIFE I2 step A: what the allocated bytes are and which of them a census
//! birth site claims (`PERRY_LIFE_ALLOC=<path>` at run time; the
//! instrumented runtime only, with `PERRY_LIFE_MEASURE`).
//!
//! Exact, no sampling: every allocation the collector serves is counted by
//! its collector object type, on the runtime path where
//! [`super::alloc_sample::note`] sees it (runtime bumps are routed through
//! that path while this is on, as for the site sampler) and, for the
//! compiled inline bump path, by walking the headers of each inline burst
//! when the block offset is written back ([`inline_burst`]). Covered bytes
//! are counted at `perry_life_birth` from the claimed object's header.
//! Uncovered = allocated − covered, per type.
//!
//! Sampled: byte-proportional (one sample per `PERRY_LIFE_ALLOC_SAMPLE`
//! bytes, default 16 KiB, on the sampler's shared countdown) — the object
//! that crosses the boundary keeps its RAW native return addresses (resolved
//! offline against the measured binary; the TSV names the image base) and
//! is marked covered when a birth claims its address before the next
//! collection, uncovered otherwise. A runtime-path sample learns its address
//! where its fresh header is written ([`placed`], [`old_birth`]: young bump,
//! slow path, free-list reuse, old generation, longlived, collecting holder);
//! one whose header no path reports stays `unplaced`.
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

static ON: AtomicBool = AtomicBool::new(false);
/// Every collector object type (`GC_TYPE_MAX` + 1): a narrower table folds
/// the high kinds (overflow storage, record elements, symbols, ...) together.
const TYPES: usize = crate::gc::GC_TYPE_MAX as usize + 1;
const DEPTH: usize = 12;
const RING: usize = 256;
pub(crate) const DEFAULT_INTERVAL: usize = 16 * 1024;

#[derive(Clone, Copy)]
struct Sample {
    inline: bool,
    t: u8,
    bytes: u32,
    pcs: [usize; DEPTH],
}

/// Sample states in the output.
const COVERED: u8 = 0;
const UNCOVERED: u8 = 1;
const UNPLACED: u8 = 2;

#[derive(Default)]
struct State {
    /// [count, bytes] per type: runtime path, inline path, covered.
    runtime: Vec<[u64; 2]>,
    inline: Vec<[u64; 2]>,
    covered: Vec<[u64; 2]>,
    covered_inline_samples: u64,
    births_seen: u64,
    births_other: u64,
    walk_breaks: u64,
    walk_bytes_lost: u64,
    /// Bytes a no-collection attempt noted and then refused (its caller
    /// retries on a collecting path, which notes the same object again).
    refused: u64,
    /// Fresh old-generation / holder births no runtime note announced
    /// (counted into `runtime` by type), and the part of them that was an
    /// old-page defragmentation move (taken back out).
    unnoted: Vec<[u64; 2]>,
    evacuated: u64,
    /// Collector arena bytes already allocated when the knob armed.
    armed_arena_bytes: u64,
    pending: Option<(usize, Sample)>,
    /// A refused attempt's pending sample, adopted by the retry's note.
    carry: Option<(usize, Sample)>,
    /// The last runtime note (total, type) whose header is not yet written:
    /// an old-generation birth matching it is that noted object.
    noted: Option<(usize, u8)>,
    /// The object the compiled inline path's slow call just handed out
    /// (`js_inline_arena_slow_alloc`): address, size, sample stack if due.
    /// The compiled caller writes its header after the call, so it is read at
    /// the next event ([`flush_slow`]).
    slow: Option<(usize, usize, Option<[usize; DEPTH]>)>,
    slow_objects: u64,
    slow_bad: u64,
    /// Samples placed, awaiting a birth: address -> (collections, sample).
    open: HashMap<usize, (u64, Sample)>,
    /// Recent births (address, collections), for inline samples found after
    /// their birth.
    born: Vec<(usize, u64)>,
    born_at: usize,
    agg: HashMap<(u8, bool, u8, [usize; DEPTH]), [u64; 2]>,
    moved: [u64; 3],
    minors: u64,
    path: String,
    interval: usize,
}

crate::perry_thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

#[inline(always)]
pub(crate) fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// `PERRY_LIFE_ALLOC=<path>`: on, with the sampling interval (bytes) the
/// caller installs on the shared countdown.
pub(crate) fn init_from_env() -> usize {
    let Some(path) = std::env::var("PERRY_LIFE_ALLOC").ok().filter(|p| !p.is_empty()) else {
        return 0;
    };
    let interval = std::env::var("PERRY_LIFE_ALLOC_SAMPLE")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|&v| v >= super::alloc_sample::MIN_INTERVAL_BYTES)
        .unwrap_or(DEFAULT_INTERVAL);
    let _ = STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.path = path;
        s.interval = interval;
        s.born = vec![(0, 0); RING];
        s.runtime = vec![[0; 2]; TYPES];
        s.inline = vec![[0; 2]; TYPES];
        s.covered = vec![[0; 2]; TYPES];
        s.unnoted = vec![[0; 2]; TYPES];
        s.armed_arena_bytes = super::allocation_totals().0 as u64;
    });
    ON.store(true, Ordering::Relaxed);
    interval
}

fn collections() -> u64 {
    crate::gc::diagnostic_hooks::life_collection_count()
}

fn capture() -> [usize; DEPTH] {
    let mut pcs = [0usize; crate::error::MAX_CAPTURED_FRAMES];
    let n = crate::error::capture_ips(&mut pcs);
    let mut out = [0usize; DEPTH];
    for (slot, pc) in out.iter_mut().zip(&pcs[1.min(n)..n]) {
        *slot = *pc;
    }
    out
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|s| s.try_borrow_mut().ok().map(|mut s| f(&mut s)))
}

fn settle(s: &mut State, state: u8, x: Sample) {
    let e = s.agg.entry((state, x.inline, x.t, x.pcs)).or_default();
    e[0] += 1;
    e[1] += x.bytes as u64;
}

/// A runtime-path allocation of `total` bytes of `t` (`sampled`: it crossed
/// the countdown).
pub(crate) fn runtime(total: usize, t: u8, sampled: bool) {
    let pcs = if sampled { Some(capture()) } else { None };
    let _ = with(|s| {
        flush(s);
        s.noted = Some((total, t));
        let k = (t as usize).min(TYPES - 1);
        s.runtime[k][0] += 1;
        s.runtime[k][1] += total as u64;
        if let Some((_, x)) = s.pending.take() {
            settle(s, UNPLACED, x);
        }
        if let Some((want, x)) = s.carry.take() {
            if pcs.is_none() && want == total && x.t == t {
                s.pending = Some((want, x));
                return;
            }
            settle(s, UNPLACED, x);
        }
        if let Some(pcs) = pcs {
            s.pending = Some((total, Sample { inline: false, t, bytes: total as u32, pcs }));
        }
    });
}

/// A fresh header was written at `header` (`total`, `t`) on a noted path:
/// the pending sample, if it is this object, learns its address. Another
/// object (a collection's move between the note and the header) leaves the
/// pending sample waiting.
pub(crate) fn placed(header: usize, total: usize, t: u8) {
    let _ = with(|s| {
        place(s, header, total, t);
    });
}

fn place(s: &mut State, header: usize, total: usize, t: u8) -> bool {
    let noted = s.noted.take() == Some((total, t));
    match s.pending {
        Some((want, x)) if want == total && x.t == t => {
            s.pending = None;
            let gen = collections();
            s.open.insert(header + crate::gc::GC_HEADER_SIZE, (gen, x));
            true
        }
        _ => noted,
    }
}

/// `js_inline_arena_slow_alloc` handed `[addr, addr + size)` to compiled
/// code, which writes the header after the call. `due`: it crossed the
/// sampler's countdown.
pub(crate) fn slow_alloc(addr: usize, size: usize, due: bool) {
    let pcs = if due { Some(capture()) } else { None };
    let _ = with(|s| {
        flush(s);
        s.slow = Some((addr, size, pcs));
    });
}

/// Count the object of the last inline slow call now that its header is
/// written: called at the next event of any kind (and before every inline
/// write-back, which a collection does first).
pub(crate) fn flush_slow() {
    let _ = with(flush);
}

fn flush(s: &mut State) {
    let Some((addr, size, pcs)) = s.slow.take() else { return };
    if addr == 0 {
        return;
    }
    let (t, total) = unsafe {
        let h = addr as *const crate::gc::GcHeader;
        ((*h).obj_type, (*h).size as usize)
    };
    if total != size {
        s.slow_bad += 1;
        return;
    }
    s.slow_objects += 1;
    let k = (t as usize).min(TYPES - 1);
    s.inline[k][0] += 1;
    s.inline[k][1] += total as u64;
    if let Some(pcs) = pcs {
        let gen = collections();
        s.open.insert(addr + crate::gc::GC_HEADER_SIZE, (gen, Sample { inline: true, t, bytes: total as u32, pcs }));
    }
}

/// A fresh old-generation (or collecting-holder) header: noted when the
/// pending note is this object, otherwise a birth no note announced, counted
/// here.
pub(crate) fn old_birth(header: usize, total: usize, t: u8) {
    let _ = with(|s| {
        if place(s, header, total, t) {
            return;
        }
        let k = (t as usize).min(TYPES - 1);
        s.runtime[k][0] += 1;
        s.runtime[k][1] += total as u64;
        s.unnoted[k][0] += 1;
        s.unnoted[k][1] += total as u64;
    });
}

/// The old-page defragmentation moved an object (its new home went through
/// [`old_birth`]): a move, not a birth.
pub(crate) fn evacuated(total: usize, t: u8) {
    let _ = with(|s| {
        let k = (t as usize).min(TYPES - 1);
        s.runtime[k][0] = s.runtime[k][0].saturating_sub(1);
        s.runtime[k][1] = s.runtime[k][1].saturating_sub(total as u64);
        s.unnoted[k][0] = s.unnoted[k][0].saturating_sub(1);
        s.unnoted[k][1] = s.unnoted[k][1].saturating_sub(total as u64);
        s.evacuated += total as u64;
    });
}

/// A no-collection attempt noted `total` of `t` and returned null: the
/// caller's collecting retry notes the same object again.
pub(crate) fn refused(total: usize, t: u8) {
    let _ = with(|s| {
        let k = (t as usize).min(TYPES - 1);
        s.runtime[k][0] = s.runtime[k][0].saturating_sub(1);
        s.runtime[k][1] = s.runtime[k][1].saturating_sub(total as u64);
        s.refused += total as u64;
        if let Some((want, x)) = s.pending {
            if want == total && x.t == t {
                s.pending = None;
                s.carry = Some((want, x));
            }
        }
    });
}

/// The compiled inline path allocated `[from, to)` of the block at `data`
/// since the last write-back. `countdown(bytes)` is the sampler's shared
/// countdown (true: this object is sampled).
pub(crate) fn inline_burst(data: *mut u8, from: usize, to: usize, mut countdown: impl FnMut(usize) -> bool) {
    if data.is_null() {
        return;
    }
    let mut picks: Vec<(usize, usize, u8)> = Vec::new();
    let mut breaks = 0u64;
    let mut lost = 0u64;
    let mut counts: Vec<(u8, usize)> = Vec::new();
    let mut offset = from;
    while offset < to {
        let aligned = (offset + 7) & !7;
        if aligned >= to {
            break;
        }
        let header = (data as usize + aligned) as *const crate::gc::GcHeader;
        let (t, total) = unsafe { ((*header).obj_type, (*header).size as usize) };
        if total < crate::gc::GC_HEADER_SIZE || total > to - aligned {
            breaks += 1;
            lost += (to - aligned) as u64;
            break;
        }
        counts.push((t, total));
        if countdown(total) {
            picks.push((data as usize + aligned + crate::gc::GC_HEADER_SIZE, total, t));
        }
        offset = aligned + total;
    }
    let pcs = if picks.is_empty() { [0usize; DEPTH] } else { capture() };
    let _ = with(|s| {
        flush(s);
        for (t, total) in counts {
            let k = (t as usize).min(TYPES - 1);
            s.inline[k][0] += 1;
            s.inline[k][1] += total as u64;
        }
        s.walk_breaks += breaks;
        s.walk_bytes_lost += lost;
        if picks.is_empty() {
            return;
        }
        let gen = collections();
        for (addr, total, t) in picks {
            let x = Sample { inline: true, t, bytes: total as u32, pcs };
            if s.born.iter().any(|&(a, g)| a == addr && g == gen) {
                s.covered_inline_samples += 1;
                settle(s, COVERED, x);
            } else {
                s.open.insert(addr, (gen, x));
            }
        }
    });
}

/// A census birth claimed `bits` (`perry_life_birth`).
pub(crate) fn birth(bits: u64) {
    let _ = with(flush);
    let tag = bits & 0xFFFF_0000_0000_0000;
    if tag != 0x7FFD_0000_0000_0000 && tag != 0x7FFF_0000_0000_0000 {
        let _ = with(|s| s.births_other += 1);
        return;
    }
    let addr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
    if addr < crate::gc::GC_HEADER_SIZE {
        let _ = with(|s| s.births_other += 1);
        return;
    }
    let header = (addr - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    let (t, total) = unsafe { ((*header).obj_type, (*header).size as u64) };
    let gen = collections();
    let _ = with(|s| {
        s.births_seen += 1;
        let k = (t as usize).min(TYPES - 1);
        s.covered[k][0] += 1;
        s.covered[k][1] += total;
        match s.open.remove(&addr) {
            Some((g, x)) if g == gen => settle(s, COVERED, x),
            Some((_, x)) => settle(s, UNCOVERED, x),
            None => {
                let at = s.born_at % RING;
                s.born[at] = (addr, gen);
                s.born_at += 1;
            }
        }
    });
}

/// A collection ended: placed samples nobody claimed are uncovered.
pub(crate) fn minor() {
    let _ = with(|s| {
        let open: Vec<_> = s.open.drain().map(|(_, (_, x))| x).collect();
        for x in open {
            settle(s, UNCOVERED, x);
        }
        for b in s.born.iter_mut() {
            *b = (0, 0);
        }
    });
}

fn type_name(t: usize) -> String {
    crate::gc::gc_type_info(t as u8).map_or_else(|| format!("type{t}"), |i| i.name.to_string())
}

/// Process exit: write the TSV.
pub(crate) fn write() {
    flush_slow();
    minor();
    let base = image_base();
    let (arena, large) = super::allocation_totals();
    let _ = with(|s| {
        if let Some((_, x)) = s.pending.take() {
            settle(s, UNPLACED, x);
        }
        if let Some((_, x)) = s.carry.take() {
            settle(s, UNPLACED, x);
        }
        let mut out = format!(
            "#meta\timage_base={base:#x}\tinterval={}\tarena_bytes={arena}\tlarge_bytes={large}\tbirths_seen={}\tbirths_other={}\twalk_breaks={}\twalk_bytes_lost={}\tcovered_inline_samples={}\tcollections={}\tinline_slow_objects={}\tinline_slow_bad={}\n",
            s.interval, s.births_seen, s.births_other, s.walk_breaks, s.walk_bytes_lost, s.covered_inline_samples,
            collections(), s.slow_objects, s.slow_bad
        );
        out.push_str(&arena_lines(s));
        let by = super::allocation_totals_by_arena();
        out.push_str(&format!(
            "#arenas\tyoung={}\told={}\tlonglived={}\tsurvivor0={}\tsurvivor1={}\tyoung_large_flag={}\told_large_flag={}\tarmed_arena_bytes={}\trefused_bytes={}\tevacuated_bytes={}\n",
            by[0].0, by[1].0, by[2].0, by[3].0, by[4].0, by[0].1, by[1].1, s.armed_arena_bytes, s.refused, s.evacuated
        ));
        for t in 0..TYPES {
            if s.unnoted[t][0] != 0 {
                out.push_str(&format!("#unnoted\t{}\t{}\t{}\n", type_name(t), s.unnoted[t][0], s.unnoted[t][1]));
            }
        }
        for t in 0..TYPES {
            let (r, i, c) = (s.runtime[t], s.inline[t], s.covered[t]);
            if r[0] + i[0] + c[0] == 0 {
                continue;
            }
            out.push_str(&format!(
                "#type\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                type_name(t), r[0], r[1], i[0], i[1], c[0], c[1]
            ));
        }
        let names = ["covered", "uncovered", "unplaced"];
        for ((state, inline, t, pcs), v) in s.agg.iter() {
            out.push_str(&format!(
                "sample\t{}\t{}\t{}\t{}\t{}",
                names[*state as usize],
                if *inline { "inline" } else { "runtime" },
                type_name(*t as usize),
                v[0],
                v[1]
            ));
            for pc in pcs.iter().take_while(|&&p| p != 0) {
                out.push_str(&format!("\t{pc:#x}"));
            }
            out.push('\n');
        }
        let _ = std::fs::write(&s.path, out);
    });
}

/// The bytes the copying minors moved (promotions, survivor copies,
/// in-place promotions): the arena counters count a move as an allocation,
/// a birth it is not.
pub(crate) fn copied(promoted: usize, copied: usize, in_place: usize) {
    let _ = with(|s| {
        s.moved[0] += promoted as u64;
        s.moved[1] += copied as u64;
        s.moved[2] += in_place as u64;
        s.minors += 1;
    });
}

fn arena_lines(s: &State) -> String {
    format!(
        "#collector\tpromoted_bytes={}\tcopied_bytes={}\tin_place_promoted_bytes={}\tcopying_minors={}\n",
        s.moved[0], s.moved[1], s.moved[2], s.minors
    )
}

fn image_base() -> usize {
    let mut info: libc::Dl_info = unsafe { std::mem::zeroed() };
    let f = write as fn() as *const libc::c_void;
    if unsafe { libc::dladdr(f, &mut info) } == 0 {
        return 0;
    }
    info.dli_fbase as usize
}
