//! LIFE I3: the region ceiling (`PERRY_LIFE_REGIONS=<path>` at run time; the
//! instrumented runtime only, in a binary compiled with `PERRY_LIFE_MEASURE`).
//!
//! A region instance is one iteration of a loop (`perry_life_loop` to the
//! iteration's end), one activation of a body (`perry_life_enter` to its
//! return or late pop) or one turn (the previous turn's end to
//! `perry_life_turn`). It ends CLEAN when no object allocated inside it, by
//! compiled code or by the runtime, is still reachable at its end.
//!
//! Inside set, ONE predicate: the allocation census's fresh-header event
//! (`arena::alloc_life`, every path) numbers each object; a region hook first
//! flushes the census (the pending inline burst, the inline slow call) and then
//! reads the number, so an instance's inside set is the objects numbered
//! between its start and its end. Every numbered object is tracked by address
//! (moved by the copied minor's reach, dropped when a minor does not reach it,
//! moved by the old-page evacuation, dropped by a full's sweep).
//!
//! Decision at the end, for a sampled instance: one forced copied minor at that
//! point (shared by every instance ending there); a tracked inside object still
//! present was reached (survivor; holder = the walk phase of the reach) when the
//! walk reached it from a root (`life::minor_decides`). An inside object reached
//! only through the remembered set or another object (a dead, uncollected old
//! object may hold it), and an old inside object the minor did not reach
//! (promoted by a collection inside the instance, or born old), are decided by
//! one synchronous full at the same point (its mark); with fulls off
//! (`PERRY_LIFE_REGIONS_FULL=0`), or a full that is not observed, the instance
//! is `unresolved`, as with any collection the hooks did not observe. Never guessed.
//!
//! Sampling per region id: the first 32 instances, then the rate halves each
//! time the id's sampled count doubles (floor 2^-16; `PERRY_LIFE_REGIONS_RATE=1`:
//! every instance), plus a global shift from the time budget (`PERRY_LIFE_REGIONS_SHIFT=<n>`: fixed at
//! n). Turns always.
//! Byte samples are the census's own draws (an object of `b` bytes carries
//! `b / I` samples plus one with probability `(b mod I) / I`): every instance
//! open at a sampled object's birth is decided, and the sample records the
//! verdict of each (innermost first), so the byte shares are computed offline.
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU8, Ordering};

const LOOP: u8 = 0;
const BODY: u8 = 1;
const TURN: u8 = 2;
const KINDS: [&str; 3] = ["loop", "body", "turn"];
const HOLDERS: [&str; 8] = ["frame", "outer_frame", "shadow", "global", "remembered", "object", "other", "old"];
const H_OLD: u8 = 7;

#[derive(Clone, Copy)]
struct Ent {
    seq: u64,
    bytes: u32,
    /// The minor that last reached it (0: none).
    reached: u32,
    /// The full whose marks found it alive (0: none).
    full_alive: u32,
    holder: u8,
}

struct Inst {
    kind: u8,
    id: u64,
    fp: usize,
    seq0: u64,
    cnt0: u64,
    bytes0: u64,
    /// Weight when the id's sampler took it (0: not id-sampled).
    w: f64,
    /// Decided at its end (id-sampled, a turn, or holding a byte sample).
    decide: bool,
    unresolved: bool,
    /// When it opened (the confirmation budget's inside time).
    t0: std::time::Instant,
    /// Byte samples inside (born while innermost, or handed up by children).
    bs: Vec<u32>,
}

#[derive(Default, Clone)]
struct Stat {
    instances: u64,
    bytes: u64,
    taken: u64,
    sampled: u64,
    n: [u64; 4],
    w: [f64; 4],
    bytes_w: [f64; 4],
    surv_inst: [u64; 4],
    surv_bytes: [u64; 4],
    surv_objs: u64,
    holders: [u64; 8],
    /// Iterations ended by the next iteration's start (`loop_start`), not by
    /// a back-edge `perry_life_iter`: the loop took a lowering path without
    /// the back-edge hook. Reported so such paths are visible.
    restarted: u64,
    /// Inside objects present at a decided end that are shape key/attribute
    /// storage (`life::engine_metadata`): not survivors, counted here.
    meta_objs: u64,
    /// Confirming fulls charged to this id, their time, and the time spent
    /// inside its decided instances (the confirmation budget).
    confirm_fulls: u64,
    confirm_ns: u64,
    inside_ns: u64,
}

