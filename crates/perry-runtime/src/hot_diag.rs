//! Counter-first instruments for the mutator paths a TUI keystroke exercises.
//!
//! * `PERRY_REGEX_DIAG=<path>` — RegExp construction / lazy build / cache
//!   clears / exec-family calls, plus a per-pattern table (keyed by the
//!   pattern `StringHeader` address, merged by content prefix at dump time).
//! * `PERRY_IC_DIAG=<path>` — property-read inline-cache misses split by the
//!   REASON the handler took (receiver kind, own/inherited, prime outcome),
//!   with a per-site table keyed by the site's cache slot.
//!
//! `<path>` is a file; `1`/`stderr` writes to stderr. A snapshot is written
//! every ~1 s of activity — the measurement rig kills the process with
//! `SIGKILL`, so an exit hook alone would never fire — and the snapshot
//! replaces the previous one (write to `<path>.tmp`, then rename). Both
//! instruments are diagnostic only: nothing may branch on them for behaviour,
//! and when the variable is unset every probe is one relaxed atomic load.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

/// How the diag output is delivered.
#[derive(Clone)]
enum Sink {
    Stderr,
    File(String),
}

fn sink_from_env(name: &str) -> Option<Sink> {
    let raw = std::env::var(name).ok()?;
    let raw = raw.trim();
    match raw {
        "" | "0" | "off" | "false" | "no" => None,
        "1" | "stderr" | "on" | "true" | "yes" => Some(Sink::Stderr),
        path => Some(Sink::File(path.to_string())),
    }
}

fn write_sink(sink: &Sink, text: &str) {
    match sink {
        Sink::Stderr => eprint!("{text}"),
        Sink::File(path) => {
            let tmp = format!("{path}.tmp");
            if std::fs::write(&tmp, text).is_ok() {
                let _ = std::fs::rename(&tmp, path);
            }
        }
    }
}

/// Events between two "should we dump?" clock reads.
const TICK_EVERY: u32 = 256;
const DUMP_INTERVAL_MS: u128 = 1000;

// ---------------------------------------------------------------------------
// RegExp
// ---------------------------------------------------------------------------

static REGEX_SINK: OnceLock<Option<Sink>> = OnceLock::new();
static REGEX_ON: AtomicBool = AtomicBool::new(false);

/// One-time env parse; arms [`REGEX_ON`]. Called from the first probe.
fn regex_sink() -> &'static Option<Sink> {
    REGEX_SINK.get_or_init(|| {
        let sink = sink_from_env("PERRY_REGEX_DIAG");
        REGEX_ON.store(sink.is_some(), Ordering::Relaxed);
        sink
    })
}

/// Is the regex instrument armed? One relaxed load once initialised.
#[inline]
pub fn regex_on() -> bool {
    if REGEX_SINK.get().is_none() {
        regex_sink();
    }
    REGEX_ON.load(Ordering::Relaxed)
}

#[derive(Default)]
struct PatStat {
    prefix: String,
    byte_len: u32,
    flags: String,
    news: u64,
    builds: u64,
    execs: u64,
    tests: u64,
    replaces: u64,
    matches: u64,
}

#[derive(Default)]
pub struct RegexDiag {
    started: Option<Instant>,
    last_dump: Option<Instant>,
    events: u32,
    pub new_calls: u64,
    /// `js_regexp_new` found `(pattern, flags)` in `VALIDATED_PATTERNS`.
    pub new_validated_hit: u64,
    /// `js_regexp_new` answered from the literal-site cache (no validation,
    /// no owned copies, programs installed eagerly).
    pub new_site_hit: u64,
    /// Sum of pattern bytes seen by `js_regexp_new` (what a content hash or
    /// copy of the pattern costs per construction).
    pub new_pattern_bytes: u64,
    /// `js_regexp_new` had to allocate a GC string for the canonical flags
    /// because the caller's flags string was not already in canonical form.
    /// The common case — a regex literal, whose flags text the author wrote in
    /// spec order — shares the caller's immutable string instead, so this
    /// counter is the per-construction flags allocation that remains.
    pub new_flags_allocated: u64,
    pub compiles_std: u64,
    pub compiles_fancy: u64,
    pub compiles_repeat: u64,
    pub cache_clears: u64,
    /// `lazy::build_and_install_programs` runs (one per header that is
    /// executed at least once).
    pub lazy_builds: u64,
    /// Of those, the standard-engine program came from `REGEX_CACHE`.
    pub lazy_cache_hits: u64,
    pub exec_calls: u64,
    pub exec_matched: u64,
    pub exec_capture_slots: u64,
    pub exec_capture_bytes: u64,
    pub test_calls: u64,
    /// `test` on a global/sticky receiver (used to build a full exec array).
    pub test_global: u64,
    pub match_calls: u64,
    pub replace_calls: u64,
    pub replace_matches: u64,
    pub split_calls: u64,
    per_pattern: HashMap<usize, PatStat>,
}

crate::perry_thread_local! {
    static REGEX_DIAG: RefCell<RegexDiag> = RefCell::new(RegexDiag::default());
}

