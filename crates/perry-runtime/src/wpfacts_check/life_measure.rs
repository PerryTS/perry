//! The lifetime measurement (LIFE I1, `PERRY_LIFE_MEASURE` at compile time;
//! this build only).
//!
//! Codegen calls `perry_life_birth` at every census birth, `perry_life_enter`
//! / `perry_life_return` where a body's activation starts and returns,
//! `perry_life_iter` where an iteration of a loop holding a birth ends, and
//! `perry_life_turn` after each microtask checkpoint of the entry's event
//! loop. Births and bytes are counted per site exactly. Some births are
//! sampled (per-site rate 1 for the first 32 samples, halved each time the
//! site's sample count doubles, floor 1/65536; `PERRY_LIFE_MEASURE_RATE=1`:
//! every birth). A sample is a claim of the check mode's table (state
//! `Probe`) and moves with its object. At each of its boundaries (iteration,
//! return, the caller's return, turn end), in order, a probe decides it: a
//! forced copied minor while it is young (reached: alive, with the walk phase
//! that reached it; not reached: dead there and at every later boundary), the
//! turn's synchronous full while it is old (marked or not). Any copied minor
//! that does not reach a sample proves it dead from its next boundary on. An
//! old sample at another boundary, or one whose claims were dropped by an
//! unobserved collection, is `unresolved` there, never guessed.
//!
//! Activations are told apart by the frame pointer of the compiled body
//! that calls the hook: a hook whose caller frame is above the top
//! activation shows that the activation returned without `perry_life_return`
//! (a throw unwound it); its boundaries are probed then and counted `late`
//! (survival exact, death "dead by then").
//!
//! The run writes `PERRY_LIFE_MEASURE=<path>` (run time) at each turn end:
//! one row per (site, boundary). `PERRY_FACTS_SABOTAGE=lifekeep` (negative
//! control): every sample is also stored in a global root.
use std::collections::HashMap;

use super::life::{with, Claim, State, Table};

const ITER: usize = 0;
const RET: usize = 1;
const CALLER: usize = 2;
const TURN: usize = 3;
const DONE: u8 = 4;
const BOUNDARIES: [&str; 4] = ["iteration", "return", "caller", "turn"];
const HOLDERS: [&str; 7] = ["frame", "outer_frame", "shadow", "global", "remembered", "object", "other"];

#[derive(Default, Clone, Copy)]
struct Bnd {
    samples: u64,
    alive: u64,
    alive_w: f64,
    dead_w: f64,
    alive_bytes_w: f64,
    dead_bytes_w: f64,
    late: u64,
    unresolved: u64,
    holders: [u64; 7],
}

#[derive(Default)]
struct Site {
    births: u64,
    bytes: u64,
    unborn: u64,
    sampled: u64,
    b: [Bnd; 4],
}

#[derive(Clone, Copy, Default)]
struct Sample {
    site: u64,
    addr: usize,
    bytes: u64,
    w: f64,
    loop_id: u64,
    /// The next boundary to decide (`DONE` when decided or freed).
    next: u8,
    /// Boundaries that apply (bit per boundary).
    applies: u8,
    gen: u32,
    old: bool,
    reached: bool,
    holder: u8,
}

struct Act {
    fp: usize,
    /// Samples whose next boundary is this activation's (iteration and
    /// return of its own births, the caller boundary of its callees').
    pend: Vec<(u32, u32)>,
}

#[derive(Default)]
pub(super) struct Measure {
    sites: HashMap<u64, Site>,
    samples: Vec<Sample>,
    free: Vec<u32>,
    acts: Vec<Act>,
    /// Iteration-pending samples born with no activation.
    root: Vec<(u32, u32)>,
    turn: Vec<(u32, u32)>,
    rng: u64,
    global_shift: u32,
    probe_ns: u64,
    probes: u64,
    probe_minors: u64,
    turn_fulls: u64,
    late_pops: u64,
    start: Option<std::time::Instant>,
    /// During a probe: the probed activation's frame pointer.
    probe_fp: usize,
    in_probe: bool,
    probe_late: bool,
    /// During the turn's full: the old samples it decides.
    full: Vec<(u32, u32)>,
    full_seen: bool,
    births_at_write: u64,
    births: u64,
    keep_roots: Vec<Box<u64>>,
}