/// Verdicts (index into `Stat::n`).
const EMPTY: usize = 0;
const CLEAN: usize = 1;
const DIRTY: usize = 2;
const UNRES: usize = 3;
const VERDICT: [char; 4] = ['e', 'c', 'd', 'u'];

struct ByteSample {
    n: u32,
    chain: Vec<(u8, u64, u8)>,
}

#[derive(Default)]
struct Reg {
    started: bool,
    stack: Vec<Inst>,
    seq: u64,
    cnt: u64,
    bytes: u64,
    before_bytes: u64,
    young: HashMap<usize, Ent>,
    from: HashMap<usize, Ent>,
    old: BTreeMap<u64, (usize, Ent)>,
    old_addr: HashMap<usize, u64>,
    minor_id: u32,
    full_id: u32,
    synced: u64,
    full_pending: bool,
    in_minor: bool,
    in_probe: bool,
    probe_fp: usize,
    last_born: Option<(usize, Option<u32>)>,
    stats: HashMap<(u8, u64), Stat>,
    bs: Vec<ByteSample>,
    orphan_bs: u64,
    rng: u64,
    global_shift: u32,
    decisions: u64,
    minors: u64,
    fulls: u64,
    fulls_unseen: u64,
    /// Confirming fulls run, and decided instances left `unresolved` because
    /// their id was over its confirmation budget (or fulls are off).
    confirm_fulls: u64,
    confirm_unres: u64,
    confirm_ns: u64,
    unobserved: u64,
    born_in_minor: u64,
    probe_ns: u64,
    start: Option<std::time::Instant>,
    turns: u64,
    turn_bytes: u64,
    turn_end_bytes: u64,
    keep_roots: Vec<Box<u64>>,
}

crate::perry_thread_local! {
    static REG: RefCell<Reg> = RefCell::new(Reg::default());
}

/// 0: unread, 1: off, 2: on.
static ON: AtomicU8 = AtomicU8::new(0);
static BUSY: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn knob(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// `PERRY_LIFE_REGIONS` is set (read once).
pub(crate) fn on() -> bool {
    match ON.load(Ordering::Relaxed) {
        0 => {
            let v = knob("PERRY_LIFE_REGIONS").is_some();
            ON.store(if v { 2 } else { 1 }, Ordering::Relaxed);
            v
        }
        v => v == 2,
    }
}

/// `PERRY_LIFE_REGIONS_SHIFT=<n>` set: the global shift is fixed at `n` (the time budget does not
/// raise it), so a sparse run's density does not depend on the machine's speed.
fn shift_fixed() -> bool {
    static F: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *F.get_or_init(|| knob("PERRY_LIFE_REGIONS_SHIFT").is_some())
}

fn rate_one() -> bool {
    static R: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *R.get_or_init(|| knob("PERRY_LIFE_REGIONS_RATE").as_deref() == Some("1"))
}

fn fulls_allowed() -> bool {
    static R: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *R.get_or_init(|| knob("PERRY_LIFE_REGIONS_FULL").as_deref() != Some("0"))
}

/// `PERRY_LIFE_REGIONS_WHY=<path>`: every decided instance gets the
/// synchronous full, and the first `PERRY_LIFE_REGIONS_WHY_N` (default 4)
/// dirty instances of each region id write each survivor's holder chain
/// (`gc::life_why`) to the path.
fn why_path() -> Option<&'static str> {
    static R: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    R.get_or_init(|| knob("PERRY_LIFE_REGIONS_WHY")).as_deref()
}

fn why_n() -> u64 {
    static R: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *R.get_or_init(|| knob("PERRY_LIFE_REGIONS_WHY_N").and_then(|v| v.parse().ok()).unwrap_or(4))
}

