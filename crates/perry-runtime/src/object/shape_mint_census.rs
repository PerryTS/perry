//! #10868: attribute every ShapeId MINT by cause and by call site.
//!
//! The band is 2^30 and exhaustion is `shape_id_exhausted_abort()` ->
//! `std::process::abort()`. A real `ts.transpileModule` mints ~2.26 M ids, so
//! the question is not "how many shapes are alive" but **why a mint happened
//! at all**. This is the #10287 method — counters that split a storm by CAUSE
//! rather than counting it — applied to the mint funnel instead of the miss
//! handler.
//!
//! Armed by `PERRY_SHAPE_MINT_DIAG=<path|1>`; one relaxed atomic load when
//! unset. Diagnostic only: nothing branches on it for behaviour.
//!
//! The per-mint CALL SITE needs `#[track_caller]` on the shape-publication
//! funnels, and that costs a hidden argument on every call whether or not
//! anyone is measuring. That half sits behind the non-default
//! `shape-mint-callsites` feature: a stock build pays nothing and still
//! produces every count, and a measurement build adds
//! `--features perry-runtime/shape-mint-callsites` to gain the site table.
//!
//! ## What each counter separates
//!
//! `shape_descriptor_ensure_with_holes` de-duplicates on the SIX identity
//! facts (keys-array ADDRESS, logical key count, live inline slot count,
//! semantic generation, object kind, hole count). A mint means no existing
//! descriptor matched all six. Against the family already indexed under this
//! keys address, exactly one of these is true, and they are very different
//! bugs:
//!
//! * `fresh_keys_new_list` — the keys address is new AND its ordered key-NAME
//!   list has never been seen. A genuinely new layout. This is the only class
//!   that *should* scale with program size rather than with work done.
//! * `fresh_keys_known_list` — the keys address is new but the exact ordered
//!   key-name list has been seen before. **A semantically identical shape that
//!   exists only because identity is keyed on the ARRAY ADDRESS, not on the
//!   key list.** `intl/segmenter.rs` and `array/iter_object.rs` already name
//!   this mechanism in their own comments ("a fresh ShapeId per record",
//!   "shape id per call").
//! * `gen_unique` — same keys, same counts, only `semantic_generation`
//!   differs, and the generation came from the monotonic counter
//!   (`transition_object_shape_semantics`, bit 63 clear). One mint per
//!   OPERATION. This is #10287's failure shape.
//! * `gen_deterministic` — the same, but bit 63 set, so the generation came
//!   from `deterministic_semantic_generation`. Shared between receivers that
//!   performed the same transition, so a mint here is a genuinely new
//!   (predecessor, key, attrs) triple.
//! * `key_count` / `slot_bound` / `holes` / `kind` — the keys address is
//!   already known and a structural fact moved: an append, a bound change, a
//!   delete, a class transition. Expected to be proportional to mutations.
//! * `identical` — every fact matches a live sibling and the mint happened
//!   anyway. That is a `facts_key`/`RECORD_FLAG_FACTS_INDEXED` memo failure
//!   and should be ZERO. It is listed so that "it is zero" is a measurement
//!   rather than an assumption.
//!
//! Plus: memo hits (the denominator), retirements and retirement age (churn on
//! objects that die immediately), family-size histogram at mint, and the
//! `#[track_caller]` call site of each mint.

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::hot_diag::{sink_from_env, write_sink, Sink};

/// 0 = not yet resolved, 1 = off, 2 = on. Three states rather than two
/// booleans: a thread that loses the resolve race under the two-boolean form
/// answered "off" and dropped its events, which is the same class of error as
/// a census reading a partial counter.
static STATE: AtomicU8 = AtomicU8::new(0);
static SINK: OnceLock<Option<Sink>> = OnceLock::new();
static MEMO_HITS: AtomicU64 = AtomicU64::new(0);
static MINTS: AtomicU64 = AtomicU64::new(0);

/// How a mint differs from the nearest descriptor already indexed under the
/// same keys-array address.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum MintCause {
    FreshKeysNewList,
    FreshKeysKnownList,
    Identical,
    GenUnique,
    GenDeterministic,
    KeyCount,
    SlotBound,
    Holes,
    Kind,
    Mixed,
}

