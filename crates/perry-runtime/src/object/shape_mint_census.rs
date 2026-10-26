//! #10868: attribute every ShapeId MINT by cause and by call site.
//!
//! The band is 2^30 and exhaustion is `shape_id_exhausted_abort()` ->
//! `std::process::abort()`. A real `ts.transpileModule` mints ~2.26 M ids, so
//! the question is not "how many shapes are alive" but **why a mint happened
//! at all**. This is the #10287 method — counters that split a storm by CAUSE
//! rather than counting it — applied to the mint funnel instead of the miss
//! handler.
//!
//! Armed by `PERRY_SHAPE_MINT_DIAG=<path|1>` in a build with
//! `shape-mint-diag`. Production call sites and caller-location arguments are
//! compiled out without that feature. Unit tests also compile the mint hooks,
//! so the small census gate runs in ordinary CI's runtime lib-test target.
//!
//! The layout census compares ordered key identities AND attribute entries,
//! just as the canonical trie does. Name-only totals remain descriptive, not
//! a duplicate-layout verdict: writable and readonly versions of the same
//! names are different layouts. A null keys edge owns no backing.
//!
//! Mint causes compare the complete record identity used by the by-facts
//! memo, including prototype, representation, ConstFn bodies, brands,
//! integrity and the exact object kind. An `identical` mint is therefore a
//! real memo failure rather than a sibling differing in an omitted fact.

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::codegen_abi::ShapeId;
use crate::hot_diag::{sink_from_env, write_sink, Sink};

// 0 = not yet resolved, 1 = off, 2 = on. Three states rather than two
// booleans: a thread that loses the resolve race under the two-boolean form
// answered "off" and dropped its events, which is the same class of error as
// a census reading a partial counter.
//
// `STATE`, `MEMO_HITS`, `MINTS`, `CENSUS` and the `TC_*` counters are
// `per_test_global!`: the gate tests arm the census and then assert on its
// totals, so with process-wide storage every other test's concurrent shape
// mint landed in the armed census (and moved the counters) and the gate read
// the whole parallel suite instead of its own workload. Each test thread
// gets its own state in a test build; outside one these are the plain
// statics.
per_test_global! {
    static STATE: AtomicU8 = AtomicU8::new(0);
}
static SINK: OnceLock<Option<Sink>> = OnceLock::new();
per_test_global! {
    static MEMO_HITS: AtomicU64 = AtomicU64::new(0);
    static MINTS: AtomicU64 = AtomicU64::new(0);
}

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
    Prototype,
    Attributes,
    Representation,
    ConstFn,
    Brands,
    Integrity,
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
            MintCause::Prototype => "prototype",
            MintCause::Attributes => "attributes",
            MintCause::Representation => "representation",
            MintCause::ConstFn => "constfn",
            MintCause::Brands => "brands",
            MintCause::Integrity => "integrity",
        }
    }
}

/// Owned diagnostic snapshot of the authoritative record's complete identity.
/// Representation deprecation bits are storage state, as in the real memo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MintFacts {
    pub logical_key_count: u32,
    pub live_inline_slot_count: u32,
    pub semantic_generation: u64,
    pub kind: super::shapes::ShapeObjectKind,
    pub hole_count: u32,
    pub proto_id: u64,
    pub summary: u8,
    pub rep: u64,
    pub infos: Vec<super::shapes::ConstFnSlotInfo>,
    pub brands: Vec<u64>,
    pub integrity: u16,
}

impl MintFacts {
    pub(crate) fn of(record: &super::shapes::ShapeDescriptor) -> Self {
        Self {
            logical_key_count: record.logical_key_count,
            live_inline_slot_count: record.live_inline_slot_count,
            semantic_generation: record.semantic_generation,
            kind: record.object_kind,
            hole_count: record.hole_count,
            proto_id: record.proto_id,
            summary: record.summary,
            rep: super::field_rep::identity_with_special(record.rep),
            infos: record.constfn_infos().to_vec(),
            brands: record.brands().to_vec(),
            integrity: record.integrity_flags,
        }
    }
}