/// The holder chain of each survivor of a dirty instance, back to its root.
fn why_dump(r: &Reg, inst: &Inst, seq1: u64, n: u64, survivors: &[(usize, Ent)]) {
    let Some(path) = why_path() else { return };
    let inside = |user: usize| {
        let seq = r.young.get(&user).map(|e| e.seq).or_else(|| r.old_addr.get(&user).copied());
        match seq {
            Some(q) if q > inst.seq0 && q <= seq1 => "inside",
            Some(_) => "tracked-outside",
            None => "untracked",
        }
    };
    let mut out = format!("== {}:{} instance {n} survivors {}\n", KINDS[inst.kind as usize], inst.id, survivors.len());
    for (addr, e) in survivors {
        out.push_str(&format!("  obj seq={} holder={} {}\n", e.seq, HOLDERS[e.holder as usize], crate::gc::life_why::label(addr - crate::gc::GC_HEADER_SIZE)));
        let mut h = addr - crate::gc::GC_HEADER_SIZE;
        for _ in 0..64 {
            match crate::gc::life_why::get(h) {
                None => {
                    out.push_str("    <- not marked by this full\n");
                    break;
                }
                Some(w) if w.parent == 0 => {
                    out.push_str(&format!("    <- ROOT {} fn={} slot={:#x}\n", w.phase, crate::gc::life_why::symbol(w.func), w.slot));
                    break;
                }
                Some(w) => {
                    let pu = w.parent + crate::gc::GC_HEADER_SIZE;
                    out.push_str(&format!("    <- {} {}\n", inside(pu), crate::gc::life_why::label(w.parent)));
                    h = w.parent;
                }
            }
        }
    }
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(out.as_bytes());
    }
}

fn with<R>(f: impl FnOnce(&mut Reg) -> R) -> Option<R> {
    REG.with(|r| r.try_borrow_mut().ok().map(|mut r| f(&mut r)))
}

fn count() -> u64 {
    crate::gc::diagnostic_hooks::life_collection_count()
}

impl Reg {
    fn next_rand(&mut self) -> u64 {
        if self.rng == 0 {
            self.rng = 0x2545_F491_4F6C_DD1D;
        }
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        x.wrapping_mul(0x9E37_79B9_7F4A_7C15)
    }

    fn open(&mut self, kind: u8, id: u64, fp: usize) {
        let w = if kind == TURN {
            1.0
        } else {
            let rand = self.next_rand();
            let global = self.global_shift;
            let s = self.stats.entry((kind, id)).or_default();
            s.taken += 1;
            let own = if s.sampled < 32 { 0 } else { 64 - (s.sampled / 32).leading_zeros() };
            let shift = if rate_one() { 0 } else { (own + global).min(16) };
            if rand & ((1u64 << shift) - 1) == 0 {
                s.sampled += 1;
                (1u64 << shift) as f64
            } else {
                0.0
            }
        };
        let inst = Inst {
            kind,
            id,
            fp,
            seq0: self.seq,
            cnt0: self.cnt,
            bytes0: self.bytes,
            w,
            decide: w > 0.0,
            unresolved: false,
            t0: std::time::Instant::now(),
            bs: Vec::new(),
        };
        self.stack.push(inst);
    }

    /// A collection the hooks did not observe ran: tracked objects may be
    /// freed (addresses reused), so every open instance is unresolved.
    fn sync(&mut self) {
        let now = count();
        if now == self.synced {
            return;
        }
        if !(self.full_pending && now == self.synced + 1) {
            self.unobserved += 1;
            for i in &mut self.stack {
                i.unresolved = true;
            }
            self.young.clear();
            self.old.clear();
            self.old_addr.clear();
        }
        self.full_pending = false;
        self.synced = now;
    }

    fn untrack_old(&mut self, addr: usize) -> Option<Ent> {
        let seq = self.old_addr.remove(&addr)?;
        self.old.remove(&seq).map(|(_, e)| e)
    }

    fn track(&mut self, addr: usize, e: Ent) {
        if crate::arena::pointer_in_nursery(addr) {
            self.young.insert(addr, e);
        } else {
            self.untrack_old(addr);
            self.old.insert(e.seq, (addr, e));
            self.old_addr.insert(addr, e.seq);
        }
    }
}

