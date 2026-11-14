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
//! Activations nest in hook order (an inlined body enters in its caller's
//! frame). A throw leaves activations without `perry_life_return`: the
//! handler of the catching `try` pops every activation deeper than the depth
//! read at its entry (`perry_life_depth` / `perry_life_catch`); a hook whose
//! caller frame is above the top activation's frame pops it as well (a throw
//! caught outside a measured `try`). Their boundaries are probed then and
//! counted `late` (survival exact, death "dead by then").
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
const BOUNDARIES: [&str; 6] = ["iteration", "return", "caller", "turn", "dyn", "dyn2"];
const HOLDERS: [&str; 7] = [
    "frame",
    "outer_frame",
    "shadow",
    "global",
    "remembered",
    "object",
    "other",
];

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
    /// LIFE I2: the dynamic iteration boundaries (nearest loop iteration live
    /// on the stack at the birth, in any frame; and the one around it).
    d: [Bnd; 2],
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
    /// Dynamic boundaries still to decide (bit 0: `dyn`, bit 1: `dyn2`).
    dyn_: u8,
}

/// One loop iteration in progress (`perry_life_loop` at its body's start to
/// the iteration's end): its activation (index + 1; 0: none), frame and id,
/// and the samples whose dynamic boundary it is (with the bit).
struct LoopIt {
    act: usize,
    fp: usize,
    id: u64,
    pend: Vec<(u32, u32, u8)>,
}