/// Run `f` against the thread's regex counters, then maybe dump.
#[inline]
pub fn regex_with(f: impl FnOnce(&mut RegexDiag)) {
    REGEX_DIAG.with(|d| {
        let mut d = d.borrow_mut();
        if d.started.is_none() {
            d.started = Some(Instant::now());
            d.last_dump = d.started;
        }
        f(&mut d);
        d.events = d.events.wrapping_add(1);
        if d.events % TICK_EVERY == 0 {
            let due = d
                .last_dump
                .is_some_and(|t| t.elapsed().as_millis() >= DUMP_INTERVAL_MS);
            if due {
                d.last_dump = Some(Instant::now());
                if let Some(sink) = regex_sink() {
                    write_sink(sink, &d.render());
                }
            }
        }
    });
}

impl RegexDiag {
    fn pat(&mut self, pattern_addr: usize, pattern: &[u8], flags: &str) -> &mut PatStat {
        let entry = self.per_pattern.entry(pattern_addr).or_default();
        if entry.prefix.is_empty() && entry.byte_len == 0 {
            let n = pattern.len().min(48);
            entry.prefix = String::from_utf8_lossy(&pattern[..n]).into_owned();
            entry.byte_len = pattern.len() as u32;
            entry.flags = flags.to_string();
        }
        entry
    }

    /// Record one `js_regexp_new`.
    pub fn note_new(
        &mut self,
        pattern_addr: usize,
        pattern: &[u8],
        flags: &str,
        validated_hit: bool,
        site_hit: bool,
    ) {
        self.new_calls += 1;
        self.new_pattern_bytes += pattern.len() as u64;
        if validated_hit {
            self.new_validated_hit += 1;
        }
        if site_hit {
            self.new_site_hit += 1;
        }
        self.pat(pattern_addr, pattern, flags).news += 1;
    }

    /// Record one lazy program build for a header.
    pub fn note_build(
        &mut self,
        pattern_addr: usize,
        pattern: &[u8],
        flags: &str,
        cache_hit: bool,
    ) {
        self.lazy_builds += 1;
        if cache_hit {
            self.lazy_cache_hits += 1;
        }
        self.pat(pattern_addr, pattern, flags).builds += 1;
    }

    /// Record one exec-family call against a header's pattern.
    pub fn note_op(&mut self, pattern_addr: usize, pattern: &[u8], flags: &str, op: RegexOp) {
        let stat = self.pat(pattern_addr, pattern, flags);
        match op {
            RegexOp::Exec => {
                stat.execs += 1;
            }
            RegexOp::Test => {
                stat.tests += 1;
            }
            RegexOp::Replace => {
                stat.replaces += 1;
            }
            RegexOp::Match => {
                stat.matches += 1;
            }
        }
        match op {
            RegexOp::Exec => self.exec_calls += 1,
            RegexOp::Test => self.test_calls += 1,
            RegexOp::Replace => self.replace_calls += 1,
            RegexOp::Match => self.match_calls += 1,
        }
    }

    fn render(&self) -> String {
        use std::fmt::Write as _;
        let mut out = String::with_capacity(4096);
        let secs = self.started.map_or(0.0, |t| t.elapsed().as_secs_f64());
        let _ = writeln!(
            out,
            "[regex-diag] t={secs:.1}s new={} validated_hit={} site_hit={} pattern_bytes={} \
             compiles std={} fancy={} repeat={} cache_clears={} lazy_builds={} lazy_cache_hits={} \
             exec={} exec_matched={} capture_slots={} capture_bytes={} test={} test_global={} \
             match={} replace={} replace_matches={} split={} flags_alloc={}",
            self.new_calls,
            self.new_validated_hit,
            self.new_site_hit,
            self.new_pattern_bytes,
            self.compiles_std,
            self.compiles_fancy,
            self.compiles_repeat,
            self.cache_clears,
            self.lazy_builds,
            self.lazy_cache_hits,
            self.exec_calls,
            self.exec_matched,
            self.exec_capture_slots,
            self.exec_capture_bytes,
            self.test_calls,
            self.test_global,
            self.match_calls,
            self.replace_calls,
            self.replace_matches,
            self.split_calls,
            self.new_flags_allocated,
        );
        // Merge by content (prefix, len, flags): distinct literal sites with
        // the same pattern are one row.
        let mut merged: HashMap<(String, u32, String), PatStat> = HashMap::new();
        for p in self.per_pattern.values() {
            let e = merged
                .entry((p.prefix.clone(), p.byte_len, p.flags.clone()))
                .or_default();
            e.news += p.news;
            e.builds += p.builds;
            e.execs += p.execs;
            e.tests += p.tests;
            e.replaces += p.replaces;
            e.matches += p.matches;
        }
        let mut rows: Vec<_> = merged.into_iter().collect();
        rows.sort_by_key(|(_, s)| {
            std::cmp::Reverse(s.news * (1 + s.builds) + s.execs + s.tests + s.replaces + s.matches)
        });
        let _ = writeln!(
            out,
            "  news builds execs tests replaces matches  len flags pattern-prefix ({} distinct)",
            rows.len()
        );
        for ((prefix, len, flags), s) in rows.iter().take(40) {
            let _ = writeln!(
                out,
                "  {:5} {:6} {:5} {:5} {:8} {:7}  {len:5} /{flags}/ {}",
                s.news,
                s.builds,
                s.execs,
                s.tests,
                s.replaces,
                s.matches,
                prefix.replace('\n', "\\n")
            );
        }
        out
    }
}