/// The first hook: the first turn starts here.
fn ensure_started() {
    super::life::ensure();
    let _ = with(|r| {
        if !r.started {
            r.started = true;
            r.synced = count();
            r.start = Some(std::time::Instant::now());
            r.global_shift = knob("PERRY_LIFE_REGIONS_SHIFT").and_then(|v| v.parse().ok()).unwrap_or(0).min(16);
            r.open(TURN, 0, 0);
        }
    });
}

/// The census numbered a fresh object at user address `addr` (`n`: its byte
/// samples).
pub(crate) fn born(addr: usize, bytes: u32, n: u32) {
    let done = with(|r| {
        if !r.started {
            r.before_bytes += bytes as u64;
            return;
        }
        if r.in_minor {
            r.born_in_minor += 1;
            return;
        }
        r.seq += 1;
        r.cnt += 1;
        r.bytes += bytes as u64;
        let e = Ent { seq: r.seq, bytes, reached: 0, full_alive: 0, holder: 6 };
        r.track(addr, e);
        let mut sample = None;
        if n > 0 {
            let i = r.bs.len() as u32;
            r.bs.push(ByteSample { n, chain: Vec::new() });
            for inst in &mut r.stack {
                inst.decide = true;
            }
            match r.stack.last_mut() {
                Some(top) => top.bs.push(i),
                None => r.orphan_bs += n as u64,
            }
            sample = Some(i);
        }
        r.last_born = Some((addr, sample));
    });
    if done.is_none() {
        // The state was held (a region hook never allocates: counted, checked 0).
        BUSY.fetch_add(1, Ordering::Relaxed);
    }
}

/// The old-page evacuation moved the object at `old` to `new` (whose fresh
/// header the census just numbered as a birth: taken back).
pub(crate) fn evacuated(old: usize, new: usize, total: u32) {
    let _ = with(|r| {
        if !r.started {
            return;
        }
        if let Some((addr, sample)) = r.last_born.take() {
            if addr == new {
                r.cnt -= 1;
                r.bytes -= total as u64;
                r.untrack_old(new);
                if let Some(i) = sample {
                    if let Some(top) = r.stack.last_mut() {
                        top.bs.retain(|&b| b != i);
                    }
                    r.bs[i as usize].n = 0;
                }
            }
        }
        // A tenured nursery object (or an old one) moves to the old arena.
        if let Some(e) = r.young.remove(&old).or_else(|| r.untrack_old(old)) {
            r.track(new, e);
        }
    });
}

pub(super) fn before_minor() {
    let _ = with(|r| {
        r.sync();
        r.in_minor = true;
        r.minor_id += 1;
        r.from = std::mem::take(&mut r.young);
    });
}

pub(super) fn reached(old: usize, new: usize) {
    let hit = with(|r| {
        if let Some(e) = r.from.remove(&old) {
            Some((e, r.in_probe, r.probe_fp))
        } else if let Some(&seq) = r.old_addr.get(&old) {
            // A young-large or in-place promoted object (`old == new`).
            r.old.get(&seq).map(|(_, e)| (*e, r.in_probe, r.probe_fp))
        } else {
            None
        }
    })
    .flatten();
    let Some((mut e, in_probe, fp)) = hit else { return };
    e.holder = if in_probe { holder(fp) } else { 6 };
    let _ = with(|r| {
        e.reached = r.minor_id;
        if old != new {
            r.untrack_old(old);
        }
        r.track(new, e);
    });
}

fn holder(fp: usize) -> u8 {
    let (phase, frame) = crate::gc::diagnostic_hooks::life_holder();
    let phase = phase.unwrap_or("");
    if phase.contains("native") {
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
    }
}

pub(super) fn after_minor() {
    let _ = with(|r| {
        // Every young object the minor did not reach is freed.
        r.from.clear();
        r.in_minor = false;
        r.synced = count();
    });
}