struct Act {
    fp: usize,
    /// The body's id (the site file's `body` row).
    body: u64,
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
    /// LIFE I2: per body id, activations by how they ended: entered,
    /// returned through the hook, popped late at a handler (`catch`), by a
    /// shallower frame's hook (`deeper`), at turn end (`turn`).
    exits: HashMap<u64, [u64; 5]>,
    /// LIFE I2: the loop iterations in progress, innermost last.
    loops: Vec<LoopIt>,
    dyn_probes: u64,
    start: Option<std::time::Instant>,
    /// During a probe: the probed activation's frame pointer.
    probe_fp: usize,
    in_probe: bool,
    probe_late: bool,
    /// During the turn's full: the old samples it decides.
    full: Vec<(u32, u32)>,
    full_seen: bool,
    /// During a probe's confirming full: the samples its minor reached only
    /// through the remembered set or another object (`life::minor_decides`).
    confirm: Vec<(u32, u32)>,
    confirm_fulls: u64,
    /// Confirmed samples by the full's verdict: alive, dead, unresolved (full not observed).
    confirm_alive: u64,
    confirm_dead: u64,
    confirm_unres: u64,
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
        self.samples
            .get(i as usize)
            .is_some_and(|s| s.gen == g && s.next != DONE)
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
        for k in 0..2 {
            if s.dyn_ & (1 << k) != 0 {
                let bd = &mut site.d[k];
                bd.samples += 1;
                bd.dead_w += s.w;
                bd.dead_bytes_w += s.w * s.bytes as f64;
            }
        }
        self.samples[i as usize].dyn_ = 0;
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
        // An old sample is never seen by a minor: its dynamic boundaries
        // cannot be decided either.
        for k in 0..2 {
            if s.dyn_ & (1 << k) != 0 {
                site.d[k].unresolved += 1;
            }
        }
        self.samples[i as usize].dyn_ = 0;
        if keep_turn {
            self.samples[i as usize].next = TURN as u8;
            let g = s.gen;
            self.turn.push((i, g));
        } else {
            self.release(i);
        }
    }

    fn release(&mut self, i: u32) {
        let s = self.samples[i as usize];
        for k in 0..2 {
            if s.dyn_ & (1 << k) != 0 {
                self.sites.entry(s.site).or_default().d[k].unresolved += 1;
            }
        }
        let s = &mut self.samples[i as usize];
        s.dyn_ = 0;
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

/// The top activation ends without its return hook (`how`: the `exits` column).
fn late(t: &mut Table, how: usize) {
    let m = &mut t.measure;
    m.late_pops += 1;
    if let Some(body) = m.acts.last().map(|a| a.body) {
        m.exits.entry(body).or_default()[how] += 1;
    }
}

/// Pops the activations deeper than the frame `fp`: each returned without its
/// hook (a throw caught outside a measured `try`). Their boundaries are
/// probed now, `late`.
fn pop_deeper(fp: usize) {
    loop {
        let popped = table(|t| {
            let top = t.measure.acts.last()?;
            if top.fp < fp {
                let body = top.body;
                t.measure.late_pops += 1;
                t.measure.exits.entry(body).or_default()[3] += 1;
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
    // Its loop iterations in progress end with it.
    let (dyn_keys, _, dyn_young) = match table(|t| {
        let m = &t.measure;
        let n = m.acts.len();
        m.loops.iter().position(|l| l.act >= n && n > 0)
    })
    .flatten()
    {
        Some(k) => take_loops(k),
        None => (Vec::new(), 0, 0),
    };
    let Some((pend, fp)) = table(|t| {
        let a = t.measure.acts.last_mut()?;
        Some((std::mem::take(&mut a.pend), a.fp))
    })
    .flatten() else {
        // No activation: the dynamic boundaries alone.
        if !dyn_keys.is_empty() {
            let ran = probe(&[], &dyn_keys, 0, late, dyn_young);
            settle_loops(&dyn_keys, ran);
        }
        return;
    };
    // One collection decides the return (and the iteration) of this
    // activation's samples and the dynamic boundaries ending with it.
    let ran = probe(&pend, &dyn_keys, fp, late, dyn_young);
    settle_loops(&dyn_keys, ran);
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

/// The loop iterations from index `k` up end (innermost first is not needed:
/// one probe decides them all at this point).
fn end_loops(k: usize) {
    let (keys, fp, young) = take_loops(k);
    if keys.is_empty() {
        return;
    }
    let ran = probe(&[], &keys, fp, false, young);
    settle_loops(&keys, ran);
}

/// LIFE I2: the loop iterations from index `k` up end. Drains them; their old
/// samples are unresolved at the dynamic boundary. Returns the keys, the
/// frame and how many young samples the caller's ONE collection must decide
/// (with any iteration/return probe at the same point: a second forced minor
/// would age the survivors the other probe decides).
fn take_loops(k: usize) -> (Vec<(u32, u32, u8)>, usize, usize) {
    let Some((keys, fp)) = table(|t| {
        let m = &mut t.measure;
        if k >= m.loops.len() {
            return None;
        }
        let fp = m.loops[k].fp;
        let keys: Vec<_> = m.loops.drain(k..).flat_map(|l| l.pend).collect();
        Some((keys, fp))
    })
    .flatten() else {
        return (Vec::new(), 0, 0);
    };
    let young = table(|t| {
        let m = &mut t.measure;
        let mut young = 0;
        for k in &keys {
            if !dyn_live(m, k) {
                continue;
            }
            if m.samples[k.0 as usize].old {
                let site = m.samples[k.0 as usize].site;
                m.sites.entry(site).or_default().d[(k.2 >> 1) as usize].unresolved += 1;
                m.samples[k.0 as usize].dyn_ &= !k.2;
            } else {
                m.samples[k.0 as usize].reached = false;
                young += 1;
            }
        }
        if young > 0 {
            m.dyn_probes += 1;
        }
        young
    })
    .unwrap_or(0);
    (keys, fp, young)
}

fn dyn_live(m: &Measure, &(i, g, bit): &(u32, u32, u8)) -> bool {
    m.samples
        .get(i as usize)
        .is_some_and(|s| s.gen == g && s.next != DONE && s.dyn_ & bit != 0)
}

/// After the collection (`ran`: `Some(true)` when it ran): a dynamic sample
/// still live was reached (one the minor did not reach is dead and was
/// decided by `died`).
fn settle_loops(keys: &[(u32, u32, u8)], ran: Option<bool>) {
    let _ = with(|t| {
        let m = &mut t.measure;
        for k in keys {
            if !dyn_live(m, k) {
                continue;
            }
            let s = m.samples[k.0 as usize];
            let bd = &mut m.sites.entry(s.site).or_default().d[(k.2 >> 1) as usize];
            if ran == Some(true) {
                bd.samples += 1;
                bd.alive += 1;
                bd.alive_w += s.w;
                bd.alive_bytes_w += s.w * s.bytes as f64;
                bd.holders[s.holder as usize] += 1;
            } else {
                bd.unresolved += 1;
            }
            m.samples[k.0 as usize].dyn_ &= !k.2;
        }
    });
}

/// Decides the pending samples `keys` at a boundary of the activation at
/// frame `fp`: old ones are unresolved (except at turn), young ones go
/// through one forced copied minor. After it, a sample still live was reached.
///
/// A sample the minor reached only through the remembered set or another
/// object is not decided by that reach (`life::minor_decides`): one
/// synchronous full at the same point, shared by all of them, decides it
/// (marked: alive; unmarked: dead; full not observed: unresolved).
fn probe(
    keys: &[(u32, u32)],
    dyn_keys: &[(u32, u32, u8)],
    fp: usize,
    late: bool,
    extra: usize,
) -> Option<bool> {
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
        if young + extra > 0 {
            m.in_probe = true;
            m.probe_fp = fp;
            m.probe_late = late;
            m.probes += 1;
        }
        young + extra
    })
    .unwrap_or(0);
    if young == 0 {
        return None;
    }
    let began = std::time::Instant::now();
    let before = crate::gc::diagnostic_hooks::life_collection_count();
    crate::gc::gc_collect_minor();
    let ran = crate::gc::diagnostic_hooks::life_collection_count() != before;
    let _ = with(|t| {
        let m = &mut t.measure;
        m.in_probe = false;
        m.probe_minors += 1;
        if !ran {
            for &k in keys {
                if m.live(k) && !m.samples[k.0 as usize].old {
                    m.unresolved_from(k.0, false);
                }
            }
        }
    });
    if ran {
        confirm(keys, dyn_keys, late);
    }
    let _ = with(|t| {
        let m = &mut t.measure;
        m.probe_ns += began.elapsed().as_nanos() as u64;
        budget(m);
    });
    Some(ran)
}

/// The confirming full of a probe (see `probe`).
fn confirm(keys: &[(u32, u32)], dyn_keys: &[(u32, u32, u8)], late: bool) {
    let n = with(|t| {
        let m = &mut t.measure;
        let mut weak: Vec<(u32, u32)> = Vec::new();
        let reached_weak = |m: &Measure, (i, g): (u32, u32)| {
            m.live((i, g)) && {
                let s = &m.samples[i as usize];
                s.reached && !super::life::minor_decides(s.holder)
            }
        };
        for &k in keys {
            if reached_weak(m, k) && !weak.contains(&k) {
                weak.push(k);
            }
        }
        for k in dyn_keys {
            let k2 = (k.0, k.1);
            if dyn_live(m, k) && reached_weak(m, k2) && !weak.contains(&k2) {
                weak.push(k2);
            }
        }
        let n = weak.len();
        if n > 0 {
            m.confirm = weak;
            m.confirm_fulls += 1;
            m.probe_late = late;
        }
        n
    })
    .unwrap_or(0);
    if n == 0 {
        return;
    }
    crate::gc::life_collect_full();
    let _ = with(|t| {
        let m = &mut t.measure;
        for k in std::mem::take(&mut m.confirm) {
            if m.live(k) {
                m.confirm_unres += 1;
                m.unresolved_from(k.0, false);
            }
        }
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
    let Some(s) = t.measure.samples.get_mut(i as usize) else {
        return;
    };
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
    super::life_regions::full_marked();
    let _ = with(|t| {
        t.full_pending = true;
        let m = &mut t.measure;
        // A probe's confirming full: its weakly reached samples, young or old.
        let late = m.probe_late;
        for (i, g) in std::mem::take(&mut m.confirm) {
            if !m.live((i, g)) {
                continue;
            }
            let addr = m.samples[i as usize].addr;
            if super::life::full_alive(addr) {
                m.confirm_alive += 1;
            } else {
                m.confirm_dead += 1;
                m.dead_from(i, late);
                // Its claim leaves the table (the index is reused).
                t.claims.remove(&addr);
            }
        }
        let turn_full = !m.full.is_empty();
        m.full_seen = true;
        m.full.clear();
        for i in 0..m.samples.len() as u32 {
            let s = m.samples[i as usize];
            if s.next == DONE || !s.old {
                continue;
            }
            let marked = super::life::full_alive(s.addr);
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
    if crate::arena::alloc_life::on() {
        crate::arena::alloc_life::birth(bits as u64);
    }
    if super::life_regions::on() {
        // LIFE I3: regions replace the per-birth probes.
        if keep() {
            super::life_regions::keep_birth(bits as u64);
        }
        return;
    }
    let bits = bits as u64;
    let sampled = table(|t| {
        let m = &mut t.measure;
        if m.births == 0 {
            // `PERRY_LIFE_MEASURE_SHIFT=k`: start every site at rate 2^-k more.
            m.global_shift = knob("PERRY_LIFE_MEASURE_SHIFT")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0)
                .min(16);
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
        let bytes =
            unsafe { (*((addr - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader)).size }
                as u64;
        s.bytes += bytes;
        let own = if s.sampled < 32 {
            0
        } else {
            64 - (s.sampled / 32).leading_zeros()
        };
        let shift = if rate_one() {
            0
        } else {
            (own + global).min(16)
        };
        if rand & ((1u64 << shift) - 1) != 0 {
            return None;
        }
        s.sampled += 1;
        Some((addr, bytes, (1u64 << shift) as f64))
    })
    .flatten();
    let Some((addr, bytes, w)) = sampled else {
        return;
    };
    // No probe here: the birth's value is not rooted across this call, so
    // the hook must not collect. Activations a throw left on top (frames
    // deeper than this one) are skipped; the next hook that may collect
    // pops them.
    let _ = table(|t| {
        let m = &mut t.measure;
        let live = m.acts.iter().rposition(|a| a.fp >= fp).map_or(0, |i| i + 1);
        let own = (live > 0 && m.acts[live - 1].fp == fp).then(|| live - 1);
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
            dyn_: 0,
        };
        // The dynamic boundaries: the innermost loop iteration in progress in
        // this frame or a caller's (deeper frames' entries are stale), and the
        // one around it.
        let mut found = 0u8;
        for l in m.loops.iter_mut().rev() {
            if l.fp < fp || found == 2 {
                continue;
            }
            l.pend.push((i, gen, 1 << found));
            found += 1;
        }
        m.samples[i as usize].dyn_ = (1u8 << found) - 1;
        m.advance(i, ITER);
        m.file(i, own);
        if m.samples[i as usize].old {
            let b = m.samples[i as usize].next;
            if b as usize != TURN {
                m.unresolved_from(i, true);
            }
        } else {
            t.claims.insert(
                addr,
                Claim {
                    site: 0,
                    len: 0,
                    state: State::Probe(i),
                },
            );
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
pub extern "C" fn perry_life_enter(body: i64) {
    let fp = caller_fp();
    if super::life_regions::on() {
        super::life_regions::enter(body as u64, fp);
    }
    // No probe here (the body's arguments are not rooted across this call):
    // activations a throw left deeper than this frame stay below the new one
    // and are popped, late, by the next hook of a shallower frame. An inlined
    // body enters in its caller's frame (same fp), nested in its activation.
    let _ = table(|t| {
        t.measure.exits.entry(body as u64).or_default()[0] += 1;
        t.measure.acts.push(Act {
            fp,
            body: body as u64,
            pend: Vec::new(),
        })
    });
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn perry_life_return() {
    let fp = caller_fp();
    if super::life_regions::on() {
        super::life_regions::ret(fp);
    }
    pop_deeper(fp);
    let top = table(|t| t.measure.acts.last().filter(|a| a.fp == fp).map(|a| a.body)).flatten();
    if let Some(body) = top {
        let _ = table(|t| t.measure.exits.entry(body).or_default()[1] += 1);
        pop_top(false);
    }
}

/// A `try` is entered: the activation depth its handler restores.
#[no_mangle]
#[inline(never)]
pub extern "C" fn perry_life_depth() -> i64 {
    table(|t| (t.measure.acts.len() as u64 | (t.measure.loops.len() as u64) << 32) as i64)
        .unwrap_or(0)
}

/// An iteration of the loop `id` begins (its body's statement list starts).
/// Never collects.
#[no_mangle]
#[inline(never)]
pub extern "C" fn perry_life_loop(id: i64) {
    let fp = caller_fp();
    if super::life_regions::on() {
        super::life_regions::loop_start(id as u64, fp);
    }
    let _ = table(|t| {
        let m = &mut t.measure;
        let act = m.acts.iter().rposition(|a| a.fp >= fp).map_or(0, |i| i + 1);
        m.loops.push(LoopIt {
            act,
            fp,
            id: id as u64,
            pend: Vec::new(),
        });
    });
}

/// The handler of a `try` entered at depth `depth` runs: every activation
/// deeper than it left on the throw without its return hook. Their
/// boundaries are decided now, `late` (an inlined body shares its caller's
/// frame, so the frame pointer alone cannot tell these apart).
#[no_mangle]
#[inline(never)]
pub extern "C" fn perry_life_catch(depth: i64) {
    let loops = (depth as u64 >> 32) as usize;
    let depth = (depth as u64 & 0xFFFF_FFFF) as usize;
    if super::life_regions::on() {
        super::life_regions::catch(depth, loops, caller_fp());
    }
    while table(|t| t.measure.acts.len() > depth) == Some(true) {
        let _ = table(|t| late(t, 2));
        pop_top(true);
    }
    // The loop iterations the throw left in the handler's own frame end here.
    end_loops(loops);
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn perry_life_iter(loop_id: i64) {
    let fp = caller_fp();
    if super::life_regions::on() {
        super::life_regions::iter(loop_id as u64, fp);
    }
    pop_deeper(fp);
    // The dynamic boundary: this loop's iteration in progress in this frame
    // ends (with any inner one a labelled jump or stale frame left above it).
    let k = table(|t| {
        let m = &t.measure;
        let mut k = None;
        for (j, l) in m.loops.iter().enumerate().rev() {
            if l.fp > fp {
                break;
            }
            if l.fp == fp && l.id == loop_id as u64 {
                k = Some(j);
                break;
            }
        }
        k
    })
    .flatten();
    let (dyn_keys, _, dyn_young) = match k {
        Some(k) => take_loops(k),
        None => (Vec::new(), 0, 0),
    };
    let keys = table(|t| {
        let m = &mut t.measure;
        let list = match m.acts.last_mut() {
            Some(a) if a.fp == fp => &mut a.pend,
            None => &mut m.root,
            Some(_) => return Vec::new(),
        };
        let mut hit = Vec::new();
        list.retain(|&(i, g)| {
            let Some(s) = m.samples.get(i as usize) else {
                return false;
            };
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
    // One collection decides this iteration's own-function samples and the
    // dynamic boundaries ending here.
    let ran = probe(&keys, &dyn_keys, fp, false, dyn_young);
    settle_loops(&dyn_keys, ran);
    if keys.is_empty() {
        return;
    }
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
    if super::life_regions::on() {
        super::life_regions::turn();
    }
    // The JS stack is unwound: every activation still listed returned.
    while table(|t| !t.measure.acts.is_empty()) == Some(true) {
        let _ = table(|t| late(t, 4));
        pop_top(true);
    }
    end_loops(0);
    // Iteration-pending samples with no activation: the turn ended their loop.
    let root = table(|t| std::mem::take(&mut t.measure.root)).unwrap_or_default();
    probe(&root, &[], 0, true, 0);
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
    probe(&turn, &[], 0, false, 0);
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
    let Some(path) = knob("PERRY_LIFE_MEASURE").filter(|p| p != "1") else {
        return;
    };
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
            let d = if b < 4 { &s.b[b] } else { &s.d[b - 4] };
            out.push_str(&format!(
                "{id}\t{}\t{}\t{}\t{}\t{name}\t{}\t{}\t{:.0}\t{:.0}\t{:.0}\t{:.0}\t{}\t{}",
                s.births,
                s.bytes,
                s.unborn,
                s.sampled,
                d.samples,
                d.alive,
                d.alive_w,
                d.dead_w,
                d.alive_bytes_w,
                d.dead_bytes_w,
                d.late,
                d.unresolved
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
        "#totals\tsite_births={births}\tsite_bytes={bytes}\tarena_bytes={arena}\tlarge_bytes={large}\tprobes={}\tprobe_minors={}\tturn_fulls={}\tlate_pops={}\tdyn_probes={}\tprobe_ns={}\telapsed_ns={elapsed}\tglobal_shift={}\tpending={}\tconfirm_fulls={}\tconfirm_alive={}\tconfirm_dead={}\tconfirm_unres={}\n",
        m.probes, m.probe_minors, m.turn_fulls, m.late_pops, m.dyn_probes, m.probe_ns, m.global_shift,
        m.samples.iter().filter(|s| s.next != DONE).count(),
        m.confirm_fulls, m.confirm_alive, m.confirm_dead, m.confirm_unres
    ));
    let mut bodies: Vec<_> = m.exits.iter().collect();
    bodies.sort_unstable_by_key(|(id, _)| **id);
    for (id, e) in bodies {
        out.push_str(&format!(
            "#body\t{id}\t{}\t{}\t{}\t{}\t{}\n",
            e[0], e[1], e[2], e[3], e[4]
        ));
    }
    let _ = std::fs::write(path, out);
}