/// Which exec-family entry point recorded an operation.
#[derive(Clone, Copy)]
pub enum RegexOp {
    Exec,
    Test,
    Replace,
    Match,
}

// ---------------------------------------------------------------------------
// Property-read inline-cache misses
// ---------------------------------------------------------------------------

static IC_SINK: OnceLock<Option<Sink>> = OnceLock::new();
static IC_ON: AtomicBool = AtomicBool::new(false);

fn ic_sink() -> &'static Option<Sink> {
    IC_SINK.get_or_init(|| {
        let sink = sink_from_env("PERRY_IC_DIAG");
        IC_ON.store(sink.is_some(), Ordering::Relaxed);
        sink
    })
}

// ---------------------------------------------------------------------------
// Per-object layout tables: occupancy vs the address filter that gates them
// ---------------------------------------------------------------------------

static LAYOUT_SINK: OnceLock<Option<Sink>> = OnceLock::new();
static LAYOUT_ON: AtomicBool = AtomicBool::new(false);

fn layout_sink() -> &'static Option<Sink> {
    LAYOUT_SINK.get_or_init(|| {
        let sink = sink_from_env("PERRY_LAYOUT_DIAG");
        LAYOUT_ON.store(sink.is_some(), Ordering::Relaxed);
        sink
    })
}

/// Is the per-object-layout occupancy instrument armed?
#[inline]
pub fn layout_on() -> bool {
    if LAYOUT_SINK.get().is_none() {
        layout_sink();
    }
    LAYOUT_ON.load(Ordering::Relaxed)
}

/// One collection's view of the per-object layout tables and the 4096-bit
/// address filter that is supposed to keep evacuation off them.
///
/// The question this exists to settle: `layout_addr_filter_may_hold` is
/// documented for "one or two entries … ~0.05 % false-positive rate", and
/// `transfer_per_object_descriptor` / `transfer_per_object_slot_mask` return
/// early only when it says no. If the tables hold far more keys than the
/// filter has bits, it answers "maybe" for every address, both early returns
/// stop firing, and every evacuated object pays the full two-map hash path —
/// while nothing in the system says so out loud.
///
/// `keys` is also what a filter rebuild used to cost: one `Vec<usize>` of
/// every live key per prune, plus a second full walk to recount the young
/// records.
#[derive(Default)]
pub struct LayoutDiag {
    prunes: u64,
    typed_len: usize,
    masks_len: usize,
    typed_max: usize,
    masks_max: usize,
    /// Set bits in the address filter, and its capacity in bits.
    filter_bits_set: usize,
    filter_bits_total: usize,
    filter_bits_set_max: usize,
    /// Live keys past which a rebuild stops producing a selective filter.
    useful_keys: usize,
    /// Prunes that rebuilt the filter from the survivors, and prunes that
    /// found it already outgrown and skipped the walk.
    rebuilt: u64,
    outgrown: u64,
    /// Keys visited by prunes that DID rebuild — the walk that is still paid.
    rebuilt_keys: u64,
}

crate::perry_thread_local! {
    static LAYOUT_DIAG: RefCell<LayoutDiag> = RefCell::new(LayoutDiag::default());
}

/// Record one death-prune's occupancy. `rebuilt_filter` says whether this
/// prune rebuilt the address filter from its survivors, or found the tables
/// too full for a 4,096-bit sketch to discriminate and saturated it instead.
pub fn layout_note_prune(
    typed_len: usize,
    masks_len: usize,
    filter_bits_set: usize,
    filter_bits_total: usize,
    rebuilt_filter: bool,
    useful_keys: usize,
) {
    LAYOUT_DIAG.with(|d| {
        let mut d = d.borrow_mut();
        d.prunes += 1;
        d.typed_len = typed_len;
        d.masks_len = masks_len;
        d.typed_max = d.typed_max.max(typed_len);
        d.masks_max = d.masks_max.max(masks_len);
        d.filter_bits_set = filter_bits_set;
        d.filter_bits_total = filter_bits_total;
        d.filter_bits_set_max = d.filter_bits_set_max.max(filter_bits_set);
        d.useful_keys = useful_keys;
        if rebuilt_filter {
            d.rebuilt += 1;
            d.rebuilt_keys += (typed_len + masks_len) as u64;
        } else {
            d.outgrown += 1;
        }
        let text = d.render();
        if let Some(sink) = layout_sink() {
            write_sink(sink, &text);
        }
    });
}