/// A synchronous full has complete marks: an unmarked old object is freed,
/// a marked one is alive (its full id).
pub(super) fn full_marked() {
    let _ = with(|r| {
        if !r.started {
            return;
        }
        r.full_id += 1;
        r.full_pending = true;
        let id = r.full_id;
        // A young inside object a probe minor reached through the remembered
        // set or another object waits for this mark (`life::minor_decides`).
        r.young.retain(|addr, e| {
            let alive = super::life::full_alive(*addr);
            if alive {
                e.full_alive = id;
            }
            alive
        });
        let mut dead = Vec::new();
        for (seq, (addr, e)) in r.old.iter_mut() {
            if super::life::full_alive(*addr) {
                e.full_alive = id;
            } else {
                dead.push((*seq, *addr));
            }
        }
        for (seq, addr) in dead {
            r.old.remove(&seq);
            r.old_addr.remove(&addr);
        }
    });
}

/// The time budget: region probes may not take more than 4x the mutator time;
/// over it, every id's rate halves.
fn budget(r: &mut Reg) {
    if rate_one() || shift_fixed() || r.decisions % 64 != 0 {
        return;
    }
    let Some(start) = r.start else { return };
    let total = start.elapsed().as_nanos() as u64;
    if total > 50_000_000 && r.probe_ns > 4 * total.saturating_sub(r.probe_ns) && r.global_shift < 16 {
        r.global_shift += 1;
    }
}

