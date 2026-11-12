//! LIFE I2 step A: what the allocated bytes are and which of them a census
//! birth site claims (`PERRY_LIFE_ALLOC=<path>` at run time; the
//! instrumented runtime only, with `PERRY_LIFE_MEASURE`).
//!
//! Exact, no sampling: every allocation the collector serves is counted by
//! its collector object type, on the runtime path where
//! [`super::alloc_sample::note`] sees it (runtime bumps are routed through
//! that path while this is on, as for the site sampler) and, for the
//! compiled inline bump path, by walking the headers of each inline burst
//! ([`inline_burst`]; a census birth walks the burst so far first).
//!
//! Covered, ONE predicate for the exact count and the samples: every
//! allocated object enters the epoch map under its address where its fresh
//! header is known ([`placed`], [`old_birth`], the inline walks, the inline
//! slow call); a census birth (`perry_life_birth`) that finds its address in
//! the map takes the entry out: that object is covered (exact covered bytes
//! by the entry's type and size, and its samples covered). A collection
//! ([`minor`]) ends the epoch: entries still in the map are uncovered. A
//! runtime note whose header no path reports is `unplaced` (never covered).
//! Claims that find no entry (a second claim, an object a collection moved
//! before its claim, an object no counted path allocated) cover nothing and
//! are counted (`claims_outside`; of them a second claim in the same epoch:
//! `claims_repeat`).
//!
//! Sampled: byte-proportional and independent of the order the paths are
//! counted in: an object of `b` bytes gets `b / I` samples plus one more
//! with probability `(b mod I) / I` (`I` = `PERRY_LIFE_ALLOC_SAMPLE`, default
//! 16 KiB, at least 1 byte; at 1 every byte is a sample), so samples x `I`
//! estimates bytes per type and state without bias. A sampled object keeps
//! its RAW native return addresses (resolved offline against the measured
//! binary; the TSV names the image base).
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

static ON: AtomicBool = AtomicBool::new(false);
static INTERVAL: AtomicUsize = AtomicUsize::new(0);
/// Every collector object type (`GC_TYPE_MAX` + 1): a narrower table folds
/// the high kinds (overflow storage, record elements, symbols, ...) together.
const TYPES: usize = crate::gc::GC_TYPE_MAX as usize + 1;
const DEPTH: usize = 12;
pub(crate) const DEFAULT_INTERVAL: usize = 16 * 1024;

#[derive(Clone, Copy)]
struct Sample {
    inline: bool,
    t: u8,
    bytes: u32,
    /// Samples this object carries (0: counted, not sampled).
    n: u32,
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
    births_seen: u64,
    births_other: u64,
    /// Claims that found no entry in the epoch map: [count, bytes], and the
    /// part of them that claimed an object already claimed in this epoch.
    claims_outside: [u64; 2],
    claims_repeat: [u64; 2],
    /// Addresses claimed in this epoch (diagnostic for `claims_repeat`).
    claimed: std::collections::HashSet<usize>,
    /// Runtime notes whose header no path reported: [count, bytes].
    unplaced: [u64; 2],
    walk_breaks: u64,
    walk_bytes_lost: u64,
    birth_walks: u64,
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
    /// The last runtime note whose header is not yet written.
    pending: Option<(usize, Sample)>,
    /// A refused attempt's pending note, adopted by the retry's note.
    carry: Option<(usize, Sample)>,
    /// The object the compiled inline path's slow call just handed out
    /// (`js_inline_arena_slow_alloc`): address, size, samples, stack if
    /// sampled. The compiled caller writes its header after the call, so it
    /// is read at the next event ([`flush_slow`]).
    slow: Option<(usize, usize, u32, [usize; DEPTH])>,
    slow_objects: u64,
    slow_bad: u64,
    /// The epoch map: every object allocated since the last collection and
    /// not yet claimed, by user address.
    epoch: HashMap<usize, Sample>,
    /// A census birth walked the current inline burst up to here (block data,
    /// offset): the write-back walk starts at this point.
    walked: (usize, usize),
    agg: HashMap<(u8, bool, u8, [usize; DEPTH]), [u64; 2]>,
    moved: [u64; 3],
    minors: u64,
    path: String,
    interval: usize,
}