impl LayoutDiag {
    fn render(&self) -> String {
        use std::fmt::Write as _;
        let mut out = String::with_capacity(512);
        let pct = |n: usize, d: usize| {
            if d == 0 {
                0.0
            } else {
                100.0 * n as f64 / d as f64
            }
        };
        let keys = self.typed_len + self.masks_len;
        let _ = writeln!(
            out,
            "[layout-diag] prunes={} keys_now={} (typed={} masks={}) keys_max={} \
             (typed={} masks={})",
            self.prunes,
            keys,
            self.typed_len,
            self.masks_len,
            self.typed_max + self.masks_max,
            self.typed_max,
            self.masks_max
        );
        let _ = writeln!(
            out,
            "  filter: {}/{} bits set ({:.1} %), max {}/{} ({:.1} %); selective up to \
             {} keys, so this table is {}",
            self.filter_bits_set,
            self.filter_bits_total,
            pct(self.filter_bits_set, self.filter_bits_total),
            self.filter_bits_set_max,
            self.filter_bits_total,
            pct(self.filter_bits_set_max, self.filter_bits_total),
            self.useful_keys,
            if keys > self.useful_keys {
                "OUTGROWN it -- every probe answers `may hold`"
            } else {
                "within it"
            }
        );
        let _ = writeln!(
            out,
            "  filter rebuilds={} over {} keys walked; outgrown-and-skipped={}",
            self.rebuilt, self.rebuilt_keys, self.outgrown
        );
        out
    }
}

/// Is the IC-miss instrument armed? One relaxed load once initialised.
#[inline]
pub fn ic_on() -> bool {
    if IC_SINK.get().is_none() {
        ic_sink();
    }
    IC_ON.load(Ordering::Relaxed)
}

/// Why `js_object_get_field_ic_miss` answered the way it did. The order is
/// the order of the handler's ladder.
#[derive(Clone, Copy, Debug)]
#[repr(u8)]
pub enum IcMissReason {
    /// SSO (short-string) receiver — never cacheable.
    SsoReceiver = 0,
    /// Null receiver or key.
    NullArgs,
    /// Proxy id band.
    Proxy,
    /// Async-resource handle property.
    AsyncResource,
    /// Array-subclass elements store answered.
    SubclassElements,
    /// `.length` on a dense array / object-backed Array subclass.
    ArrayLength,
    /// Closure receiver (function object with expando props).
    ClosureProp,
    /// Registered Buffer receiver.
    Buffer,
    /// Registered typed array receiver.
    TypedArray,
    /// Small native handle (timers, text codecs, handle dispatch).
    SmallHandle,
    /// Receiver is a heap pointer but not `GC_TYPE_OBJECT` (array, string,
    /// map, set, promise, ...): the IC can never serve it.
    NonObjectGcType,
    /// `GC_TYPE_OBJECT` whose shape kind is not `Ordinary` (dictionary /
    /// exotic) or whose header is forwarded.
    ObjectIrregular,
    /// Ordinary object with no keys array yet.
    ObjectNoKeys,
    /// Own inline field found: primed the MRU entry and returned.
    OwnInlinePrimed,
    /// Own overflow field found: primed with the overflow bit.
    OwnOverflowPrimed,
    /// Own field found but the receiver carries descriptors (or the overflow
    /// value was not readable) — fell through to the generic read.
    OwnDescriptorFallthrough,
    /// Key is not an own property of the receiver: inherited (prototype
    /// method / accessor) or absent. The generic read walks the chain.
    NotOwn,
}

pub const IC_MISS_REASONS: usize = 17;

const IC_REASON_NAMES: [&str; IC_MISS_REASONS] = [
    "sso_receiver",
    "null_args",
    "proxy",
    "async_resource",
    "subclass_elements",
    "array_length",
    "closure_prop",
    "buffer",
    "typed_array",
    "small_handle",
    "non_object_gc_type",
    "object_irregular",
    "object_no_keys",
    "own_inline_primed",
    "own_overflow_primed",
    "own_descriptor_fallthrough",
    "not_own",
];

#[derive(Default)]
struct SiteStat {
    key: String,
    misses: u64,
    by_reason: [u32; IC_MISS_REASONS],
    /// Primes at this site whose token equals the one the site's MRU entry
    /// ALREADY held. The cache was written with this shape, the next read of
    /// the same shape came back to the miss handler anyway, and the handler
    /// wrote the identical value again: the prime is not sticking. See
    /// [`IcDiag::prime_same_token`].
    prime_same_token: u64,
    /// Primes whose token differs from the MRU entry's — the site really is
    /// seeing more than one receiver shape (polymorphism / megamorphism).
    prime_new_token: u64,
    /// Primes taken while the site's `PIC_WAY_STATE` was < 0, i.e. the ways
    /// are latched off because the rotation was wider than they hold.
    prime_while_megamorphic: u64,
    /// Primes taken while the site had no way populated yet (state == 0).
    prime_while_fresh: u64,
    /// Primes taken while the site's ways were populated and live (state > 0).
    prime_while_armed: u64,
    /// Primes whose token was ALREADY sitting in one of the site's ways. The
    /// cache held the right answer in a way and the read came back to the miss
    /// handler regardless. See [`IcDiag::prime_in_ways`].
    prime_in_ways: u64,
}