/// Ends the instances `stack[k..]` (inner first) at the frame `fp`.
fn end_from(k: usize, fp: usize) {
    crate::arena::alloc_life::sync_point();
    let Some((insts, need)) = with(|r| {
        r.sync();
        if k >= r.stack.len() {
            return None;
        }
        let insts = r.stack.split_off(k);
        let need = insts.iter().any(|i| i.decide && !i.unresolved && r.cnt > i.cnt0);
        Some((insts, need))
    })
    .flatten() else {
        return;
    };
    let began = std::time::Instant::now();
    let mut probe_minor = 0;
    if need {
        let _ = with(|r| {
            r.in_probe = true;
            r.probe_fp = fp;
        });
        let before = count();
        crate::gc::gc_collect_minor();
        let ran = count() != before;
        probe_minor = with(|r| {
            r.in_probe = false;
            r.minors += 1;
            r.sync();
            if ran {
                r.minor_id
            } else {
                0
            }
        })
        .unwrap_or(0);
    }
    // Old inside objects the minor did not reach: a synchronous full decides
    // them. Inside objects it reached only through the remembered set or
    // another object (a dead old object may hold them: `life::minor_decides`)
    // are decided by that full too; one full is shared by every instance
    // decided here. Confirmation budget: an id's first 32 confirming fulls;
    // then while its fulls' time is at most 4x the time inside its decided
    // instances, or all confirming fulls' time at most 2 s plus 4x the
    // mutator time. An instance over it with no full here is `unresolved`.
    let (need_old, confirm_ids) = if need {
        with(|r| {
            let now = std::time::Instant::now();
            let mutator = r.start.map_or(0, |t| (now.duration_since(t).as_nanos() as u64).saturating_sub(r.probe_ns));
            let global_ok = r.confirm_ns <= 2_000_000_000 + 4 * mutator;
            let mut need_old = false;
            let mut ids = Vec::new();
            for i in insts.iter().filter(|i| i.decide && !i.unresolved && r.seq > i.seq0) {
                let range = i.seq0 + 1..=r.seq;
                need_old |= r.old.range(range.clone()).any(|(_, (a, e))| e.reached != probe_minor && !super::life::engine_metadata(*a));
                let weak = r.old.range(range).any(|(_, (a, e))| {
                    e.reached == probe_minor && !super::life::minor_decides(e.holder) && !super::life::engine_metadata(*a)
                }) || r.young.iter().any(|(a, e)| {
                    e.seq > i.seq0 && e.seq <= r.seq && !super::life::minor_decides(e.holder) && !super::life::engine_metadata(*a)
                });
                if weak {
                    let inside = now.duration_since(i.t0).as_nanos() as u64;
                    let ok = global_ok
                        || r.stats.get(&(i.kind, i.id)).map_or(true, |s| {
                            s.confirm_fulls < 32 || s.confirm_ns <= 4 * (s.inside_ns + inside)
                        });
                    if ok {
                        ids.push((i.kind, i.id));
                    }
                }
            }
            (need_old, ids)
        })
        .unwrap_or((false, Vec::new()))
    } else {
        (false, Vec::new())
    };
    let need_full = need && (why_path().is_some() || need_old || !confirm_ids.is_empty());
    let mut full = 0;
    if need_full && (fulls_allowed() || insts.iter().any(|i| i.kind == TURN)) {
        let id = with(|r| r.full_id).unwrap_or(0);
        let f0 = std::time::Instant::now();
        crate::gc::life_why::arm(why_path().is_some());
        crate::gc::life_collect_full();
        crate::gc::life_why::arm(false);
        let full_ns = f0.elapsed().as_nanos() as u64;
        full = with(|r| {
            r.fulls += 1;
            if !confirm_ids.is_empty() {
                r.confirm_fulls += 1;
                r.confirm_ns += full_ns;
            }
            for k in &confirm_ids {
                let s = r.stats.entry(*k).or_default();
                s.confirm_fulls += 1;
                s.confirm_ns += full_ns;
            }
            r.sync();
            if r.full_id != id {
                r.full_id
            } else {
                r.fulls_unseen += 1;
                0
            }
        })
        .unwrap_or(0);
    }
    let _ = with(|r| {
        r.probe_ns += began.elapsed().as_nanos() as u64;
        let seq1 = r.seq;
        let (cnt1, bytes1) = (r.cnt, r.bytes);
        let mut carry: Vec<u32> = Vec::new();
        for mut inst in insts.into_iter().rev() {
            inst.bs.append(&mut carry);
            let inside = if seq1 > inst.seq0 { cnt1.saturating_sub(inst.cnt0) } else { 0 };
            let bytes = bytes1.saturating_sub(inst.bytes0);
            let mut verdict = if inside == 0 { EMPTY } else { CLEAN };
            let (mut objs, mut sbytes, mut hold, mut meta) = (0u64, 0u64, [0u64; 8], 0u64);
            if inside > 0 && inst.decide {
                if inst.unresolved || probe_minor == 0 {
                    verdict = UNRES;
                } else {
                    for (a, e) in r.young.iter().filter(|(_, e)| e.seq > inst.seq0 && e.seq <= seq1) {
                        if super::life::engine_metadata(*a) {
                            meta += 1;
                            continue;
                        }
                        if !super::life::minor_decides(e.holder) {
                            if full == 0 {
                                verdict = UNRES;
                                r.confirm_unres += 1;
                                break;
                            }
                            if e.full_alive != full {
                                // Not marked: dead (the full's `full_marked` dropped it).
                                continue;
                            }
                        }
                        objs += 1;
                        sbytes += e.bytes as u64;
                        hold[e.holder as usize] += 1;
                    }
                    for (_, (a, e)) in r.old.range(inst.seq0 + 1..=seq1) {
                        if verdict == UNRES {
                            break;
                        }
                        if super::life::engine_metadata(*a) {
                            meta += 1;
                            continue;
                        }
                        if e.reached == probe_minor && super::life::minor_decides(e.holder) {
                            hold[e.holder as usize] += 1;
                        } else if e.reached == probe_minor && full != 0 && e.full_alive == full {
                            // Reached through the remembered set or an object, confirmed by the full.
                            hold[e.holder as usize] += 1;
                        } else if full != 0 && e.full_alive == full {
                            hold[H_OLD as usize] += 1;
                        } else {
                            if e.reached == probe_minor && full == 0 {
                                r.confirm_unres += 1;
                            }
                            verdict = UNRES;
                            break;
                        }
                        objs += 1;
                        sbytes += e.bytes as u64;
                    }
                    if verdict != UNRES && objs > 0 {
                        verdict = DIRTY;
                        let done = r.stats.get(&(inst.kind, inst.id)).map_or(0, |s| s.surv_inst.iter().sum::<u64>());
                        if why_path().is_some() && done < why_n() {
                            let mut sv: Vec<(usize, Ent)> =
                                r.young.iter().filter(|(_, e)| e.seq > inst.seq0 && e.seq <= seq1).map(|(a, e)| (*a, *e)).collect();
                            sv.extend(r.old.range(inst.seq0 + 1..=seq1).map(|(_, (a, e))| (*a, *e)));
                            let n = r.stats.get(&(inst.kind, inst.id)).map_or(0, |s| s.instances);
                            why_dump(r, &inst, seq1, n, &sv);
                        }
                    }
                }
            }
            if inst.decide {
                r.decisions += 1;
                for &b in &inst.bs {
                    r.bs[b as usize].chain.push((inst.kind, inst.id, verdict as u8));
                }
            }
            let s = r.stats.entry((inst.kind, inst.id)).or_default();
            if inst.decide {
                s.inside_ns += began.duration_since(inst.t0).as_nanos() as u64;
            }
            s.instances += 1;
            s.bytes += bytes;
            if inst.w > 0.0 {
                s.n[verdict] += 1;
                s.w[verdict] += inst.w;
                s.bytes_w[verdict] += inst.w * bytes as f64;
                s.meta_objs += meta;
                if verdict == DIRTY {
                    let b = match objs {
                        1 => 0,
                        2..=4 => 1,
                        5..=16 => 2,
                        _ => 3,
                    };
                    s.surv_inst[b] += 1;
                    s.surv_bytes[b] += sbytes;
                    s.surv_objs += objs;
                    for (h, n) in hold.iter().enumerate() {
                        s.holders[h] += n;
                    }
                }
            }
            if inst.kind == TURN {
                r.turns += 1;
                r.turn_bytes += bytes;
                r.turn_end_bytes = bytes1;
            }
            carry = std::mem::take(&mut inst.bs);
        }
        match r.stack.last_mut() {
            Some(top) => top.bs.append(&mut carry),
            None => {
                for b in carry {
                    r.orphan_bs += r.bs[b as usize].n as u64;
                }
            }
        }
        budget(r);
    });
}