impl MintCause {
    fn name(self) -> &'static str {
        match self {
            MintCause::FreshKeysNewList => "fresh_keys_new_list",
            MintCause::FreshKeysKnownList => "fresh_keys_known_list",
            MintCause::Identical => "identical (MEMO FAILURE)",
            MintCause::GenUnique => "gen_unique",
            MintCause::GenDeterministic => "gen_deterministic",
            MintCause::KeyCount => "key_count",
            MintCause::SlotBound => "slot_bound",
            MintCause::Holes => "holes",
            MintCause::Kind => "kind",
            MintCause::Mixed => "mixed",
        }
    }
}

#[derive(Default)]
struct Census {
    by_cause: HashMap<MintCause, u64>,
    by_site: HashMap<(&'static str, u32), u64>,
    by_site_cause: HashMap<((&'static str, u32), MintCause), u64>,
    /// Ordered key-name list content hashes ever seen.
    key_lists: HashSet<u64>,
    /// Distinct keys-array addresses ever minted under.
    keys_addrs: HashSet<u64>,
    /// Family size at mint, bucketed: 0,1,2,3-4,5-8,9-16,17-64,65+.
    family_hist: [u64; 8],
    /// Per-key-list mint counts, for the worst offenders.
    per_list: HashMap<u64, (u64, u32)>,
    retires: u64,
    /// Retirement age in mints, bucketed the same way as `family_hist`.
    age_hist: [u64; 8],
    mint_index_of: HashMap<u32, u64>,
    define_outcomes: HashMap<&'static str, u64>,
    /// Lever (iv): the predecessor ShapeId at every prototype divergence. If
    /// this set is about as large as the number of divergences, the
    /// predecessors have already forked and a deterministic generation merges
    /// nothing until identity is canonical; if it is small, it pays now.
    proto_div_preds: HashSet<u32>,
    proto_div_count: u64,
    /// Free-form event tallies (meta records born, objects marked prototype):
    /// the cost drivers for where a prototype serial would live.
    events: HashMap<&'static str, u64>,
}

static CENSUS: OnceLock<Mutex<Census>> = OnceLock::new();

fn census() -> &'static Mutex<Census> {
    CENSUS.get_or_init(|| Mutex::new(Census::default()))
}

fn sink() -> &'static Option<Sink> {
    SINK.get_or_init(|| sink_from_env("PERRY_SHAPE_MINT_DIAG"))
}

/// One relaxed load and a compare on the unarmed path, which is every
/// production build.
#[inline]
pub(crate) fn armed() -> bool {
    match STATE.load(Ordering::Relaxed) {
        1 => false,
        2 => true,
        _ => resolve_arming(),
    }
}

#[cold]
#[inline(never)]
fn resolve_arming() -> bool {
    let on = sink().is_some();
    // Idempotent: every thread computes the same answer from the same env.
    STATE.store(if on { 2 } else { 1 }, Ordering::Relaxed);
    on
}

extern "C" fn exit_dump_shim() {
    dump();
}

/// The measurement rig usually lets the process exit normally; the periodic
/// dump every 2^20 mints is the fallback for a `SIGKILL`.
fn register_exit_dump() {
    static REGISTERED: AtomicBool = AtomicBool::new(false);
    if REGISTERED.swap(true, Ordering::Relaxed) {
        return;
    }
    unsafe {
        libc::atexit(exit_dump_shim);
    }
}

/// Why a `transition_cache_lookup` did not serve an edge. The direct-mapped
/// `Collide` case is the one that matters: the cache is
/// `TRANSITION_CACHE_SIZE = 16384` entries, one edge per slot, so a working
/// set wider than that evicts live edges and every eviction costs a fresh
/// keys array and a fresh ShapeId.
#[cfg_attr(not(feature = "shape-mint-diag"), allow(dead_code))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum TcMiss {
    Empty,
    Collide,
    PlacesKey,
    /// The target array refused the `GC_FLAG_SHAPE_SHARED` stamp.
    Unshared,
    /// The target array's length/capacity no longer matches the edge.
    TargetLen,
    Unstable,
}