crate::perry_thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
    static RNG: Cell<u64> = const { Cell::new(0x9E37_79B9_7F4A_7C15) };
}

#[inline(always)]
pub(crate) fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// `PERRY_LIFE_ALLOC=<path>`: on, with the sampling interval (bytes).
pub(crate) fn init_from_env() -> usize {
    // LIFE I3: the region measurement numbers objects at the census's event,
    // so it arms the census (no census file unless `PERRY_LIFE_ALLOC` names one).
    let path = std::env::var("PERRY_LIFE_ALLOC").ok().filter(|p| !p.is_empty());
    let Some(path) = path.or_else(|| crate::wpfacts_check::life_regions_on().then(String::new)) else {
        return 0;
    };
    let interval = std::env::var("PERRY_LIFE_ALLOC_SAMPLE")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|&v| v >= 1)
        .unwrap_or(DEFAULT_INTERVAL);
    let _ = STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.path = path;
        s.interval = interval;
        s.runtime = vec![[0; 2]; TYPES];
        s.inline = vec![[0; 2]; TYPES];
        s.covered = vec![[0; 2]; TYPES];
        s.unnoted = vec![[0; 2]; TYPES];
        s.armed_arena_bytes = super::allocation_totals().0 as u64;
    });
    INTERVAL.store(interval, Ordering::Relaxed);
    ON.store(true, Ordering::Relaxed);
    interval
}

/// Samples an object of `bytes` carries: `bytes / I`, plus one with
/// probability `(bytes mod I) / I` (xorshift64, fixed seed: runs repeat).
fn draw(bytes: usize) -> u32 {
    let i = INTERVAL.load(Ordering::Relaxed).max(1);
    let mut n = bytes / i;
    let rem = bytes % i;
    if rem != 0 {
        let r = RNG.with(|c| {
            let mut x = c.get();
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            c.set(x);
            x
        });
        if (r % i as u64) < rem as u64 {
            n += 1;
        }
    }
    n.min(u32::MAX as usize) as u32
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
    if x.n == 0 {
        return;
    }
    let e = s.agg.entry((state, x.inline, x.t, x.pcs)).or_default();
    e[0] += x.n as u64;
    e[1] += x.bytes as u64;
}

/// An object allocated in this epoch, at user address `addr`. An entry
/// already there is an object that died unclaimed (its slot was reused).
fn enter(s: &mut State, addr: usize, x: Sample) {
    // LIFE I3: the same event numbers the object for the region measurement.
    if crate::wpfacts_check::life_regions_on() {
        crate::wpfacts_check::life_regions_born(addr, x.bytes, x.n);
    }
    if let Some(old) = s.epoch.insert(addr, x) {
        settle(s, UNCOVERED, old);
    }
}

fn drop_pending(s: &mut State, x: Sample) {
    s.unplaced[0] += 1;
    s.unplaced[1] += x.bytes as u64;
    settle(s, UNPLACED, x);
}

/// A runtime-path allocation of `total` bytes of `t`.
pub(crate) fn runtime(total: usize, t: u8) {
    let n = draw(total);
    let pcs = if n != 0 { capture() } else { [0; DEPTH] };
    let _ = with(|s| {
        flush(s);
        let k = (t as usize).min(TYPES - 1);
        s.runtime[k][0] += 1;
        s.runtime[k][1] += total as u64;
        if let Some((_, x)) = s.pending.take() {
            drop_pending(s, x);
        }
        if let Some((want, x)) = s.carry.take() {
            if want == total && x.t == t {
                // The retry of a refused attempt: the object drew once.
                s.pending = Some((want, x));
                return;
            }
            drop_pending(s, x);
        }
        s.pending = Some((total, Sample { inline: false, t, bytes: total as u32, n, pcs }));
    });
}

/// A fresh header was written at `header` (`total`, `t`) on a noted path:
/// the pending note, if it is this object, enters the epoch map. Another
/// object (a collection's move between the note and the header) leaves the
/// pending note waiting.
pub(crate) fn placed(header: usize, total: usize, t: u8) {
    let _ = with(|s| {
        place(s, header, total, t);
    });
}

