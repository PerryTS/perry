//! Check mode for lifetime claims (LIFE G1, `PERRY_VERIFY_FACTS=1`).
//!
//! The compiler's census claims that no pointer to a birth exists after its
//! death point: a `frame` local at the end of its scope or at a return, an
//! argument birth after a call whose callee drops it. Each death puts the
//! object's address into a claim table (this build only). The collector
//! reaching a claimed address is the violation: the copied minor's copy point
//! and in-place promotion (`gc/copying.rs`, diagnostic specialization only)
//! call [`reached`], which writes one JSON line naming the holder (walk phase,
//! owning frame) and exits 86. A claim resting on a bet writes the same line
//! with marker `PERRY_BET_FAILED` once per site and the program continues.
//!
//! The exact check at death: the first death of each claim site and every
//! Nth after it force a copied minor (N from the compile,
//! `PERRY_VERIFY_FACTS_LIFE_EVERY`). An argument birth is registered before
//! the call (`perry_verify_facts_life_arg`) and moves with the object while
//! the call holds it; the call's normal return declares it dead.
//!
//! What is not checked (missed, never wrong): an object already outside the
//! nursery at its death (`non_young`), claims alive across a collection that
//! is not a copied minor (a full, a non-moving minor: the table is dropped,
//! `dropped_unobserved`), deaths on a throw edge (no death call runs).
use std::cell::RefCell;
use std::collections::HashMap;

use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum State {
    /// An argument birth the call still holds; its token.
    Pending(u64),
    Dead,
    /// A sampled birth of the lifetime measurement (`life_measure`): its
    /// sample index.
    Probe(u32),
}

#[derive(Clone, Copy)]
pub(super) struct Claim {
    pub(super) site: u64,
    pub(super) len: usize,
    pub(super) state: State,
}

#[derive(Default)]
pub(super) struct Table {
    /// The lifetime measurement's state (`life_measure`).
    pub(super) measure: super::life_measure::Measure,
    /// User address -> claim.
    pub(super) claims: HashMap<usize, Claim>,
    /// During a copied minor: the claims of from-space, not yet reached.
    from: HashMap<usize, Claim>,
    /// Token -> user address of a pending argument birth.
    tokens: HashMap<u64, usize>,
    next_token: u64,
    /// Deaths per site, for the sampled exact check.
    per_site: HashMap<u64, u64>,
    /// Bet sites already reported.
    bets_reported: std::collections::HashSet<u64>,
    /// `collection_count` the table is in sync with.
    synced: u64,
    /// A synchronous full ran its marks through `life_measure::full_marked`
    /// (an observed collection: it neither moves nor reuses a young claim's object).
    pub(super) full_pending: bool,
}

/// Run totals, printed at exit (`PERRY_VERIFY_FACTS_LIFE_REPORT`): plain
/// statics, because the exit handler runs after thread-locals are gone.
mod totals {
    use std::sync::atomic::AtomicU64;
    pub(super) static DEATHS: AtomicU64 = AtomicU64::new(0);
    pub(super) static UNBORN: AtomicU64 = AtomicU64::new(0);
    pub(super) static NON_YOUNG: AtomicU64 = AtomicU64::new(0);
    pub(super) static EXACT: AtomicU64 = AtomicU64::new(0);
    pub(super) static CHECKED_MINORS: AtomicU64 = AtomicU64::new(0);
    pub(super) static DROPPED_UNOBSERVED: AtomicU64 = AtomicU64::new(0);
    pub(super) static DIED_IN_CALL: AtomicU64 = AtomicU64::new(0);
    pub(super) static BETS_FAILED: AtomicU64 = AtomicU64::new(0);
}

fn bump(c: &std::sync::atomic::AtomicU64, n: u64) {
    c.fetch_add(n, std::sync::atomic::Ordering::Relaxed);
}

fn get(c: &std::sync::atomic::AtomicU64) -> u64 {
    c.load(std::sync::atomic::Ordering::Relaxed)
}

crate::perry_thread_local! {
    static TABLE: RefCell<Option<Table>> = const { RefCell::new(None) };
}

pub(super) fn with<R>(f: impl FnOnce(&mut Table) -> R) -> Option<R> {
    TABLE.with(|t| t.borrow_mut().as_mut().map(f))
}