static TC_HITS: AtomicU64 = AtomicU64::new(0);
static TC_MISS_EMPTY: AtomicU64 = AtomicU64::new(0);
static TC_MISS_COLLIDE: AtomicU64 = AtomicU64::new(0);
static TC_MISS_PLACES: AtomicU64 = AtomicU64::new(0);
static TC_MISS_UNSHARED: AtomicU64 = AtomicU64::new(0);
static TC_MISS_TARGET_LEN: AtomicU64 = AtomicU64::new(0);
static TC_MISS_UNSTABLE: AtomicU64 = AtomicU64::new(0);
static TC_INSERTS: AtomicU64 = AtomicU64::new(0);
static TC_EVICTIONS: AtomicU64 = AtomicU64::new(0);

#[cfg_attr(not(feature = "shape-mint-diag"), allow(dead_code))]
#[inline]
pub(crate) fn note_transition_hit() {
    if armed() {
        TC_HITS.fetch_add(1, Ordering::Relaxed);
    }
}

#[cfg_attr(not(feature = "shape-mint-diag"), allow(dead_code))]
#[inline]
pub(crate) fn note_transition_miss(kind: TcMiss) {
    if !armed() {
        return;
    }
    let c = match kind {
        TcMiss::Empty => &TC_MISS_EMPTY,
        TcMiss::Collide => &TC_MISS_COLLIDE,
        TcMiss::PlacesKey => &TC_MISS_PLACES,
        TcMiss::Unshared => &TC_MISS_UNSHARED,
        TcMiss::TargetLen => &TC_MISS_TARGET_LEN,
        TcMiss::Unstable => &TC_MISS_UNSTABLE,
    };
    c.fetch_add(1, Ordering::Relaxed);
}

/// `evicted` means the slot already held a DIFFERENT live edge.
#[cfg_attr(not(feature = "shape-mint-diag"), allow(dead_code))]
#[inline]
pub(crate) fn note_transition_insert(evicted: bool) {
    if !armed() {
        return;
    }
    TC_INSERTS.fetch_add(1, Ordering::Relaxed);
    if evicted {
        TC_EVICTIONS.fetch_add(1, Ordering::Relaxed);
    }
}

/// One outcome of the `defineProperty` key-add path's attempt to use or
/// publish a transition edge. #10868 lever (iii): that path performs cache
/// LOOKUPS and never an INSERT on `acc`/`accd`, so every object forks. Which
/// predicate refuses is the question, and guessing it from the source is how
/// #10287's lane got the wrong fix; this counts it instead.
#[cfg_attr(not(feature = "shape-mint-diag"), allow(dead_code))]
#[inline]
pub(crate) fn note_define_outcome(what: &'static str) {
    if !armed() {
        return;
    }
    if let Ok(mut c) = census().lock() {
        *c.define_outcomes.entry(what).or_insert(0) += 1;
    }
}

/// Lever (iv): one prototype divergence, recorded with its predecessor.
#[cfg_attr(not(feature = "shape-mint-diag"), allow(dead_code))]
#[inline]
pub(crate) fn note_proto_divergence(predecessor: u32) {
    if !armed() {
        return;
    }
    if let Ok(mut c) = census().lock() {
        c.proto_div_count += 1;
        c.proto_div_preds.insert(predecessor);
    }
}

/// A free-form event tally.
#[cfg_attr(not(feature = "shape-mint-diag"), allow(dead_code))]
#[inline]
pub(crate) fn note_event(what: &'static str) {
    if !armed() {
        return;
    }
    if let Ok(mut c) = census().lock() {
        *c.events.entry(what).or_insert(0) += 1;
    }
}

#[inline]
pub(crate) fn note_memo_hit() {
    if armed() {
        MEMO_HITS.fetch_add(1, Ordering::Relaxed);
    }
}

fn bucket(n: u64) -> usize {
    match n {
        0 => 0,
        1 => 1,
        2 => 2,
        3..=4 => 3,
        5..=8 => 4,
        9..=16 => 5,
        17..=64 => 6,
        _ => 7,
    }
}

const BUCKET_LABELS: [&str; 8] = ["0", "1", "2", "3-4", "5-8", "9-16", "17-64", "65+"];