#[derive(Default)]
pub struct IcDiag {
    started: Option<Instant>,
    last_dump: Option<Instant>,
    events: u32,
    pub misses: u64,
    by_reason: [u64; IC_MISS_REASONS],
    sites: HashMap<usize, SiteStat>,
    /// THE SPLIT. A site classified `own_inline_primed` misses, finds the
    /// property as an own inline slot, and primes the cache — and then misses
    /// again. Two mutually exclusive explanations, and the fix differs:
    ///
    /// * `prime_new_token` — the receiver's shape really did change between
    ///   the two reads. The site is polymorphic and the ways are (or should
    ///   be) doing their job; a fix would widen or re-tier them.
    /// * `prime_same_token` — the site re-primed the shape it already had.
    ///   The cache holds the right answer and the emitted hit path did not
    ///   use it, or something invalidated it in between. That is a
    ///   priming/invalidation or IC-layout bug, not polymorphism.
    ///
    /// Measured on the claude-code TUI, shape identity is NOT the explanation
    /// for the bulk of the misses (`{value, done}` iterator results share one
    /// keys array since #7564 and their read sites still miss ~178k times per
    /// turn), which is what this counter exists to settle.
    pub prime_same_token: u64,
    pub prime_new_token: u64,
    pub prime_while_megamorphic: u64,
    pub prime_while_fresh: u64,
    pub prime_while_armed: u64,
    /// The decisive counter for the `new_token` half. `prime_same_token` only
    /// compares against the MRU entry (word 0), so a site rotating k <= 5
    /// shapes reports `new_token` on every prime even when the ways are doing
    /// exactly what they were built for. This counts the primes whose token was
    /// found in one of the four ways at prime time: the polymorphic cache
    /// ALREADY held that shape's slot, and the emitted hit path still fell
    /// through to the miss handler.
    ///
    /// So the three-way split of every prime is:
    /// * `same_token` — re-primed the MRU shape (priming/invalidation),
    /// * `new_token` + `in_ways` — the ways held it and were not consulted
    ///   (emitted gate / IC layout, i.e. codegen),
    /// * `new_token` + not `in_ways` — a shape neither the MRU entry nor the
    ///   ways had (genuine polymorphism, or a first sighting).
    pub prime_in_ways: u64,
}

crate::perry_thread_local! {
    static IC_DIAG: RefCell<IcDiag> = RefCell::new(IcDiag::default());
}

/// Record one `pic_prime_get`, splitting it by whether the token the site is
/// being primed with is one it already held — in the MRU entry (`same`) or in
/// one of the ways (`in_ways`). See [`IcDiag::prime_same_token`] and
/// [`IcDiag::prime_in_ways`].
///
/// Diagnostic only: called from `pic_prime_get` behind [`ic_on`], and every
/// value it reads (`prev_tok`, `token`, `state`, the ways) is one the caller
/// already has in a register or in the cache line it has just touched.
pub fn ic_note_prime(site: usize, prev_tok: i64, token: i64, state: i64, in_ways: bool) {
    IC_DIAG.with(|d| {
        let mut d = d.borrow_mut();
        if d.started.is_none() {
            d.started = Some(Instant::now());
            d.last_dump = d.started;
        }
        let same = prev_tok != 0 && prev_tok == token;
        if same {
            d.prime_same_token += 1;
        } else {
            d.prime_new_token += 1;
        }
        if in_ways {
            d.prime_in_ways += 1;
        }
        match state.cmp(&0) {
            std::cmp::Ordering::Less => d.prime_while_megamorphic += 1,
            std::cmp::Ordering::Equal => d.prime_while_fresh += 1,
            std::cmp::Ordering::Greater => d.prime_while_armed += 1,
        }
        let s = d.sites.entry(site).or_default();
        if same {
            s.prime_same_token += 1;
        } else {
            s.prime_new_token += 1;
        }
        if in_ways {
            s.prime_in_ways += 1;
        }
        match state.cmp(&0) {
            std::cmp::Ordering::Less => s.prime_while_megamorphic += 1,
            std::cmp::Ordering::Equal => s.prime_while_fresh += 1,
            std::cmp::Ordering::Greater => s.prime_while_armed += 1,
        }
    });
}

/// Record one IC miss. `site` is the per-site cache address (stable for the
/// process lifetime), `key` the property-name string bytes.
pub fn ic_note(site: usize, key: &[u8], reason: IcMissReason) {
    IC_DIAG.with(|d| {
        let mut d = d.borrow_mut();
        if d.started.is_none() {
            d.started = Some(Instant::now());
            d.last_dump = d.started;
        }
        d.misses += 1;
        d.by_reason[reason as usize] += 1;
        let s = d.sites.entry(site).or_default();
        if s.key.is_empty() {
            s.key = String::from_utf8_lossy(&key[..key.len().min(40)]).into_owned();
        }
        s.misses += 1;
        s.by_reason[reason as usize] += 1;
        d.events = d.events.wrapping_add(1);
        if d.events % TICK_EVERY == 0 {
            let due = d
                .last_dump
                .is_some_and(|t| t.elapsed().as_millis() >= DUMP_INTERVAL_MS);
            if due {
                d.last_dump = Some(Instant::now());
                if let Some(sink) = ic_sink() {
                    write_sink(sink, &d.render());
                }
            }
        }
    });
}