fn knob(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn rate_one() -> bool {
    static R: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *R.get_or_init(|| knob("PERRY_LIFE_MEASURE_RATE").as_deref() == Some("1"))
}

fn keep() -> bool {
    static K: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *K.get_or_init(|| knob("PERRY_FACTS_SABOTAGE").as_deref() == Some("lifekeep"))
}

/// The frame pointer of the compiled body that called the hook calling
/// this (`#[inline(always)]`: read in the hook's own frame).
#[inline(always)]
fn caller_fp() -> usize {
    let fp: usize;
    #[cfg(target_arch = "aarch64")]
    unsafe {
        std::arch::asm!("mov {}, x29", out(reg) fp, options(nomem, nostack, preserves_flags));
    }
    #[cfg(target_arch = "x86_64")]
    unsafe {
        std::arch::asm!("mov {}, rbp", out(reg) fp, options(nomem, nostack, preserves_flags));
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        fp = 0;
    }
    if fp == 0 {
        return 0;
    }
    unsafe { *(fp as *const usize) }
}

impl Measure {
    fn next_rand(&mut self) -> u64 {
        if self.rng == 0 {
            self.rng = 0x9E37_79B9_7F4A_7C15;
        }
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn live(&self, (i, g): (u32, u32)) -> bool {
        self.samples.get(i as usize).is_some_and(|s| s.gen == g && s.next != DONE)
    }

    fn bnd(&mut self, i: u32, b: usize) -> (&mut Bnd, Sample) {
        let s = self.samples[i as usize];
        (&mut self.sites.entry(s.site).or_default().b[b], s)
    }

    /// Sample `i` is alive at its next boundary `b` (decided by a probe).
    fn alive_at(&mut self, i: u32, b: usize, late: bool) {
        let (bd, s) = self.bnd(i, b);
        bd.samples += 1;
        bd.alive += 1;
        bd.alive_w += s.w;
        bd.alive_bytes_w += s.w * s.bytes as f64;
        bd.holders[s.holder as usize] += 1;
        if late {
            bd.late += 1;
        }
    }

    /// Sample `i` is dead from its next boundary on.
    fn dead_from(&mut self, i: u32, late: bool) {
        let s = self.samples[i as usize];
        let site = self.sites.entry(s.site).or_default();
        let mut first = true;
        for b in (s.next as usize)..4 {
            if s.applies & (1 << b) == 0 {
                continue;
            }
            let bd = &mut site.b[b];
            bd.samples += 1;
            bd.dead_w += s.w;
            bd.dead_bytes_w += s.w * s.bytes as f64;
            if first && late {
                bd.late += 1;
            }
            first = false;
        }
        self.release(i);
    }

    /// Sample `i` cannot be decided from its next boundary on (`keep_turn`:
    /// an old sample still waits for the turn's full).
    fn unresolved_from(&mut self, i: u32, keep_turn: bool) {
        let s = self.samples[i as usize];
        let site = self.sites.entry(s.site).or_default();
        for b in (s.next as usize)..4 {
            if s.applies & (1 << b) != 0 && !(keep_turn && b == TURN) {
                site.b[b].unresolved += 1;
            }
        }
        if keep_turn {
            self.samples[i as usize].next = TURN as u8;
            let g = s.gen;
            self.turn.push((i, g));
        } else {
            self.release(i);
        }
    }

    fn release(&mut self, i: u32) {
        let s = &mut self.samples[i as usize];
        s.next = DONE;
        s.gen = s.gen.wrapping_add(1);
        self.free.push(i);
    }

    /// The first boundary at or after `from` that applies to sample `i`.
    fn advance(&mut self, i: u32, from: usize) -> usize {
        let s = &mut self.samples[i as usize];
        let mut b = from;
        while b < TURN && s.applies & (1 << b) == 0 {
            b += 1;
        }
        s.next = b as u8;
        b
    }

    /// Files sample `i` (next boundary `b`) where that boundary is decided:
    /// iteration and return with its own activation `own`, caller with the
    /// activation below it, turn on the turn list.
    fn file(&mut self, i: u32, own: Option<usize>) {
        let b = self.samples[i as usize].next as usize;
        let key = (i, self.samples[i as usize].gen);
        match (b, own) {
            (ITER | RET, Some(a)) => self.acts[a].pend.push(key),
            (ITER, None) => self.root.push(key),
            (CALLER, Some(a)) if a > 0 => self.acts[a - 1].pend.push(key),
            _ => {
                self.samples[i as usize].next = TURN as u8;
                self.turn.push(key);
            }
        }
    }
}

/// The table, installed at the first hook.
fn table<R>(f: impl FnOnce(&mut Table) -> R) -> Option<R> {
    super::life::ensure();
    with(|t| {
        super::life::sync(t);
        if t.measure.start.is_none() {
            t.measure.start = Some(std::time::Instant::now());
        }
        f(t)
    })
}

/// Pops the activations deeper than the frame `fp` (`inclusive`: also one at
/// `fp`): each returned without its hook (or, inclusive, a sibling call
/// reuses its frame). Their boundaries are probed now, `late`.
fn pop_deeper(fp: usize, inclusive: bool) {
    loop {
        let popped = table(|t| {
            let top = t.measure.acts.last()?;
            if top.fp < fp || (inclusive && top.fp == fp) {
                t.measure.late_pops += 1;
                Some(())
            } else {
                None
            }
        })
        .flatten();
        if popped.is_none() {
            return;
        }
        pop_top(true);
    }
}

/// The top activation returns: its samples' iteration and return, and its
/// callees' samples' caller boundary, are decided by one probe.
fn pop_top(late: bool) {
    let Some((pend, fp)) = table(|t| {
        let a = t.measure.acts.last_mut()?;
        Some((std::mem::take(&mut a.pend), a.fp))
    })
    .flatten() else {
        return;
    };
    probe(&pend, fp, late);
    let _ = table(|t| {
        let m = &mut t.measure;
        let depth = m.acts.len() - 1;
        for &(i, g) in &pend {
            if !m.live((i, g)) {
                continue;
            }
            let b = m.samples[i as usize].next as usize;
            if b == TURN {
                // Old: already on the turn list.
                continue;
            }
            if b == ITER {
                m.alive_at(i, ITER, late);
                m.advance(i, RET);
            }
            let b2 = m.samples[i as usize].next as usize;
            if b2 == RET || (b == CALLER) {
                m.alive_at(i, b2, late);
                m.advance(i, b2 + 1);
            }
            // Its next boundary is the caller's (the activation below) or the turn.
            m.file(i, Some(depth));
        }
        m.acts.pop();
    });
}

/// Decides the pending samples `keys` at a boundary of the activation at
/// frame `fp`: old ones are unresolved (except at turn), young ones go
/// through one forced copied minor. After it, a sample still live was reached.
fn probe(keys: &[(u32, u32)], fp: usize, late: bool) {
    let young = table(|t| {
        let m = &mut t.measure;
        let mut young = 0;
        for &k in keys {
            if !m.live(k) {
                continue;
            }
            let s = &mut m.samples[k.0 as usize];
            if s.old {
                m.unresolved_from(k.0, true);
            } else {
                s.reached = false;
                young += 1;
            }
        }
        if young > 0 {
            m.in_probe = true;
            m.probe_fp = fp;
            m.probe_late = late;
            m.probes += 1;
        }
        young
    })
    .unwrap_or(0);
    if young == 0 {
        return;
    }
    let began = std::time::Instant::now();
    let before = crate::gc::diagnostic_hooks::life_collection_count();
    crate::gc::gc_collect_minor();
    let ran = crate::gc::diagnostic_hooks::life_collection_count() != before;
    let _ = with(|t| {
        let m = &mut t.measure;
        m.in_probe = false;
        m.probe_minors += 1;
        m.probe_ns += began.elapsed().as_nanos() as u64;
        if !ran {
            for &k in keys {
                if m.live(k) && !m.samples[k.0 as usize].old {
                    m.unresolved_from(k.0, false);
                }
            }
        }
        budget(m);
    });
}

/// The probe budget: probe time may not exceed 4x mutator time; over it,
/// every site's rate halves.
fn budget(m: &mut Measure) {
    if rate_one() || m.probes % 64 != 0 {
        return;
    }
    let Some(start) = m.start else { return };
    let total = start.elapsed().as_nanos() as u64;
    let mutator = total.saturating_sub(m.probe_ns);
    if total > 50_000_000 && m.probe_ns > 4 * mutator && m.global_shift < 16 {
        m.global_shift += 1;
    }
}

/// A copied minor reached sample `i` (now at `new`).
pub(super) fn reached(t: &mut Table, i: u32, new: usize) {
    let in_probe = t.measure.in_probe;
    let fp = t.measure.probe_fp;
    let Some(s) = t.measure.samples.get_mut(i as usize) else { return };
    s.addr = new;
    if !crate::arena::pointer_in_nursery(new) {
        s.old = true;
    }
    if in_probe && !s.reached {
        s.reached = true;
        let (phase, frame) = crate::gc::diagnostic_hooks::life_holder();
        let phase = phase.unwrap_or("");
        s.holder = if phase.contains("native") {
            match frame {
                Some((_, _, slot)) if fp != 0 && slot < fp + 16 => 0,
                _ => 1,
            }
        } else if phase.contains("shadow") {
            2
        } else if phase.contains("global") {
            3
        } else if phase.contains("remembered") {
            4
        } else if phase.contains("worklist") {
            5
        } else {
            6
        };
    }
    if s.old {
        // A minor never sees it again: its claim leaves the table.
        t.claims.remove(&new);
    }
}

/// A copied minor did not reach sample `i`: dead from its next boundary on.
pub(super) fn died(t: &mut Table, i: u32) {
    let m = &mut t.measure;
    if m.samples.get(i as usize).is_some_and(|s| s.next != DONE) {
        let late = m.in_probe && m.probe_late;
        m.dead_from(i, late);
    }
}

/// An unobserved collection dropped sample `i`'s claim.
pub(super) fn dropped(t: &mut Table, i: u32) {
    let m = &mut t.measure;
    if m.samples.get(i as usize).is_some_and(|s| s.next != DONE) {
        m.unresolved_from(i, false);
    }
}

/// A synchronous full has complete marks: an unmarked old sample is dead
/// from its next boundary on; in the turn's own full, a marked one waiting
/// for the turn is alive at it.
pub(super) fn full_marked() {
    let _ = with(|t| {
        let m = &mut t.measure;
        let turn_full = !m.full.is_empty();
        m.full_seen = true;
        m.full.clear();
        for i in 0..m.samples.len() as u32 {
            let s = m.samples[i as usize];
            if s.next == DONE || !s.old {
                continue;
            }
            let header = (s.addr - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            let marked = unsafe { (*header).gc_flags & (crate::gc::GC_FLAG_MARKED | crate::gc::GC_FLAG_PINNED) != 0 };
            if !marked {
                m.dead_from(i, false);
            } else if turn_full && s.next as usize == TURN {
                m.samples[i as usize].holder = 6;
                m.alive_at(i, TURN, false);
                m.release(i);
            }
        }
    });
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn perry_life_birth(bits: i64, site: i64, loop_id: i64) {
    let fp = caller_fp();
    let bits = bits as u64;
    let sampled = table(|t| {
        let m = &mut t.measure;
        if m.births == 0 {
            // `PERRY_LIFE_MEASURE_SHIFT=k`: start every site at rate 2^-k more.
            m.global_shift = knob("PERRY_LIFE_MEASURE_SHIFT").and_then(|v| v.parse().ok()).unwrap_or(0).min(16);
        }
        m.births += 1;
        let rand = m.next_rand();
        let global = m.global_shift;
        let s = m.sites.entry(site as u64).or_default();
        s.births += 1;
        if bits & super::TAG_MASK != super::POINTER_TAG {
            s.unborn += 1;
            return None;
        }
        let addr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
        let bytes = unsafe { (*((addr - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader)).size } as u64;
        s.bytes += bytes;
        let own = if s.sampled < 32 { 0 } else { 64 - (s.sampled / 32).leading_zeros() };
        let shift = if rate_one() { 0 } else { (own + global).min(16) };
        if rand & ((1u64 << shift) - 1) != 0 {
            return None;
        }
        s.sampled += 1;
        Some((addr, bytes, (1u64 << shift) as f64))
    })
    .flatten();
    let Some((addr, bytes, w)) = sampled else { return };
    pop_deeper(fp, false);
    let _ = table(|t| {
        let m = &mut t.measure;
        let depth = m.acts.len();
        let own = (depth > 0 && m.acts[depth - 1].fp == fp).then(|| depth - 1);
        let mut applies = 1 << TURN;
        if loop_id != 0 {
            applies |= 1 << ITER;
        }
        if let Some(a) = own {
            applies |= 1 << RET;
            if a > 0 {
                applies |= 1 << CALLER;
            }
        }
        let i = m.free.pop().unwrap_or_else(|| {
            m.samples.push(Sample::default());
            (m.samples.len() - 1) as u32
        });
        let gen = m.samples[i as usize].gen;
        m.samples[i as usize] = Sample {
            site: site as u64,
            addr,
            bytes,
            w,
            loop_id: loop_id as u64,
            next: 0,
            applies,
            gen,
            old: !crate::arena::pointer_in_nursery(addr),
            reached: false,
            holder: 6,
        };
        m.advance(i, ITER);
        m.file(i, own);
        if m.samples[i as usize].old {
            let b = m.samples[i as usize].next;
            if b as usize != TURN {
                m.unresolved_from(i, true);
            }
        } else {
            t.claims.insert(addr, Claim { site: 0, len: 0, state: State::Probe(i) });
        }
        if keep() {
            let slot = Box::new(bits);
            crate::gc::js_gc_register_global_root(&*slot as *const u64 as i64);
            m.keep_roots.push(slot);
        }
    });
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn perry_life_enter() {
    let fp = caller_fp();
    pop_deeper(fp, true);
    let _ = table(|t| t.measure.acts.push(Act { fp, pend: Vec::new() }));
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn perry_life_return() {
    let fp = caller_fp();
    pop_deeper(fp, false);
    if table(|t| t.measure.acts.last().is_some_and(|a| a.fp == fp)) == Some(true) {
        pop_top(false);
    }
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn perry_life_iter(loop_id: i64) {
    let fp = caller_fp();
    pop_deeper(fp, false);
    let keys = table(|t| {
        let m = &mut t.measure;
        let list = match m.acts.last_mut() {
            Some(a) if a.fp == fp => &mut a.pend,
            None => &mut m.root,
            Some(_) => return Vec::new(),
        };
        let mut hit = Vec::new();
        list.retain(|&(i, g)| {
            let Some(s) = m.samples.get(i as usize) else { return false };
            if s.gen != g || s.next == DONE {
                return false;
            }
            if s.next as usize == ITER && s.loop_id == loop_id as u64 {
                hit.push((i, g));
                return false;
            }
            true
        });
        hit
    })
    .unwrap_or_default();
    if keys.is_empty() {
        return;
    }
    probe(&keys, fp, false);
    let _ = table(|t| {
        let m = &mut t.measure;
        let depth = m.acts.len();
        let own = (depth > 0 && m.acts[depth - 1].fp == fp).then(|| depth - 1);
        for &k in &keys {
            if m.live(k) && m.samples[k.0 as usize].next as usize == ITER {
                m.alive_at(k.0, ITER, false);
                m.advance(k.0, RET);
                m.file(k.0, own);
            }
        }
    });
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn perry_life_turn() {
    // The JS stack is unwound: every activation still listed returned.
    while table(|t| !t.measure.acts.is_empty()) == Some(true) {
        let _ = table(|t| t.measure.late_pops += 1);
        pop_top(true);
    }
    // Iteration-pending samples with no activation: the turn ended their loop.
    let root = table(|t| std::mem::take(&mut t.measure.root)).unwrap_or_default();
    probe(&root, 0, true);
    let _ = table(|t| {
        let m = &mut t.measure;
        for &k in &root {
            if m.live(k) && m.samples[k.0 as usize].next as usize == ITER {
                m.alive_at(k.0, ITER, true);
                m.advance(k.0, RET);
                m.file(k.0, None);
            }
        }
    });
    let turn = table(|t| std::mem::take(&mut t.measure.turn)).unwrap_or_default();
    probe(&turn, 0, false);
    let old = table(|t| {
        let m = &mut t.measure;
        let mut old = Vec::new();
        for &k in &turn {
            if !m.live(k) {
                continue;
            }
            if m.samples[k.0 as usize].old {
                old.push(k);
            } else {
                m.alive_at(k.0, TURN, false);
                m.release(k.0);
            }
        }
        old
    })
    .unwrap_or_default();
    if !old.is_empty() {
        let _ = table(|t| {
            t.measure.full = old.clone();
            t.measure.full_seen = false;
            t.measure.turn_fulls += 1;
        });
        crate::gc::life_collect_full();
        let _ = with(|t| {
            let m = &mut t.measure;
            if !m.full_seen {
                for k in std::mem::take(&mut m.full) {
                    if m.live(k) {
                        m.unresolved_from(k.0, false);
                    }
                }
            }
        });
    }
    let _ = with(write);
}

/// Writes the run's TSV (`PERRY_LIFE_MEASURE=<path>` at run time).
fn write(t: &mut Table) {
    let m = &mut t.measure;
    if m.births == m.births_at_write {
        return;
    }
    m.births_at_write = m.births;
    let Some(path) = knob("PERRY_LIFE_MEASURE").filter(|p| p != "1") else { return };
    let mut out = String::from("site\tbirths\tbytes\tunborn\tsampled\tboundary\tsamples\talive\talive_w\tdead_w\talive_bytes_w\tdead_bytes_w\tlate\tunresolved");
    for h in HOLDERS {
        out.push_str(&format!("\t{h}"));
    }
    out.push('\n');
    let mut ids: Vec<_> = m.sites.keys().copied().collect();
    ids.sort_unstable();
    let (mut births, mut bytes) = (0u64, 0u64);
    for id in ids {
        let s = &m.sites[&id];
        births += s.births;
        bytes += s.bytes;
        for (b, name) in BOUNDARIES.iter().enumerate() {
            let d = &s.b[b];
            out.push_str(&format!(
                "{id}\t{}\t{}\t{}\t{}\t{name}\t{}\t{}\t{:.0}\t{:.0}\t{:.0}\t{:.0}\t{}\t{}",
                s.births, s.bytes, s.unborn, s.sampled, d.samples, d.alive, d.alive_w, d.dead_w,
                d.alive_bytes_w, d.dead_bytes_w, d.late, d.unresolved
            ));
            for h in d.holders {
                out.push_str(&format!("\t{h}"));
            }
            out.push('\n');
        }
    }
    let (arena, large) = crate::arena::allocation_totals();
    let elapsed = m.start.map_or(0, |s| s.elapsed().as_nanos() as u64);
    out.push_str(&format!(
        "#totals\tsite_births={births}\tsite_bytes={bytes}\tarena_bytes={arena}\tlarge_bytes={large}\tprobes={}\tprobe_minors={}\tturn_fulls={}\tlate_pops={}\tprobe_ns={}\telapsed_ns={elapsed}\tglobal_shift={}\tpending={}\n",
        m.probes, m.probe_minors, m.turn_fulls, m.late_pops, m.probe_ns, m.global_shift,
        m.samples.iter().filter(|s| s.next != DONE).count()
    ));
    let _ = std::fs::write(path, out);
}