/// Content hash of a keys array's ordered key-NAME list. Two arrays at
/// different addresses holding the same names in the same order hash equal,
/// which is exactly the question `fresh_keys_known_list` asks.
///
/// # Safety
/// `keys` is a live keys `ArrayHeader` (or 0) and no shape-table borrow is
/// held: this reads the array and its key strings only.
pub(crate) unsafe fn key_list_content_hash(keys: u64, key_count: u32) -> u64 {
    if keys == 0 {
        return 0;
    }
    let (slots, slot_len) = crate::object::keys_lookup::keys_array_dense_slots_resolved(
        keys as *const crate::array::ArrayHeader,
    );
    if slots.is_null() {
        return u64::MAX;
    }
    let n = (key_count as usize).min(slot_len);
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ ((n as u64) << 32);
    for i in 0..n {
        let v = crate::JSValue::from_bits((*slots.add(i)).to_bits());
        let part = match crate::string::js_string_key_bytes(v, &mut sso) {
            Some(bytes) => crate::object::key_bytes_hash(bytes.as_ptr(), bytes.len()),
            // A non-string key slot (a hole, a symbol) still has to
            // participate, or two different lists could collide.
            None => 0x9E37_79B9_7F4A_7C15u64.wrapping_mul(i as u64 + 1),
        };
        h ^= part;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Which of the six identity facts moved, against the CHEAPEST explanation the
/// family offers — never the worst sibling that happens to be in the list.
///
/// Split out of [`note_mint`] because this is the whole judgement the census
/// makes, and it must be testable without a heap, a shape table or an env var.
#[allow(clippy::too_many_arguments)]
pub(crate) fn classify_mint(
    family_facts: &[(u32, u32, u64, bool, u32)],
    logical_key_count: u32,
    live_inline_slot_count: u32,
    semantic_generation: u64,
    kind_is_class: bool,
    hole_count: u32,
    list_known: bool,
) -> MintCause {
    if family_facts.is_empty() {
        return if list_known {
            MintCause::FreshKeysKnownList
        } else {
            MintCause::FreshKeysNewList
        };
    }
    let mut best = (usize::MAX, MintCause::Mixed);
    for &(lkc, lisc, gen, is_class, holes) in family_facts {
        let mut diffs = 0usize;
        let mut only = MintCause::Mixed;
        if lkc != logical_key_count {
            diffs += 1;
            only = MintCause::KeyCount;
        }
        if lisc != live_inline_slot_count {
            diffs += 1;
            only = MintCause::SlotBound;
        }
        if gen != semantic_generation {
            diffs += 1;
            // Bit 63 is set by `deterministic_semantic_generation` (#10287) and
            // clear for the `SHAPE_SEMANTIC_NEXT` counter. The two are very
            // different findings: the counter mints once per OPERATION.
            only = if semantic_generation & (1 << 63) != 0 {
                MintCause::GenDeterministic
            } else {
                MintCause::GenUnique
            };
        }
        if is_class != kind_is_class {
            diffs += 1;
            only = MintCause::Kind;
        }
        if holes != hole_count {
            diffs += 1;
            only = MintCause::Holes;
        }
        let label = match diffs {
            0 => MintCause::Identical,
            1 => only,
            _ => MintCause::Mixed,
        };
        if diffs < best.0 {
            best = (diffs, label);
        }
        if diffs == 0 {
            break;
        }
    }
    best.1
}

/// Record one mint. `family_facts` is every sibling already indexed under this
/// keys address, as `(logical_key_count, live_inline_slot_count,
/// semantic_generation, kind_is_class, hole_count)`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn note_mint(
    id: u32,
    keys: u64,
    key_list_hash: u64,
    logical_key_count: u32,
    live_inline_slot_count: u32,
    semantic_generation: u64,
    kind_is_class: bool,
    hole_count: u32,
    family_facts: &[(u32, u32, u64, bool, u32)],
    site_file: &'static str,
    site_line: u32,
) {
    if !armed() {
        return;
    }
    register_exit_dump();
    let site = (site_file, site_line);
    let index = MINTS.fetch_add(1, Ordering::Relaxed);
    let Ok(mut c) = census().lock() else {
        return;
    };
    c.keys_addrs.insert(keys);
    let list_known = !c.key_lists.insert(key_list_hash);

    let cause = classify_mint(
        family_facts,
        logical_key_count,
        live_inline_slot_count,
        semantic_generation,
        kind_is_class,
        hole_count,
        list_known,
    );

    *c.by_cause.entry(cause).or_insert(0) += 1;
    *c.by_site.entry(site).or_insert(0) += 1;
    *c.by_site_cause.entry((site, cause)).or_insert(0) += 1;
    let b = bucket(family_facts.len() as u64);
    c.family_hist[b] += 1;
    let e = c
        .per_list
        .entry(key_list_hash)
        .or_insert((0, logical_key_count));
    e.0 += 1;
    c.mint_index_of.insert(id, index);

    if index % (1 << 20) == 0 {
        drop(c);
        dump();
    }
}

pub(crate) fn note_retire(id: u32) {
    if !armed() {
        return;
    }
    let now = MINTS.load(Ordering::Relaxed);
    let Ok(mut c) = census().lock() else {
        return;
    };
    c.retires += 1;
    if let Some(born) = c.mint_index_of.remove(&id) {
        let b = bucket(now.saturating_sub(born));
        c.age_hist[b] += 1;
    }
}

pub(crate) fn dump() {
    let Some(sink) = sink().as_ref() else {
        return;
    };
    let Ok(c) = census().lock() else {
        return;
    };
    let mints = MINTS.load(Ordering::Relaxed);
    let memo = MEMO_HITS.load(Ordering::Relaxed);
    let mut out = String::new();
    out.push_str("[shape-mint census] #10868 — why a ShapeId was minted\n");
    out.push_str(&format!(
        "  ensure() calls        {}\n  memo hits             {}\n  MINTS                 {}\n",
        memo + mints,
        memo,
        mints
    ));
    out.push_str(&format!(
        "  distinct keys ADDRESSES  {}\n  distinct key-NAME lists  {}\n",
        c.keys_addrs.len(),
        c.key_lists.len()
    ));
    if !c.key_lists.is_empty() {
        out.push_str(&format!(
            "  mints per distinct key-name list  {:.1}\n",
            mints as f64 / c.key_lists.len() as f64
        ));
    }
    let tc_h = TC_HITS.load(Ordering::Relaxed);
    let tc_e = TC_MISS_EMPTY.load(Ordering::Relaxed);
    let tc_c = TC_MISS_COLLIDE.load(Ordering::Relaxed);
    let tc_p = TC_MISS_PLACES.load(Ordering::Relaxed);
    let tc_s = TC_MISS_UNSHARED.load(Ordering::Relaxed);
    let tc_t = TC_MISS_TARGET_LEN.load(Ordering::Relaxed);
    let tc_u = TC_MISS_UNSTABLE.load(Ordering::Relaxed);
    let tc_total = tc_h + tc_e + tc_c + tc_p + tc_s + tc_t + tc_u;
    if !cfg!(feature = "shape-mint-diag") {
        out.push_str(
            "  transition cache: NOT COMPILED IN — rebuild with \
             `--features perry-runtime/shape-mint-diag` for the hit/collide/evict\n             split and the per-mint call sites. The zeros below are absence of\n             measurement, not absence of collisions.\n",
        );
    }
    out.push_str(&format!(
        "  transition cache ({} entries, direct-mapped):\n    lookups {}  hits {} ({:.1}%)\n             miss_empty {}  miss_COLLIDE {}  miss_places_key {}  miss_unshared {}  miss_target_len {}  miss_unstable {}\n             inserts {}  EVICTIONS {} ({:.1}%)\n",
        16384,
        tc_total,
        tc_h,
        100.0 * tc_h as f64 / tc_total.max(1) as f64,
        tc_e,
        tc_c,
        tc_p,
        tc_s,
        tc_t,
        tc_u,
        TC_INSERTS.load(Ordering::Relaxed),
        TC_EVICTIONS.load(Ordering::Relaxed),
        100.0 * TC_EVICTIONS.load(Ordering::Relaxed) as f64
            / TC_INSERTS.load(Ordering::Relaxed).max(1) as f64,
    ));
    if !c.define_outcomes.is_empty() {
        out.push_str("  defineProperty key-add, edge outcomes:\n");
        let mut d: Vec<(&&str, &u64)> = c.define_outcomes.iter().collect();
        d.sort_unstable_by(|a, b| b.1.cmp(a.1));
        for (what, n) in d {
            out.push_str(&format!("    {what:<40} {n:>12}\n"));
        }
    }
    out.push_str(&format!(
        "  lever (iv) prototype divergences: {}   distinct predecessor ShapeIds: {}\n",
        c.proto_div_count,
        c.proto_div_preds.len()
    ));
    if !c.events.is_empty() {
        let mut ev: Vec<(&&str, &u64)> = c.events.iter().collect();
        ev.sort_unstable_by(|a, b| b.1.cmp(a.1));
        for (what, n) in ev {
            out.push_str(&format!("  event {what:<36} {n:>12}\n"));
        }
    }
    out.push_str("  by cause:\n");
    let mut causes: Vec<(&MintCause, &u64)> = c.by_cause.iter().collect();
    causes.sort_unstable_by(|a, b| b.1.cmp(a.1));
    for (cause, n) in causes {
        out.push_str(&format!(
            "    {:<26} {:>12}  {:>5.1}%\n",
            cause.name(),
            n,
            100.0 * *n as f64 / mints.max(1) as f64
        ));
    }
    out.push_str("  family size at mint:\n    ");
    for (i, label) in BUCKET_LABELS.iter().enumerate() {
        out.push_str(&format!("{}={} ", label, c.family_hist[i]));
    }
    out.push_str(&format!("\n  retirements           {}\n", c.retires));
    out.push_str("  retirement age (mints between birth and retirement):\n    ");
    for (i, label) in BUCKET_LABELS.iter().enumerate() {
        out.push_str(&format!("{}={} ", label, c.age_hist[i]));
    }
    out.push('\n');
    out.push_str("  top key-name lists by mint count (hash, mints, keys):\n");
    let mut lists: Vec<(&u64, &(u64, u32))> = c.per_list.iter().collect();
    lists.sort_unstable_by(|a, b| b.1 .0.cmp(&a.1 .0).then_with(|| a.0.cmp(b.0)));
    for (h, (n, k)) in lists.into_iter().take(10) {
        out.push_str(&format!("    {h:#018x}  {n:>10}  {k} keys\n"));
    }
    if cfg!(feature = "shape-mint-diag") {
        out.push_str("  top call sites:\n");
    } else {
        out.push_str("  top call sites: NOT COMPILED IN (shape-mint-diag)\n");
    }
    let mut sites: Vec<(&(&str, u32), &u64)> = c.by_site.iter().collect();
    sites.sort_unstable_by(|a, b| b.1.cmp(a.1));
    for (site, n) in sites.into_iter().take(12) {
        let label = format!("{}:{}", site.0, site.1);
        out.push_str(&format!("    {label:<52} {n:>12}\n"));
        let mut per: Vec<(&((&str, u32), MintCause), &u64)> = c
            .by_site_cause
            .iter()
            .filter(|((s, _), _)| s == site)
            .collect();
        per.sort_unstable_by(|a, b| b.1.cmp(a.1));
        for ((_, cause), m) in per.into_iter().take(5) {
            out.push_str(&format!("      {:<26} {:>12}\n", cause.name(), m));
        }
    }
    write_sink(sink, &out);
}

#[cfg(test)]
mod tests {
    use super::*;

    const DET: u64 = 1 << 63;

    /// `(logical_key_count, live_inline_slot_count, semantic_generation,
    /// kind_is_class, hole_count)` — the five facts a sibling contributes.
    /// The sixth, the keys-array address, is what the family is keyed BY.
    fn sib(lkc: u32, lisc: u32, gen: u64, class: bool, holes: u32) -> (u32, u32, u64, bool, u32) {
        (lkc, lisc, gen, class, holes)
    }

    #[test]
    fn an_empty_family_splits_on_whether_the_key_list_was_seen_before() {
        // This is the distinction the whole census exists to draw: a new keys
        // ADDRESS whose ordered key-NAME list is already known is a
        // semantically identical shape, not a new layout.
        assert_eq!(
            classify_mint(&[], 3, 3, 0, false, 0, true),
            MintCause::FreshKeysKnownList
        );
        assert_eq!(
            classify_mint(&[], 3, 3, 0, false, 0, false),
            MintCause::FreshKeysNewList
        );
    }

    #[test]
    fn a_family_member_matching_every_fact_is_a_memo_failure() {
        // `shape_descriptor_ensure_with_holes` de-duplicates on exactly these
        // facts, so reaching a mint with an exact sibling present means the
        // `facts_key` bucket or `RECORD_FLAG_FACTS_INDEXED` failed. The census
        // reports 0 of these on every workload measured; that has to be a
        // MEASUREMENT, which means the classifier has to be able to say it.
        assert_eq!(
            classify_mint(&[sib(3, 3, 7, false, 0)], 3, 3, 7, false, 0, true),
            MintCause::Identical
        );
    }

    #[test]
    fn a_generation_only_difference_splits_on_bit_63() {
        assert_eq!(
            classify_mint(&[sib(3, 3, 1, false, 0)], 3, 3, 2, false, 0, true),
            MintCause::GenUnique
        );
        assert_eq!(
            classify_mint(&[sib(3, 3, 1, false, 0)], 3, 3, DET | 9, false, 0, true),
            MintCause::GenDeterministic
        );
    }

    #[test]
    fn each_single_structural_fact_gets_its_own_label() {
        let base = sib(3, 3, 0, false, 0);
        assert_eq!(
            classify_mint(&[base], 4, 3, 0, false, 0, true),
            MintCause::KeyCount
        );
        assert_eq!(
            classify_mint(&[base], 3, 4, 0, false, 0, true),
            MintCause::SlotBound
        );
        assert_eq!(
            classify_mint(&[base], 3, 3, 0, true, 0, true),
            MintCause::Kind
        );
        assert_eq!(
            classify_mint(&[base], 3, 3, 0, false, 1, true),
            MintCause::Holes
        );
    }

    #[test]
    fn two_facts_moving_at_once_is_mixed_not_the_first_one_found() {
        assert_eq!(
            classify_mint(&[sib(3, 3, 0, false, 0)], 4, 4, 0, false, 0, true),
            MintCause::Mixed
        );
    }

    /// The rule that makes the census honest. Without it the label depends on
    /// the ORDER of the family list, and a mint whose cheapest explanation is
    /// "one key was appended" would be reported as `Mixed` merely because some
    /// unrelated sibling of the same keys array differs in four facts.
    #[test]
    fn the_cheapest_explanation_wins_regardless_of_family_order() {
        let far = sib(9, 9, 5, true, 4);
        let near = sib(3, 3, 0, false, 0);
        assert_eq!(
            classify_mint(&[far, near], 4, 3, 0, false, 0, true),
            MintCause::KeyCount
        );
        assert_eq!(
            classify_mint(&[near, far], 4, 3, 0, false, 0, true),
            MintCause::KeyCount
        );
    }

    /// An exact sibling anywhere in the family outranks every other label, and
    /// the scan stops there.
    #[test]
    fn an_exact_sibling_outranks_a_one_fact_sibling() {
        let one_off = sib(4, 3, 0, false, 0);
        let exact = sib(3, 3, 0, false, 0);
        assert_eq!(
            classify_mint(&[one_off, exact], 3, 3, 0, false, 0, true),
            MintCause::Identical
        );
    }

    /// The instrument is inert unless `PERRY_SHAPE_MINT_DIAG` is set. Tests run
    /// without it, so this also pins that a test run never accumulates state.
    #[test]
    fn the_census_is_unarmed_by_default() {
        assert!(
            !armed(),
            "PERRY_SHAPE_MINT_DIAG must not be set in the test env"
        );
        note_memo_hit();
        note_transition_hit();
        note_transition_miss(TcMiss::Collide);
        note_transition_insert(true);
        assert_eq!(MEMO_HITS.load(Ordering::Relaxed), 0);
        assert_eq!(TC_HITS.load(Ordering::Relaxed), 0);
        assert_eq!(TC_MISS_COLLIDE.load(Ordering::Relaxed), 0);
        assert_eq!(TC_INSERTS.load(Ordering::Relaxed), 0);
    }
}