impl IcDiag {
    fn render(&self) -> String {
        use std::fmt::Write as _;
        let mut out = String::with_capacity(4096);
        let secs = self.started.map_or(0.0, |t| t.elapsed().as_secs_f64());
        let _ = write!(
            out,
            "[ic-diag] t={secs:.1}s misses={} sites={}",
            self.misses,
            self.sites.len()
        );
        for (i, name) in IC_REASON_NAMES.iter().enumerate() {
            if self.by_reason[i] != 0 {
                let _ = write!(out, " {name}={}", self.by_reason[i]);
            }
        }
        out.push('\n');
        // THE SPLIT: of every prime, how many re-primed the token the site
        // already held (the cache was right and was not used) versus a token
        // it had not seen (real polymorphism)?
        let primes = self.prime_same_token + self.prime_new_token;
        if primes != 0 {
            let pct = |n: u64| 100.0 * n as f64 / primes as f64;
            let _ = writeln!(
                out,
                "  primes={primes} same_token={} ({:.1} %) new_token={} ({:.1} %) \
                 in_ways={} ({:.1} %) | way_state: fresh={} armed={} megamorphic={}",
                self.prime_same_token,
                pct(self.prime_same_token),
                self.prime_new_token,
                pct(self.prime_new_token),
                self.prime_in_ways,
                pct(self.prime_in_ways),
                self.prime_while_fresh,
                self.prime_while_armed,
                self.prime_while_megamorphic
            );
        }
        let mut rows: Vec<&SiteStat> = self.sites.values().collect();
        rows.sort_by_key(|s| std::cmp::Reverse(s.misses));
        let _ = writeln!(
            out,
            "  misses   same/new/inways   fresh/armed/mega   key  reasons"
        );
        for s in rows.iter().take(40) {
            let mut reasons = String::new();
            let mut idx: Vec<usize> = (0..IC_MISS_REASONS)
                .filter(|&i| s.by_reason[i] != 0)
                .collect();
            idx.sort_by(|a, b| s.by_reason[*b].cmp(&s.by_reason[*a]));
            for i in idx.iter().take(3) {
                let _ = write!(reasons, " {}={}", IC_REASON_NAMES[*i], s.by_reason[*i]);
            }
            let _ = writeln!(
                out,
                "  {:6}  {:>8}/{}/{:<8}  {:>7}/{}/{:<8}  {:<24}{reasons}",
                s.misses,
                s.prime_same_token,
                s.prime_new_token,
                s.prime_in_ways,
                s.prime_while_fresh,
                s.prime_while_armed,
                s.prime_while_megamorphic,
                s.key
            );
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Enumeration and concatenation: EXECUTIONS per site, not bytes
// ---------------------------------------------------------------------------

static ENUM_SINK: OnceLock<Option<Sink>> = OnceLock::new();
static ENUM_ON: AtomicBool = AtomicBool::new(false);

fn enum_sink() -> &'static Option<Sink> {
    ENUM_SINK.get_or_init(|| {
        let sink = sink_from_env("PERRY_ENUM_DIAG");
        ENUM_ON.store(sink.is_some(), Ordering::Relaxed);
        sink
    })
}

/// Is the enumeration/concat execution counter armed?
#[inline]
pub fn enum_on() -> bool {
    if ENUM_SINK.get().is_none() {
        enum_sink();
    }
    ENUM_ON.load(Ordering::Relaxed)
}

/// What actually runs at the two allocation sites the byte-share ranking put
/// at 7.8 % (`for-in` key arrays) and 6.9 % (string concat).
///
/// The campaign's 19:30 correction is the reason this counts executions rather
/// than bytes: a category's byte share bounds the collection *schedule* it can
/// move, and nothing else. The cost that a small category can still carry is
/// whatever runs per allocation — here, for `for-in`, a heap `String` and a
/// SipHash insert for **every key at every prototype level**, allocated only to
/// be hashed for shadowing and dropped. Those `String`s are native-heap, so
/// they are not even in the 7.8 %.
#[derive(Default)]
pub struct EnumDiag {
    started: Option<Instant>,
    last_dump: Option<Instant>,
    events: u32,
    /// Entries to `js_for_in_keys_value`.
    pub for_in_calls: u64,
    /// `for-in` calls that took the non-pointer (primitive receiver) path.
    pub for_in_primitive: u64,
    /// Prototype levels walked, summed over all calls.
    pub for_in_levels: u64,
    /// Key arrays materialised by the walk: one `js_object_keys_value` plus one
    /// `js_object_get_own_property_names` per level.
    pub for_in_key_arrays: u64,
    /// Keys seen at any level — each one costs a `String` and a hash.
    pub for_in_keys_seen: u64,
    /// `String` allocations made by `key_string`.
    pub for_in_key_strings: u64,
    /// Bytes in those `String`s.
    pub for_in_key_string_bytes: u64,
    /// `seen.insert` calls (SipHash of the whole key each time).
    pub for_in_seen_inserts: u64,
    /// Of those, inserts that found the name already present — pure waste, the
    /// name was already shadowed.
    pub for_in_seen_dupes: u64,
    /// Keys actually emitted into the result array.
    pub for_in_keys_emitted: u64,
    /// Of those, keys emitted at prototype level >= 1 — the only ones for which
    /// the shadow set is load-bearing. If this is ~0, every `String` and every
    /// hash spent building that set was spent for nothing.
    pub for_in_keys_emitted_deep: u64,
    /// Times the deferred shadow set was actually materialised.
    pub for_in_shadow_built: u64,
    /// String concatenations, by entry point.
    pub concat_calls: u64,
    pub concat_site_calls: u64,
    pub concat_chain_calls: u64,
    /// Bytes produced by concatenation.
    pub concat_out_bytes: u64,
}

crate::perry_thread_local! {
    static ENUM_DIAG: RefCell<EnumDiag> = RefCell::new(EnumDiag::default());
}

/// Run `f` against this thread's enumeration counters, then maybe dump.
#[inline]
pub fn enum_with(f: impl FnOnce(&mut EnumDiag)) {
    ENUM_DIAG.with(|d| {
        let mut d = d.borrow_mut();
        if d.started.is_none() {
            d.started = Some(Instant::now());
            d.last_dump = d.started;
        }
        f(&mut d);
        d.events = d.events.wrapping_add(1);
        if d.events % TICK_EVERY == 0 {
            let due = d
                .last_dump
                .is_some_and(|t| t.elapsed().as_millis() >= DUMP_INTERVAL_MS);
            if due {
                d.last_dump = Some(Instant::now());
                if let Some(sink) = enum_sink() {
                    write_sink(sink, &d.render());
                }
            }
        }
    });
}

impl EnumDiag {
    fn render(&self) -> String {
        use std::fmt::Write as _;
        let mut out = String::with_capacity(1024);
        let per = |n: u64, d: u64| if d == 0 { 0.0 } else { n as f64 / d as f64 };
        let _ = writeln!(
            out,
            "[enum-diag] for_in calls={} (primitive={}) levels={} ({:.2}/call)",
            self.for_in_calls,
            self.for_in_primitive,
            self.for_in_levels,
            per(self.for_in_levels, self.for_in_calls)
        );
        let _ = writeln!(
            out,
            "  key arrays materialised={} ({:.2}/call)   keys seen={} ({:.1}/call)   emitted={} ({:.1}/call)",
            self.for_in_key_arrays,
            per(self.for_in_key_arrays, self.for_in_calls),
            self.for_in_keys_seen,
            per(self.for_in_keys_seen, self.for_in_calls),
            self.for_in_keys_emitted,
            per(self.for_in_keys_emitted, self.for_in_calls)
        );
        let _ = writeln!(
            out,
            "  PER-KEY WORK: String allocs={} ({:.2} MB) seen.insert={} of which duplicate={} ({:.1} %)",
            self.for_in_key_strings,
            self.for_in_key_string_bytes as f64 / (1024.0 * 1024.0),
            self.for_in_seen_inserts,
            self.for_in_seen_dupes,
            100.0 * per(self.for_in_seen_dupes, self.for_in_seen_inserts)
        );
        let _ = writeln!(
            out,
            "  emitted/String ratio = {:.3}  (1.0 would mean every String earned a key)",
            per(self.for_in_keys_emitted, self.for_in_key_strings)
        );
        let _ = writeln!(
            out,
            "  LOAD-BEARING: keys emitted at proto level >=1 = {} ({:.2} % of emitted); shadow set built {} times ({:.2}/call)",
            self.for_in_keys_emitted_deep,
            100.0 * per(self.for_in_keys_emitted_deep, self.for_in_keys_emitted),
            self.for_in_shadow_built,
            per(self.for_in_shadow_built, self.for_in_calls)
        );
        let _ = writeln!(
            out,
            "[enum-diag] concat calls={} site={} chain={} out_bytes={:.2} MB ({:.1} B/call)",
            self.concat_calls,
            self.concat_site_calls,
            self.concat_chain_calls,
            self.concat_out_bytes as f64 / (1024.0 * 1024.0),
            per(
                self.concat_out_bytes,
                self.concat_calls + self.concat_site_calls + self.concat_chain_calls
            )
        );
        out
    }
}

// ---------------------------------------------------------------------------
// `is_registered_buffer`: is the min/max window still rejecting?
// ---------------------------------------------------------------------------

use std::sync::atomic::{AtomicU64, AtomicUsize};

static BUFFER_SINK: OnceLock<Option<Sink>> = OnceLock::new();
static BUFFER_ON: AtomicBool = AtomicBool::new(false);

fn buffer_sink() -> &'static Option<Sink> {
    BUFFER_SINK.get_or_init(|| {
        let sink = sink_from_env("PERRY_BUFFER_DIAG");
        BUFFER_ON.store(sink.is_some(), Ordering::Relaxed);
        sink
    })
}

/// Is the buffer-probe instrument armed? One relaxed load once initialised.
#[inline]
pub fn buffer_on() -> bool {
    if BUFFER_SINK.get().is_none() {
        buffer_sink();
    }
    BUFFER_ON.load(Ordering::Relaxed)
}

// Plain relaxed atomics rather than the thread-local `RefCell` the other
// instruments use: this probe runs millions of times per turn, and a borrow
// per probe would dominate the thing being measured.
static BUF_PROBES: AtomicU64 = AtomicU64::new(0);
static BUF_ADMITS: AtomicU64 = AtomicU64::new(0);
static BUF_TRUE_POS: AtomicU64 = AtomicU64::new(0);
static BUF_ADDR_MIN: AtomicUsize = AtomicUsize::new(usize::MAX);
static BUF_ADDR_MAX: AtomicUsize = AtomicUsize::new(0);
static BUF_WIN_LO: AtomicUsize = AtomicUsize::new(usize::MAX);
static BUF_WIN_HI: AtomicUsize = AtomicUsize::new(0);
static BUF_REGS: AtomicU64 = AtomicU64::new(0);
static BUF_UNREGS: AtomicU64 = AtomicU64::new(0);
static BUF_LIVE_MAX: AtomicUsize = AtomicUsize::new(0);

/// One `is_registered_buffer` probe that got past the "ever registered" latch.
/// `admitted` is what the inline min/max window answered — the whole question,
/// because only an admitted address pays the out-of-line call.
#[inline]
pub fn buffer_note_probe(addr: usize, admitted: bool, window: Option<(usize, usize)>) {
    let n = BUF_PROBES.fetch_add(1, Ordering::Relaxed);
    if admitted {
        BUF_ADMITS.fetch_add(1, Ordering::Relaxed);
    }
    BUF_ADDR_MIN.fetch_min(addr, Ordering::Relaxed);
    BUF_ADDR_MAX.fetch_max(addr, Ordering::Relaxed);
    if let Some((lo, hi)) = window {
        BUF_WIN_LO.store(lo, Ordering::Relaxed);
        BUF_WIN_HI.store(hi, Ordering::Relaxed);
    }
    // Dump roughly every million probes; the rig SIGKILLs, so an exit hook
    // would never fire.
    if n & 0xF_FFFF == 0 {
        buffer_dump();
    }
}

/// The slow path found a real registered buffer.
#[inline]
pub fn buffer_note_true_positive() {
    BUF_TRUE_POS.fetch_add(1, Ordering::Relaxed);
}

/// One buffer registration, with the registry's size after it. Registrations
/// are what a Bloom filter would have to hold, and `RegistryAddrFilter` accrues
/// bits **per admission, not per live entry** — so for a high-churn set the
/// number that decides whether that structure can work is the CUMULATIVE
/// count, not the live one. Both are recorded.
pub fn buffer_note_registration(live_now: usize) {
    BUF_REGS.fetch_add(1, Ordering::Relaxed);
    BUF_LIVE_MAX.fetch_max(live_now, Ordering::Relaxed);
}

/// One buffer leaving the registry.
pub fn buffer_note_unregistration() {
    BUF_UNREGS.fetch_add(1, Ordering::Relaxed);
}

#[cold]
fn buffer_dump() {
    let probes = BUF_PROBES.load(Ordering::Relaxed);
    let admits = BUF_ADMITS.load(Ordering::Relaxed);
    let tp = BUF_TRUE_POS.load(Ordering::Relaxed);
    let amin = BUF_ADDR_MIN.load(Ordering::Relaxed);
    let amax = BUF_ADDR_MAX.load(Ordering::Relaxed);
    let wlo = BUF_WIN_LO.load(Ordering::Relaxed);
    let whi = BUF_WIN_HI.load(Ordering::Relaxed);
    let pct = |a: u64, b: u64| if b == 0 { 0.0 } else { 100.0 * a as f64 / b as f64 };
    let mb = |n: usize| n as f64 / (1024.0 * 1024.0);
    let win_span = whi.saturating_sub(wlo);
    let probe_span = amax.saturating_sub(amin);
    let mut out = String::with_capacity(768);
    use std::fmt::Write as _;
    let _ = writeln!(
        out,
        "[buffer-diag] probes={probes} admits={admits} ({:.2} %) rejected={} ({:.2} %) \
         true_positives={tp} ({:.6} % of admits)",
        pct(admits, probes),
        probes - admits,
        pct(probes - admits, probes),
        pct(tp, admits)
    );
    let _ = writeln!(
        out,
        "  window   [{wlo:#x}, {whi:#x}] span {:.1} MB",
        mb(win_span)
    );
    let _ = writeln!(
        out,
        "  probed   [{amin:#x}, {amax:#x}] span {:.1} MB  -- window covers {:.1} % of the probed range",
        mb(probe_span),
        if probe_span == 0 { 0.0 } else { 100.0 * win_span as f64 / probe_span as f64 }
    );
    let regs = BUF_REGS.load(Ordering::Relaxed);
    let unregs = BUF_UNREGS.load(Ordering::Relaxed);
    let live_max = BUF_LIVE_MAX.load(Ordering::Relaxed);
    // A 1,024-bit, 3-hash Bloom filter (`RegistryAddrFilter`) accrues bits per
    // ADMISSION and never clears them, so `regs` — not `live_max` — is what it
    // would have to hold. (1 - e^(-3n/1024))^3 at that n:
    let fp = |n: f64| {
        let x = 1.0 - (-3.0 * n / 1024.0).exp();
        100.0 * x * x * x
    };
    let _ = writeln!(
        out,
        "  registrations={regs} unregistrations={unregs} live_max={live_max}           => a 1024-bit/3-hash Bloom holding all admissions would be {:.1} % false-positive          (and {:.1} % if it could hold only the live set)",
        fp(regs as f64),
        fp(live_max as f64)
    );
    if let Some(sink) = buffer_sink() {
        write_sink(sink, &out);
    }
}