/// Index of the instance to end for a hook at frame `fp`: the topmost one
/// `want` accepts in this frame (entries of deeper frames above it are stale:
/// a throw left them), else the first of the stale run at the top.
fn find(fp: usize, want: impl Fn(&Inst) -> bool) -> Option<usize> {
    with(|r| {
        let mut stale = None;
        for (j, i) in r.stack.iter().enumerate().skip(1).rev() {
            if i.fp < fp {
                if stale.is_none() || stale == Some(j + 1) {
                    stale = Some(j);
                }
                continue;
            }
            if i.fp > fp {
                break;
            }
            if want(i) {
                return Some(j);
            }
        }
        stale
    })
    .flatten()
}

pub(super) fn enter(body: u64, fp: usize) {
    ensure_started();
    crate::arena::alloc_life::sync_point();
    let _ = with(|r| r.open(BODY, body, fp));
}

pub(super) fn loop_start(id: u64, fp: usize) {
    ensure_started();
    crate::arena::alloc_life::sync_point();
    // In one frame, loop `id`'s body begins again only after its previous
    // iteration ended: an open instance of it here ends now (a lowering path
    // that emits no back-edge hook would otherwise nest every iteration in
    // the one before).
    if let Some(k) = find(fp, |i| i.kind == LOOP && i.id == id) {
        let _ = with(|r| r.stats.entry((LOOP, id)).or_default().restarted += 1);
        end_from(k, fp);
    }
    let _ = with(|r| r.open(LOOP, id, fp));
}

pub(super) fn ret(fp: usize) {
    ensure_started();
    if let Some(k) = find(fp, |i| i.kind == BODY) {
        end_from(k, fp);
    }
}

pub(super) fn iter(id: u64, fp: usize) {
    ensure_started();
    if let Some(k) = find(fp, |i| i.kind == LOOP && i.id == id) {
        end_from(k, fp);
    }
}

/// The handler of a `try` entered with `acts` activations and `loops` loop
/// iterations open: everything opened after it ends here.
pub(super) fn catch(acts: usize, loops: usize, fp: usize) {
    ensure_started();
    end_from(1 + acts + loops, fp);
}

pub(super) fn turn() {
    ensure_started();
    end_from(0, 0);
    let _ = with(|r| {
        // No open instance holds an object born before this point.
        r.young.clear();
        r.old.clear();
        r.old_addr.clear();
        r.open(TURN, 0, 0);
        write(r, false);
    });
}