#[derive(Clone, Copy)]
struct LayoutOrigin {
    keys: u64,
    count: u32,
    file: &'static str,
    line: u32,
}

#[derive(Clone, Copy)]
struct LayoutObservation {
    hash: u64,
    origin: LayoutOrigin,
}

struct DuplicateLayout {
    hash: u64,
    first: LayoutOrigin,
    next: LayoutOrigin,
}

#[derive(Default)]
struct Census {
    by_cause: HashMap<MintCause, u64>,
    by_site: HashMap<(&'static str, u32), u64>,
    by_site_cause: HashMap<((&'static str, u32), MintCause), u64>,
    /// Ordered key-name list content hashes ever seen.
    key_lists: HashSet<u64>,
    /// Distinct `(keys-array address, key count)` lists ever minted under.
    /// A pair, not the address: the lists of one growth chain share a
    /// backing array, so the address alone names a chain.
    keys_addrs: HashSet<(u64, u32)>,
    layout_lists: HashSet<u64>,
    layout_addrs: HashSet<(u64, u32)>,
    // Active observations follow record retirement; an expired weak backing
    // can be published again. Every OVERLAP event is retained, so retirement
    // cannot turn an earlier producer bypass into a passing census.
    layout_by_id: HashMap<u32, LayoutObservation>,
    layout_first: HashMap<u64, LayoutOrigin>,
    duplicate_layouts: Vec<DuplicateLayout>,
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
    /// Distinct (predecessor, prototype) pairs — what a deterministic
    /// generation would actually collapse to. Keyed on the prototype's BITS for
    /// this one measurement only: an object prototype that moves under the
    /// collector counts twice, so this is an UPPER bound. A null prototype is
    /// the constant TAG_NULL and is exact.
    proto_div_pairs: HashSet<(u32, u64)>,
    proto_div_null: u64,
    /// Free-form event tallies (meta records born, objects marked prototype):
    /// the cost drivers for where a prototype serial would live.
    events: HashMap<&'static str, u64>,
}

per_test_global! {
    static CENSUS: OnceLock<Mutex<Census>> = OnceLock::new();
}

fn census() -> &'static Mutex<Census> {
    crate::once_init::get_or_init(&CENSUS, || Mutex::new(Census::default()))
}

fn sink() -> &'static Option<Sink> {
    crate::once_init::get_or_init(&SINK, || sink_from_env("PERRY_SHAPE_MINT_DIAG"))
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

per_test_global! {
    static TC_HITS: AtomicU64 = AtomicU64::new(0);
    static TC_MISS_EMPTY: AtomicU64 = AtomicU64::new(0);
    static TC_MISS_COLLIDE: AtomicU64 = AtomicU64::new(0);
    static TC_MISS_PLACES: AtomicU64 = AtomicU64::new(0);
    static TC_MISS_UNSHARED: AtomicU64 = AtomicU64::new(0);
    static TC_MISS_TARGET_LEN: AtomicU64 = AtomicU64::new(0);
    static TC_MISS_UNSTABLE: AtomicU64 = AtomicU64::new(0);
    /// Hits the value class refused (charter step 5, T2): counted in `TC_HITS`
    /// too, since the lookup matched before the class guard ran.
    static TC_REP_REFUSED: AtomicU64 = AtomicU64::new(0);
}

pub(crate) fn note_transition_rep_refused() {
    if armed() {
        TC_REP_REFUSED.fetch_add(1, Ordering::Relaxed);
    }
}
per_test_global! {
    static TC_INSERTS: AtomicU64 = AtomicU64::new(0);
    static TC_EVICTIONS: AtomicU64 = AtomicU64::new(0);
}

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
pub(crate) fn note_proto_divergence(predecessor: u32, proto_bits: u64) {
    if !armed() {
        return;
    }
    if let Ok(mut c) = census().lock() {
        c.proto_div_count += 1;
        c.proto_div_preds.insert(predecessor);
        c.proto_div_pairs.insert((predecessor, proto_bits));
        if proto_bits == crate::value::TAG_NULL {
            c.proto_div_null += 1;
        }
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
    key_list_hash(keys, key_count, false)
}

/// The canonical trie compares slot identity AND each slot's attribute entry.
/// Names alone collapse distinct descriptor layouts; symbols need identity.
pub(crate) unsafe fn key_layout_hash(keys: u64, key_count: u32) -> u64 {
    key_list_hash(keys, key_count, true)
}

unsafe fn key_list_hash(keys: u64, key_count: u32, attributes: bool) -> u64 {
    if key_count == 0 {
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
            None => v.bits(),
        };
        h ^= part;
        if attributes {
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            h ^= u64::from(super::key_attrs::keys_entry(keys as *const _, i as u32));
        }
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Which identity facts moved, against the CHEAPEST explanation the
/// family offers — never the worst sibling that happens to be in the list.
///
/// Split out of [`note_mint`] because this is the whole judgement the census
/// makes, and it must be testable without a heap, a shape table or an env var.
pub(crate) fn classify_mint(
    family_facts: &[MintFacts],
    facts: &MintFacts,
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
    for sibling in family_facts {
        let mut diffs = 0;
        let mut only = MintCause::Mixed;
        macro_rules! compare {
            ($field:ident, $cause:expr) => {
                if sibling.$field != facts.$field {
                    diffs += 1;
                    only = $cause;
                }
            };
        }
        compare!(logical_key_count, MintCause::KeyCount);
        compare!(live_inline_slot_count, MintCause::SlotBound);
        compare!(
            semantic_generation,
            if facts.semantic_generation & (1 << 63) != 0 {
                MintCause::GenDeterministic
            } else {
                MintCause::GenUnique
            }
        );
        compare!(kind, MintCause::Kind);
        compare!(hole_count, MintCause::Holes);
        compare!(proto_id, MintCause::Prototype);
        compare!(summary, MintCause::Attributes);
        compare!(rep, MintCause::Representation);
        compare!(infos, MintCause::ConstFn);
        compare!(brands, MintCause::Brands);
        compare!(integrity, MintCause::Integrity);
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

/// Record one mint against complete sibling facts under this keys address.
#[allow(clippy::too_many_arguments)]
pub(crate) fn note_mint(
    id: ShapeId,
    keys: u64,
    key_list_hash: u64,
    layout_hash: u64,
    facts: &MintFacts,
    family_facts: &[MintFacts],
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
    let logical_key_count = facts.logical_key_count;
    c.keys_addrs.insert((keys, logical_key_count));
    c.key_lists.insert(key_list_hash);
    // Null denotes no backing, not a second allocation for the empty list.
    let list_known = c.layout_lists.contains(&layout_hash);
    if keys != 0 {
        let current = LayoutOrigin {
            keys,
            count: logical_key_count,
            file: site_file,
            line: site_line,
        };
        c.layout_by_id.insert(
            id.0,
            LayoutObservation {
                hash: layout_hash,
                origin: current,
            },
        );
        if c.layout_addrs.insert((keys, logical_key_count)) {
            if let Some(&first) = c.layout_first.get(&layout_hash) {
                c.duplicate_layouts.push(DuplicateLayout {
                    hash: layout_hash,
                    first,
                    next: current,
                });
            } else {
                c.layout_first.insert(layout_hash, current);
            }
        }
        c.layout_lists.insert(layout_hash);
    }
    let cause = classify_mint(family_facts, facts, list_known);

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
    c.mint_index_of.insert(id.0, index);

    if index % (1 << 20) == 0 {
        drop(c);
        dump();
    }
}

pub(crate) fn note_retire(id: ShapeId) {
    if !armed() {
        return;
    }
    let now = MINTS.load(Ordering::Relaxed);
    let Ok(mut c) = census().lock() else {
        return;
    };
    if let Some(retired) = c.layout_by_id.remove(&id.0) {
        let origin = retired.origin;
        if !c
            .layout_by_id
            .values()
            .any(|v| (v.origin.keys, v.origin.count) == (origin.keys, origin.count))
        {
            c.layout_addrs.remove(&(origin.keys, origin.count));
        }
        let survivor = c
            .layout_by_id
            .values()
            .find(|v| v.hash == retired.hash)
            .copied();
        if let Some(survivor) = survivor {
            c.layout_first.insert(retired.hash, survivor.origin);
        } else {
            c.layout_lists.remove(&retired.hash);
            c.layout_first.remove(&retired.hash);
        }
    }
    c.retires += 1;
    if let Some(born) = c.mint_index_of.remove(&id.0) {
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
        "  distinct keys ADDRESSES  {} (address, count)\n  distinct key-NAME lists  {}\n",
        c.keys_addrs.len(),
        c.key_lists.len()
    ));
    out.push_str(&format!(
        "  distinct live key layouts (names + attributes) {}\n  distinct live layout addresses {}\n",
        c.layout_lists.len(),
        c.layout_addrs.len()
    ));
    if !c.key_lists.is_empty() {
        out.push_str(&format!(
            "  mints per distinct key-name list  {:.1}\n",
            mints as f64 / c.key_lists.len() as f64
        ));
    }
    // An independent content census: no trie node ids participate. Compare
    // allocated backings with layouts INCLUDING each key's attribute entry.
    // Descriptive name-only totals above do not imply layout equivalence.
    {
        let (live, canon_minted, reaped) = crate::object::canonical_keys::canonical_stats();
        let words = crate::object::canonical_keys::canonical_element_words();
        let (published, backings, backing_slots, in_place) =
            crate::object::canonical_keys::canonical_storage_stats();
        out.push_str(&format!(
            "  canonical keys trie: live {}  minted {}  reaped {}  listed keys {}\n",
            live, canon_minted, reaped, words
        ));
        out.push_str(&format!(
            "  canonical storage: published lists {}  backings {}  slots {} ({} KB)  \
             in-place appends {}\n",
            published,
            backings,
            backing_slots,
            backing_slots * 8 / 1024,
            in_place
        ));
        // A producer allocating around the funnel increases backing identities
        // without introducing a new layout, which the negative control detects.
        let addrs = c.layout_addrs.len();
        let lists = c.layout_lists.len();
        if !c.duplicate_layouts.is_empty() {
            out.push_str(&format!(
                "  FUNNEL BROKEN: {} duplicate layouts observed while indexed backings overlapped; \
                 current {} keys addresses for {} live key layouts (names + attributes)\n",
                c.duplicate_layouts.len(),
                addrs,
                lists
            ));
        } else {
            out.push_str(
                "  FUNNEL OK: 0 duplicate layouts; one keys backing per live key layout (names + attributes)\n",
            );
        }
    }
    out.push_str(&format!(
        "  identical mints (complete facts) {}\n",
        c.by_cause.get(&MintCause::Identical).copied().unwrap_or(0)
    ));
    for duplicate in &c.duplicate_layouts {
        let first = duplicate.first;
        let next = duplicate.next;
        out.push_str(&format!("  duplicate layout {:#018x}: first keys={:#x} count={} {}:{}; next keys={:#x} count={} {}:{}\n",
            duplicate.hash, first.keys, first.count, first.file, first.line, next.keys, next.count, next.file, next.line));
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
        "  transition cache ({} entries, direct-mapped):\n    lookups {}  hits {} ({:.1}%)\n             miss_empty {}  miss_COLLIDE {}  miss_places_key {}  miss_unshared {}  miss_target_len {}  miss_unstable {}\n             rep_refused {} (of the hits: the value class did not fit the target, T2)\n             inserts {}  EVICTIONS {} ({:.1}%)\n",
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
        TC_REP_REFUSED.load(Ordering::Relaxed),
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
        "  lever (iv) prototype divergences: {}   distinct predecessor ShapeIds: {}\n    \
         distinct (predecessor, prototype) pairs: {} (upper bound)   to a NULL prototype: {}\n",
        c.proto_div_count,
        c.proto_div_preds.len(),
        c.proto_div_pairs.len(),
        c.proto_div_null,
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

    /// A fixture with default prototype, representation and receiver facts.
    fn sib(lkc: u32, lisc: u32, gen: u64, class: bool, holes: u32) -> MintFacts {
        MintFacts {
            logical_key_count: lkc,
            live_inline_slot_count: lisc,
            semantic_generation: gen,
            kind: if class {
                super::super::shapes::ShapeObjectKind::Class
            } else {
                super::super::shapes::ShapeObjectKind::Ordinary
            },
            hole_count: holes,
            proto_id: 0,
            summary: 0,
            rep: 0,
            infos: vec![],
            brands: vec![],
            integrity: 0,
        }
    }

    #[test]
    fn an_empty_family_splits_on_whether_the_key_list_was_seen_before() {
        // This is the distinction the whole census exists to draw: a new keys
        // ADDRESS whose ordered key-NAME list is already known is a
        // semantically identical shape, not a new layout.
        assert_eq!(
            classify_mint(&[], &sib(3, 3, 0, false, 0), true),
            MintCause::FreshKeysKnownList
        );
        assert_eq!(
            classify_mint(&[], &sib(3, 3, 0, false, 0), false),
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
            classify_mint(&[sib(3, 3, 7, false, 0)], &sib(3, 3, 7, false, 0), true),
            MintCause::Identical
        );
    }

    #[test]
    fn a_generation_only_difference_splits_on_bit_63() {
        assert_eq!(
            classify_mint(&[sib(3, 3, 1, false, 0)], &sib(3, 3, 2, false, 0), true),
            MintCause::GenUnique
        );
        assert_eq!(
            classify_mint(
                &[sib(3, 3, 1, false, 0)],
                &sib(3, 3, DET | 9, false, 0),
                true
            ),
            MintCause::GenDeterministic
        );
    }

    #[test]
    fn each_single_structural_fact_gets_its_own_label() {
        let base = sib(3, 3, 0, false, 0);
        assert_eq!(
            classify_mint(&[base.clone()], &sib(4, 3, 0, false, 0), true),
            MintCause::KeyCount
        );
        assert_eq!(
            classify_mint(&[base.clone()], &sib(3, 4, 0, false, 0), true),
            MintCause::SlotBound
        );
        assert_eq!(
            classify_mint(&[base.clone()], &sib(3, 3, 0, true, 0), true),
            MintCause::Kind
        );
        assert_eq!(
            classify_mint(&[base.clone()], &sib(3, 3, 0, false, 1), true),
            MintCause::Holes
        );
    }

    #[test]
    fn two_facts_moving_at_once_is_mixed_not_the_first_one_found() {
        assert_eq!(
            classify_mint(&[sib(3, 3, 0, false, 0)], &sib(4, 4, 0, false, 0), true),
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
            classify_mint(&[far.clone(), near.clone()], &sib(4, 3, 0, false, 0), true),
            MintCause::KeyCount
        );
        assert_eq!(
            classify_mint(&[near, far], &sib(4, 3, 0, false, 0), true),
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
            classify_mint(&[one_off, exact], &sib(3, 3, 0, false, 0), true),
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

#[cfg(test)]
#[path = "shape_mint_census_gate_tests.rs"]
mod gate_tests;