fn place(s: &mut State, header: usize, total: usize, t: u8) -> bool {
    match s.pending {
        Some((want, x)) if want == total && x.t == t => {
            s.pending = None;
            enter(s, header + crate::gc::GC_HEADER_SIZE, x);
            true
        }
        _ => false,
    }
}

/// `js_inline_arena_slow_alloc` handed `[addr, addr + size)` to compiled
/// code, which writes the header after the call.
pub(crate) fn slow_alloc(addr: usize, size: usize) {
    let n = draw(size);
    let pcs = if n != 0 { capture() } else { [0; DEPTH] };
    let _ = with(|s| {
        flush(s);
        s.slow = Some((addr, size, n, pcs));
    });
}

/// Count the object of the last inline slow call now that its header is
/// written: called at the next event of any kind (and before every inline
/// write-back, which a collection does first).
pub(crate) fn flush_slow() {
    let _ = with(flush);
}

fn flush(s: &mut State) {
    let Some((addr, size, n, pcs)) = s.slow.take() else { return };
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
    enter(s, addr + crate::gc::GC_HEADER_SIZE, Sample { inline: true, t, bytes: total as u32, n, pcs });
}

/// A fresh old-generation (or collecting-holder) header: noted when the
/// pending note is this object, otherwise a birth no note announced, counted
/// here.
pub(crate) fn old_birth(header: usize, total: usize, t: u8) {
    let noted = with(|s| place(s, header, total, t)).unwrap_or(true);
    if noted {
        return;
    }
    let n = draw(total);
    let pcs = if n != 0 { capture() } else { [0; DEPTH] };
    let _ = with(|s| {
        let k = (t as usize).min(TYPES - 1);
        s.runtime[k][0] += 1;
        s.runtime[k][1] += total as u64;
        s.unnoted[k][0] += 1;
        s.unnoted[k][1] += total as u64;
        enter(s, header + crate::gc::GC_HEADER_SIZE, Sample { inline: false, t, bytes: total as u32, n, pcs });
    });
}