/// `PERRY_FACTS_SABOTAGE=lifekeep` with regions on: every census birth is
/// also stored in a global root.
pub(super) fn keep_birth(bits: u64) {
    if bits & super::TAG_MASK != super::POINTER_TAG {
        return;
    }
    let slot = Box::new(bits);
    crate::gc::js_gc_register_global_root(&*slot as *const u64 as i64);
    let _ = with(|r| r.keep_roots.push(slot));
}

/// Process exit (the census's exit write): the totals with the tail.
pub(crate) fn exit() {
    if !on() {
        return;
    }
    let _ = with(|r| write(r, true));
}

fn write(r: &mut Reg, exit: bool) {
    let Some(path) = knob("PERRY_LIFE_REGIONS").filter(|p| p != "1") else { return };
    let mut out = String::from(
        "kind\tid\tinstances\tbytes\ttaken\tsampled\tempty\tclean\tdirty\tunresolved\tw_empty\tw_clean\tw_dirty\tw_unresolved\tbytes_w_empty\tbytes_w_clean\tbytes_w_dirty\tbytes_w_unresolved\tsurv1\tsurv2_4\tsurv5_16\tsurv17\tsurv_bytes1\tsurv_bytes2_4\tsurv_bytes5_16\tsurv_bytes17\tsurv_objs\trestarted\tmeta_objs",
    );
    for h in HOLDERS {
        out.push_str(&format!("\th_{h}"));
    }
    out.push('\n');
    let mut ids: Vec<_> = r.stats.keys().copied().collect();
    ids.sort_unstable();
    for key in ids {
        let s = &r.stats[&key];
        out.push_str(&format!("{}\t{}\t{}\t{}\t{}\t{}", KINDS[key.0 as usize], key.1, s.instances, s.bytes, s.taken, s.sampled));
        for v in s.n {
            out.push_str(&format!("\t{v}"));
        }
        for v in s.w.iter().chain(&s.bytes_w) {
            out.push_str(&format!("\t{v:.0}"));
        }
        for v in s.surv_inst.iter().chain(&s.surv_bytes) {
            out.push_str(&format!("\t{v}"));
        }
        out.push_str(&format!("\t{}\t{}\t{}", s.surv_objs, s.restarted, s.meta_objs));
        for v in s.holders {
            out.push_str(&format!("\t{v}"));
        }
        out.push('\n');
    }
    for b in &r.bs {
        if b.n == 0 {
            continue;
        }
        out.push_str(&format!("#bs\t{}\t", b.n));
        for (j, (k, id, v)) in b.chain.iter().enumerate() {
            if j > 0 {
                out.push(',');
            }
            out.push_str(&format!("{}:{id}:{}", KINDS[*k as usize], VERDICT[*v as usize]));
        }
        out.push('\n');
    }
    let elapsed = r.start.map_or(0, |s| s.elapsed().as_nanos() as u64);
    out.push_str(&format!(
        "#totals\tinterval={}\tborn_objects={}\tborn_bytes={}\tbefore_bytes={}\tturns={}\tturn_bytes={}\ttail_bytes={}\texit={}\tdecisions={}\tminors={}\tfulls={}\tfulls_unseen={}\tconfirm_fulls={}\tconfirm_unres={}\tunobserved={}\tborn_in_minor={}\tbusy={}\torphan_bs={}\tglobal_shift={}\tprobe_ns={}\telapsed_ns={elapsed}\n",
        crate::arena::alloc_life::interval(),
        r.cnt,
        r.bytes,
        r.before_bytes,
        r.turns,
        r.turn_bytes,
        r.bytes - r.turn_end_bytes,
        exit as u8,
        r.decisions,
        r.minors,
        r.fulls,
        r.fulls_unseen,
        r.confirm_fulls,
        r.confirm_unres,
        r.unobserved,
        r.born_in_minor,
        BUSY.load(Ordering::Relaxed),
        r.orphan_bs,
        r.global_shift,
        r.probe_ns,
    ));
    let _ = std::fs::write(path, out);
}