/// The first death of the run installs the table and the collector hooks:
/// only these check-build entries reach it, so a release program references
/// no installer.
/// The ONE predicate of "alive at a boundary" that a synchronous full decides,
/// shared by the lifetime probes (`life_measure.rs`) and the regions
/// (`life_regions.rs`): the full's mark (or a pin) on the object at user
/// address `addr`. Read between the full's mark propagation and its sweep.
pub(super) fn full_alive(addr: usize) -> bool {
    let header = (addr - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    unsafe { (*header).gc_flags & (crate::gc::GC_FLAG_MARKED | crate::gc::GC_FLAG_PINNED) != 0 }
}

/// Does a probe minor's reach decide "alive at the boundary" by itself? Only
/// when the walk reached the object from a root (`holder` 0..=3: frame, outer
/// frame, shadow stack, global; the order of both probes' holder columns). A
/// minor treats every old object as alive, so a reach through the remembered
/// set (4), another object (5) or an unnamed phase (6) may come from a dead,
/// not yet collected old object (LIFE I3f): the probe confirms it with a
/// synchronous full (`full_alive`). The ONE predicate of both probes.
/// `PERRY_FACTS_SABOTAGE=lifeminor` (negative control) keeps every minor verdict.
pub(super) fn minor_decides(holder: u8) -> bool {
    static SABOTAGE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    holder <= 3 || *SABOTAGE.get_or_init(|| std::env::var("PERRY_FACTS_SABOTAGE").as_deref() == Ok("lifeminor"))
}

/// Is the object at user address `addr` private key/attribute storage of a
/// shape (`GC_FLAG_SHAPE_SHARED`, gc/types.rs)? Engine metadata, never a JS
/// value: the shape table re-mints such a list after a full pruned its shape
/// and its carrier gate holds the list while no JS reference exists (LIFE I3d),
/// so it is never part of a region's inside set. A census birth of compiled
/// code (a lifetime sample) is a JS value and never such storage.
/// `PERRY_FACTS_SABOTAGE=lifemeta` (negative control) counts it as a JS value.
pub(super) fn engine_metadata(addr: usize) -> bool {
    static SABOTAGE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *SABOTAGE.get_or_init(|| std::env::var("PERRY_FACTS_SABOTAGE").as_deref() == Ok("lifemeta")) {
        return false;
    }
    let header = (addr - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    unsafe { (*header).gc_flags & crate::gc::GC_FLAG_SHAPE_SHARED != 0 }
}

pub(super) fn ensure() {
    if TABLE.with(|t| t.borrow().is_none()) {
        install();
    }
}

/// The young user address of a boxed pointer, else `None`.
fn young_addr(bits: u64) -> Option<usize> {
    if bits & TAG_MASK != POINTER_TAG {
        return None;
    }
    let addr = (bits & POINTER_MASK) as usize;
    crate::arena::pointer_in_nursery(addr).then_some(addr)
}

/// A collection the life hooks did not see ran: every young claim's object
/// may be freed and its address reused, so the table is dropped.
pub(super) fn sync(t: &mut Table) {
    let now = crate::gc::diagnostic_hooks::life_collection_count();
    if now == t.synced + 1 && std::mem::take(&mut t.full_pending) {
        t.synced = now;
        return;
    }
    t.full_pending = false;
    if now != t.synced {
        bump(&totals::DROPPED_UNOBSERVED, t.claims.len() as u64);
        let probes: Vec<u32> = t
            .claims
            .values()
            .filter_map(|c| match c.state {
                State::Probe(i) => Some(i),
                _ => None,
            })
            .collect();
        for i in probes {
            super::life_measure::dropped(t, i);
        }
        t.claims.clear();
        t.tokens.clear();
        t.synced = now;
    }
}

/// Installs the table and the collector hooks.
fn install() {
    TABLE.with(|t| {
        *t.borrow_mut() = Some(Table {
            synced: crate::gc::diagnostic_hooks::life_collection_count(),
            ..Default::default()
        })
    });
    crate::gc::diagnostic_hooks::install_life(reached, before_minor, after_minor);
    crate::gc::diagnostic_hooks::install_life_full(super::life_measure::full_marked);
    unsafe {
        libc::atexit(totals_at_exit);
    }
}

/// A copied minor starts: every claim is a from-space object.
fn before_minor() {
    super::life_regions::before_minor();
    let _ = with(|t| {
        sync(t);
        // A minor neither reaches nor frees an object outside the nursery
        // (a pending argument promoted during its call).
        let (young, old): (HashMap<_, _>, HashMap<_, _>) = std::mem::take(&mut t.claims)
            .into_iter()
            .partition(|(a, _)| crate::arena::pointer_in_nursery(*a));
        t.from = young;
        t.claims = old;
    });
}

/// The copied minor ended: every claim it did not reach is freed.
fn after_minor() {
    super::life_regions::after_minor();
    let _ = with(|t| {
        bump(&totals::CHECKED_MINORS, 1);
        let died: Vec<u32> = t
            .from
            .values()
            .filter_map(|c| match c.state {
                State::Probe(i) => Some(i),
                _ => None,
            })
            .collect();
        t.from.clear();
        for i in died {
            super::life_measure::died(t, i);
        }
        t.synced = crate::gc::diagnostic_hooks::life_collection_count();
    });
}

/// The copied minor reached the object at `old` (now at `new`).
fn reached(old: usize, new: usize) {
    super::life_regions::reached(old, new);
    let hit = with(|t| {
        let c = t.from.remove(&old)?;
        match c.state {
            State::Pending(token) => {
                t.claims.insert(new, c);
                t.tokens.insert(token, new);
                None
            }
            State::Probe(i) => {
                t.claims.insert(new, c);
                super::life_measure::reached(t, i, new);
                None
            }
            State::Dead => Some((c, get(&totals::DEATHS))),
        }
    })
    .flatten();
    let Some((claim, deaths)) = hit else { return };
    let (phase, frame) = crate::gc::diagnostic_hooks::life_holder();
    let bet = report(claim, old, phase, frame, deaths);
    if bet {
        bump(&totals::BETS_FAILED, 1);
    }
}

/// Writes the claim's violation line. A bet's line is written once per site
/// and the program continues (returns true); a proven claim exits 86.
fn report(
    claim: Claim,
    addr: usize,
    phase: Option<&'static str>,
    frame: Option<(usize, usize, usize)>,
    deaths: u64,
) -> bool {
    let metadata = unsafe { std::slice::from_raw_parts(claim.site as *const u8, claim.len) };
    let bet = metadata.windows(12).any(|w| w == b"\"proof\":\"bet");
    if bet {
        // One line per claim site: its module, offset, birth and death kind
        // (a site unrolled or copied by the lowering is still one site).
        let id = site_identity(metadata);
        let first = with(|t| t.bets_reported.insert(id)).unwrap_or(true);
        if !first {
            return true;
        }
    }
    let mut line = String::from_utf8_lossy(metadata).into_owned();
    if bet {
        line = line.replacen("PERRY_FALSE_PROOF", "PERRY_BET_FAILED", 1);
    }
    line.push_str(&format!(",\"observed_receiver\":{{\"addr\":{addr}}}"));
    line.push_str(",\"holder\":{\"phase\":");
    match phase {
        Some(p) => line.push_str(&format!("\"{p}\"")),
        None => line.push_str("null"),
    }
    if let Some((function, ip, slot)) = frame.filter(|_| phase.is_some_and(|p| p.contains("native"))) {
        line.push_str(&format!(
            ",\"frame\":{{\"function\":\"{}\",\"function_address\":{function},\"ip\":{ip},\"slot\":{slot}}}",
            symbol(function)
        ));
    }
    line.push_str("},\"collection\":\"minor\"");
    line.push_str(&format!(",\"deaths_executed\":{deaths}}}\n"));
    write_stderr(line.as_bytes());
    if bet {
        return true;
    }
    unsafe { libc::_exit(86) }
}

fn site_identity(metadata: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let text = String::from_utf8_lossy(metadata);
    let field = |name: &str| {
        text.find(name)
            .map(|i| {
                let rest = &text[i + name.len()..];
                rest[..rest.find([',', '}']).unwrap_or(rest.len())].to_string()
            })
            .unwrap_or_default()
    };
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (
        field("\"module\":"),
        field("\"byte_offset\":"),
        field("\"key\":"),
        field("\"at\":"),
    )
        .hash(&mut h);
    h.finish()
}

#[cfg(unix)]
fn symbol(addr: usize) -> String {
    unsafe {
        let mut info: libc::Dl_info = std::mem::zeroed();
        if libc::dladdr(addr as *const libc::c_void, &mut info) != 0 && !info.dli_sname.is_null() {
            return std::ffi::CStr::from_ptr(info.dli_sname)
                .to_string_lossy()
                .replace('"', "'");
        }
    }
    "?".to_string()
}

/// `dladdr` is POSIX; elsewhere every address is the unknown symbol.
#[cfg(not(unix))]
fn symbol(_addr: usize) -> String {
    "?".to_string()
}

fn write_stderr(mut rest: &[u8]) {
    while !rest.is_empty() {
        let n = unsafe { libc::write(2, rest.as_ptr().cast(), rest.len().min(i32::MAX as usize) as _) };
        if n > 0 {
            rest = &rest[n as usize..];
        } else if n < 0
            && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted
        {
            continue;
        } else {
            break;
        }
    }
}

/// One death: claim the object, and on the sampled deaths check it exactly
/// now with a copied minor.
fn die(addr: Option<usize>, site: i64, len: i64, every: i64) {
    CHECKS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let force = with(|t| {
        sync(t);
        bump(&totals::DEATHS, 1);
        let Some(addr) = addr.filter(|a| crate::arena::pointer_in_nursery(*a)) else {
            if addr.is_some() {
                bump(&totals::NON_YOUNG, 1);
            }
            return false;
        };
        t.claims.insert(
            addr,
            Claim {
                site: site as u64,
                len: len as usize,
                state: State::Dead,
            },
        );
        let n = t.per_site.entry(site as u64).or_insert(0);
        let force = *n % (every.max(1) as u64) == 0;
        *n += 1;
        if force {
            bump(&totals::EXACT, 1);
        }
        force
    })
    .unwrap_or(false);
    if force {
        crate::gc::gc_collect_minor();
    }
}

/// A `frame` local's object dies (`kind` 0: an unset slot is a binding
/// that was never born on this path).
#[no_mangle]
pub extern "C" fn perry_verify_facts_life_dead(bits: i64, kind: i64, every: i64, site: i64, len: i64) {
    ensure();
    let bits = bits as u64;
    let addr = young_addr(bits);
    if addr.is_none() {
        if bits & TAG_MASK == POINTER_TAG {
            bump(&totals::NON_YOUNG, 1);
        } else if kind == 0 {
            bump(&totals::UNBORN, 1);
        }
        if bits & TAG_MASK != POINTER_TAG {
            return;
        }
    }
    die(addr, site, len, every);
}

/// An argument birth the next call drops: registered, the call holds it.
#[no_mangle]
pub extern "C" fn perry_verify_facts_life_arg(bits: i64) -> i64 {
    ensure();
    let Some(addr) = young_addr(bits as u64) else {
        bump(&totals::NON_YOUNG, 1);
        return 0;
    };
    with(|t| {
        sync(t);
        t.next_token += 1;
        let token = t.next_token;
        t.claims.insert(
            addr,
            Claim {
                site: 0,
                len: 0,
                state: State::Pending(token),
            },
        );
        t.tokens.insert(token, addr);
        token as i64
    })
    .unwrap_or(0)
}

/// The call returned normally: its dropped argument dies.
#[no_mangle]
pub extern "C" fn perry_verify_facts_life_arg_dead(token: i64, every: i64, site: i64, len: i64) {
    if token == 0 {
        return;
    }
    ensure();
    let addr = with(|t| {
        sync(t);
        let addr = t.tokens.remove(&(token as u64));
        match addr {
            Some(a) if t.claims.get(&a).is_some_and(|c| c.state == State::Pending(token as u64)) => {
                t.claims.remove(&a);
                Some(a)
            }
            _ => {
                bump(&totals::DIED_IN_CALL, 1);
                None
            }
        }
    })
    .flatten();
    if addr.is_some() {
        die(addr, site, len, every);
    }
}

extern "C" fn totals_at_exit() {
    let line = Some(format!(
            "{{\"marker\":\"PERRY_LIFE_TOTALS\",\"deaths_executed\":{},\"exact_at_death\":{},\"collections_checked\":{},\"unborn\":{},\"non_young\":{},\"dropped_unobserved\":{},\"died_in_call\":{},\"bets_failed\":{},\"violations\":0}}\n",
            get(&totals::DEATHS), get(&totals::EXACT), get(&totals::CHECKED_MINORS), get(&totals::UNBORN),
            get(&totals::NON_YOUNG), get(&totals::DROPPED_UNOBSERVED), get(&totals::DIED_IN_CALL),
            get(&totals::BETS_FAILED)
        ));
    if let Some(line) = line {
        if std::env::var_os("PERRY_VERIFY_FACTS_LIFE_REPORT").is_some() {
            write_stderr(line.as_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_non_pointer_frame_death_is_unborn_and_claims_nothing() {
        install_table_only();
        let before = get(&totals::UNBORN);
        perry_verify_facts_life_dead(0x7FFC_0000_0000_0001u64 as i64, 0, 1, 0, 0);
        assert_eq!(get(&totals::UNBORN), before + 1);
        assert_eq!(with(|t| t.claims.len()), Some(0));
    }

    #[test]
    fn an_unregistered_token_claims_nothing() {
        install_table_only();
        let before = get(&totals::DIED_IN_CALL);
        perry_verify_facts_life_arg_dead(7, 1, 0, 0);
        assert_eq!(get(&totals::DIED_IN_CALL), before + 1);
        assert_eq!(with(|t| t.claims.len()), Some(0));
    }

    fn install_table_only() {
        TABLE.with(|t| *t.borrow_mut() = Some(Table::default()));
    }
}