/// The old-page defragmentation moved an object (its new home went through
/// [`old_birth`]): a move, not a birth.
pub(crate) fn evacuated(total: usize, t: u8, old: usize, new: usize) {
    if crate::wpfacts_check::life_regions_on() {
        crate::wpfacts_check::life_regions_evacuated(old, new, total as u32);
    }
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
/// since the last write-back (the write-back itself).
pub(crate) fn inline_burst(data: *mut u8, from: usize, to: usize) {
    walk_inline(data, from, to, false);
}

/// Walk the inline headers of `[from, to)` (from where a census birth's walk
/// stopped, when one did): count, draw and enter each object. `at_birth`: a
/// header that does not parse may not be written yet, so the walk stops
/// there and the next walk retries it.
fn walk_inline(data: *mut u8, from: usize, to: usize, at_birth: bool) {
    if data.is_null() {
        return;
    }
    let start = with(|s| match s.walked {
        (d, o) if d == data as usize && from <= o && o <= to => o,
        _ => from,
    })
    .unwrap_or(from);
    let mut objs: Vec<(usize, usize, u8, u32)> = Vec::new();
    let (mut breaks, mut lost) = (0u64, 0u64);
    let mut offset = start;
    while offset < to {
        let aligned = (offset + 7) & !7;
        if aligned >= to {
            offset = to;
            break;
        }
        let header = (data as usize + aligned) as *const crate::gc::GcHeader;
        let (t, total) = unsafe { ((*header).obj_type, (*header).size as usize) };
        if total < crate::gc::GC_HEADER_SIZE || total > to - aligned {
            if at_birth {
                offset = aligned;
            } else {
                breaks += 1;
                lost += (to - aligned) as u64;
                offset = to;
            }
            break;
        }
        objs.push((data as usize + aligned + crate::gc::GC_HEADER_SIZE, total, t, draw(total)));
        offset = aligned + total;
    }
    let pcs = if objs.iter().any(|o| o.3 != 0) { capture() } else { [0usize; DEPTH] };
    let _ = with(|s| {
        flush(s);
        s.walked = if at_birth { (data as usize, offset.min(to)) } else { (0, 0) };
        s.walk_breaks += breaks;
        s.walk_bytes_lost += lost;
        for (addr, total, t, n) in objs {
            let k = (t as usize).min(TYPES - 1);
            s.inline[k][0] += 1;
            s.inline[k][1] += total as u64;
            let x = Sample { inline: true, t, bytes: total as u32, n, pcs: if n != 0 { pcs } else { [0; DEPTH] } };
            enter(s, addr, x);
        }
    });
}

/// The compiled inline path's burst since the last write-back, walked so a
/// census birth finds its object in the epoch map (and at exit, where no
/// write-back follows).
fn walk_pending_inline(at_birth: bool) {
    let (data, from, to) = unsafe {
        let st = &*super::hot_inline_state();
        if st.data.is_null() {
            return;
        }
        let arena = &*super::hot_arena();
        let Some(b) = arena.blocks.get(arena.current) else { return };
        if b.data != st.data || st.offset <= b.offset {
            return;
        }
        (st.data, b.offset, st.offset)
    };
    if at_birth {
        let _ = with(|s| s.birth_walks += 1);
    }
    walk_inline(data, from, to, at_birth);
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
    walk_pending_inline(true);
    let _ = with(|s| {
        s.births_seen += 1;
        let first = s.claimed.insert(addr);
        match s.epoch.remove(&addr) {
            Some(x) => {
                let k = (x.t as usize).min(TYPES - 1);
                s.covered[k][0] += 1;
                s.covered[k][1] += x.bytes as u64;
                settle(s, COVERED, x);
            }
            None => {
                let header = (addr - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
                let size = unsafe { (*header).size as u64 };
                s.claims_outside[0] += 1;
                s.claims_outside[1] += size;
                if !first {
                    s.claims_repeat[0] += 1;
                    s.claims_repeat[1] += size;
                }
            }
        }
    });
}

/// A collection ended: objects of the epoch nobody claimed are uncovered.
pub(crate) fn minor() {
    let _ = with(|s| {
        let open: Vec<Sample> = s.epoch.drain().map(|(_, x)| x).collect();
        for x in open {
            settle(s, UNCOVERED, x);
        }
        s.walked = (0, 0);
        s.claimed.clear();
    });
}

fn type_name(t: usize) -> String {
    crate::gc::gc_type_info(t as u8).map_or_else(|| format!("type{t}"), |i| i.name.to_string())
}

/// LIFE I3: a region hook's point: every object allocated so far has been
/// seen (the inline slow call flushed, the pending inline burst walked).
pub(crate) fn sync_point() {
    if on() {
        flush_slow();
        walk_pending_inline(true);
    }
}

/// The byte-sample interval.
pub(crate) fn interval() -> usize {
    INTERVAL.load(Ordering::Relaxed)
}

/// Process exit: write the TSV.
pub(crate) fn write() {
    flush_slow();
    walk_pending_inline(false);
    crate::wpfacts_check::life_regions_exit();
    minor();
    let base = image_base();
    let (arena, large) = super::allocation_totals();
    let _ = with(|s| {
        if let Some((_, x)) = s.pending.take() {
            drop_pending(s, x);
        }
        if let Some((_, x)) = s.carry.take() {
            drop_pending(s, x);
        }
        let mut out = format!(
            "#meta\timage_base={base:#x}\tinterval={}\tarena_bytes={arena}\tlarge_bytes={large}\tbirths_seen={}\tbirths_other={}\twalk_breaks={}\twalk_bytes_lost={}\tbirth_walks={}\tclaims_outside={}\tclaims_outside_bytes={}\tclaims_repeat={}\tclaims_repeat_bytes={}\tunplaced={}\tunplaced_bytes={}\tcollections={}\tinline_slow_objects={}\tinline_slow_bad={}\n",
            s.interval, s.births_seen, s.births_other, s.walk_breaks, s.walk_bytes_lost, s.birth_walks,
            s.claims_outside[0], s.claims_outside[1], s.claims_repeat[0], s.claims_repeat[1], s.unplaced[0], s.unplaced[1],
            crate::gc::diagnostic_hooks::life_collection_count(), s.slow_objects, s.slow_bad
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
        if !s.path.is_empty() {
            let _ = std::fs::write(&s.path, out);
        }
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
