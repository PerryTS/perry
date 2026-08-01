//! `--opt-report` (#6952): surface which values Perry could *not* statically
//! type, why, and whether the developer can do anything about it.
//!
//! Perry's speed comes from proving static types and selecting unboxed
//! representations (`docs/representation-selection-rfc.md`). When a proof
//! fails the value stays NaN-boxed and the fast paths silently do not fire.
//! Before this module the only way to find out *which* values failed was to
//! read LLVM IR — which is how #7034 discovered, by accident, that
//! `Ptr<Shape>` promotion is **zero** on the object-heavy workload that
//! motivates it. This is the representation-selection analogue of LLVM's
//! `-Rpass-missed` optimization remarks.
//!
//! ## Shape
//!
//! - **Off by default.** [`enabled`] is a `OnceLock<bool>` over
//!   `PERRY_OPT_REPORT`; every recording entry point early-returns when it is
//!   false, *before* any string is formatted or allocated. The CLI promotes
//!   `--opt-report` to that env var on the main thread before rayon spawns
//!   the module workers, exactly like `--trace llvm` promotes `PERRY_SAVE_LL`.
//! - **Observational only.** Nothing in this module is read by codegen. The
//!   collectors record *in addition to*, never *instead of*, the `continue`
//!   that denied the candidate — the returned fact sets are bit-identical
//!   with the report on and off, which the CLI's byte-identical-object test
//!   asserts.
//! - **Attribution.** The representation collectors run on a bare `&[Stmt]`
//!   with no idea which function they belong to, so each codegen caller
//!   brackets its `collect_native_region_fact_graph` call with [`enter`],
//!   which pushes a thread-local (module, function, region-kind) scope. Sites
//!   that already hold an `FnCtx` (`expr/slot_rep.rs`) pass the names
//!   explicitly instead.
//! - **Sink.** A process-global `Mutex<Vec<Entry>>`. Module codegen is
//!   in-process on rayon workers, so no on-disk artifact hand-off is needed
//!   (unlike `--explain-lowering`, which predates this and re-reads JSON
//!   sidecars). The CLI drains it with [`take_entries`] after codegen.
//!
//! ## What it can and cannot see
//!
//! v1 reports **function + variable name**, because HIR carries names but not
//! source spans (`Stmt::Let` is `{id, name, ty, mutable, init}`). A
//! `LocalId -> Span` side-table populated during AST→HIR lowering would add
//! `file:line` and source snippets; that is tracked separately and is
//! strictly additive to this output.

mod callbacks;
mod render;

pub(crate) use callbacks::scan_module;
pub use render::{render_json, render_text};

use std::cell::RefCell;
use std::sync::{Mutex, OnceLock};

/// `PERRY_OPT_REPORT` gate. Off unless the value is one of `1` / `text` /
/// `json` (`0`, `off`, `false`, empty, and unset are all off).
///
/// Read exactly once per process. The CLI sets the var single-threaded before
/// module codegen spawns, so the `OnceLock` can never latch a stale `false`.
pub fn enabled() -> bool {
    #[cfg(test)]
    if test_support::forced() {
        return true;
    }
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        matches!(
            std::env::var("PERRY_OPT_REPORT").as_deref(),
            Ok("1") | Ok("text") | Ok("json")
        )
    })
}

/// Turning the report on in a unit test cannot go through the env var: the
/// gate is a `OnceLock`, and cargo runs the crate's tests in one process, so
/// whichever test read it first would latch the answer for all of them.
/// Instead tests take a lock and flip an atomic, which also serialises them
/// against the process-global sink.
#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Mutex, MutexGuard, OnceLock};

    static FORCED: AtomicBool = AtomicBool::new(false);
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    pub(crate) fn forced() -> bool {
        FORCED.load(Ordering::Relaxed)
    }

    /// Enable the report and drain any leftover entries. Restores the gate on
    /// drop, so a panicking test cannot leave it on for its neighbours.
    pub(crate) struct Session {
        _guard: MutexGuard<'static, ()>,
    }

    impl Session {
        pub(crate) fn start() -> Self {
            let guard = LOCK
                .get_or_init(|| Mutex::new(()))
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            FORCED.store(true, Ordering::Relaxed);
            let _ = super::take_entries();
            // The drain above may itself have collapsed leftovers; zero the
            // counter so a hand-built render test never reads a neighbour's.
            super::MASKED_BY_DEDUP.store(0, Ordering::Relaxed);
            Session { _guard: guard }
        }

        /// Take the serialising lock WITHOUT enabling the report, so an
        /// "off means nothing is recorded" test cannot race an enabled one.
        pub(crate) fn start_disabled() -> Self {
            let guard = LOCK
                .get_or_init(|| Mutex::new(()))
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            FORCED.store(false, Ordering::Relaxed);
            let _ = super::take_entries();
            super::MASKED_BY_DEDUP.store(0, Ordering::Relaxed);
            Session { _guard: guard }
        }

        pub(crate) fn entries(&self) -> Vec<super::Entry> {
            super::take_entries()
        }
    }

    impl Drop for Session {
        fn drop(&mut self) {
            let _ = super::take_entries();
            FORCED.store(false, Ordering::Relaxed);
        }
    }
}

/// Which lowering region a value lives in. Doubles as the honest half of the
/// hotness signal: a `Closure` body has no loop of its own but is invoked once
/// per element when it is an iterating builtin's callback (see
/// [`Entry::invoked_per_element`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RegionKind {
    Function,
    Method,
    Closure,
    ModuleInit,
}

impl RegionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            RegionKind::Function => "function",
            RegionKind::Method => "method",
            RegionKind::Closure => "closure",
            RegionKind::ModuleInit => "module-init",
        }
    }
}

/// Where the value sits. v1 proves locals; `Param`/`Return`/`Field` exist so
/// the schema does not change when the collectors reach those positions
/// (RFC row 1 / #7034 §1, §3, §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Position {
    Local,
    Param,
    Return,
    Field,
    /// An allocation site that is not bound to a local at all — the
    /// `.map(x => ({...}))` idiom. Provenance (rule 1) can never see it.
    AllocSite,
    /// A whole-module fact (e.g. the §5.2 barrier kill).
    Module,
}

impl Position {
    pub fn as_str(self) -> &'static str {
        match self {
            Position::Local => "local",
            Position::Param => "param",
            Position::Return => "return",
            Position::Field => "field",
            Position::AllocSite => "alloc-site",
            Position::Module => "module",
        }
    }
}

/// Which representation analysis produced this entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Analysis {
    /// `collectors/ptr_shape.rs` — shape-proven object locals (RFC Phase 3b).
    PtrShape,
    /// `collectors/ptr_numarray.rs` — `number[]` locals (RFC Phase 4a.3).
    ///
    /// The explicit rename keeps the serialized spelling identical to
    /// [`Analysis::as_str`]. Without it serde's kebab-case derive spells this
    /// variant `ptr-num-array` in `Entry::analysis` while the summary row says
    /// `ptr-numarray`, so one analysis appears under two names in a single
    /// JSON document and a consumer keying on either sees half the data.
    #[serde(rename = "ptr-numarray")]
    PtrNumArray,
    /// `expr/slot_rep.rs` — canonical i32/u32/Str locals (RFC Phase 1 / 3a).
    CanonicalSlot,
    /// `collectors/int_valued_ta_locals.rs` — locals held native-i32 despite a
    /// possibly-out-of-bounds int typed-array read (RFC Phase 2, wrap-i32).
    IntValuedTa,
    /// `codegen/typed_abi.rs` — specialized-ABI / typed-clone entries
    /// (RFC Phase 2).
    SpecAbi,
}

impl Analysis {
    /// Every representation analysis that records into this report, in a
    /// stable order.
    ///
    /// The JSON summary and the promotion census (#7106) enumerate *this*
    /// list rather than the analyses that happen to appear in a build's
    /// entries. An analysis with no entries at all must render as an explicit
    /// `0`, never as an absent key: "missing" and "zero" look identical to a
    /// consumer, and the entire point of this report is that zero is visible.
    pub const ALL: [Analysis; 5] = [
        Analysis::PtrShape,
        Analysis::PtrNumArray,
        Analysis::CanonicalSlot,
        Analysis::IntValuedTa,
        Analysis::SpecAbi,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Analysis::PtrShape => "ptr-shape",
            Analysis::PtrNumArray => "ptr-numarray",
            Analysis::CanonicalSlot => "canonical-slot",
            Analysis::IntValuedTa => "int-valued-ta",
            Analysis::SpecAbi => "spec-abi",
        }
    }

    /// The representation this analysis would have selected, for headline
    /// lines like "Ptr<Shape>: 0 of 4 candidates promoted".
    pub fn target_rep(self) -> &'static str {
        match self {
            Analysis::PtrShape => "Ptr<Shape>",
            Analysis::PtrNumArray => "Ptr<NumArray>",
            Analysis::CanonicalSlot => "I32/U32/Str",
            Analysis::IntValuedTa => "IntValued",
            Analysis::SpecAbi => "specialized ABI",
        }
    }

    /// Whether codegen records CONSUMPTION for this analysis — i.e. whether a
    /// `consumed: 0` on its row means "nothing was applied" or merely "nobody
    /// measured".
    ///
    /// The distinction has to be a property of the analysis, not something a
    /// reader infers from the numbers. Without it the text report cannot print
    /// a consumption line for an analysis whose promotions were ALL wasted
    /// (`selected > 0, consumed == 0`) — the worst case this outcome exists to
    /// expose — because a printed zero would be indistinguishable from the
    /// false zero it would have to print for an uninstrumented analysis.
    ///
    /// The census keeps the same table (`CONSUMPTION_INSTRUMENTED`) and
    /// cross-checks it against this flag, so the two cannot drift.
    pub fn records_consumption(self) -> bool {
        match self {
            Analysis::PtrShape => true,
            // Canonical i32/u32 MOVE the storage, so consumption is structural
            // rather than a separate event. Canonical `Str` does not move
            // storage and genuinely can be selected-and-unconsumed, but its
            // consumers are the string-op lowerings and are not instrumented.
            Analysis::PtrNumArray
            | Analysis::CanonicalSlot
            | Analysis::IntValuedTa
            | Analysis::SpecAbi => false,
        }
    }

    /// The file whose rules produced the denial, cited in the report so the
    /// rule numbers are checkable against source.
    pub fn rule_source(self) -> &'static str {
        match self {
            Analysis::PtrShape => "collectors/ptr_shape.rs",
            Analysis::PtrNumArray => "collectors/ptr_numarray.rs",
            Analysis::CanonicalSlot => "expr/slot_rep.rs",
            Analysis::IntValuedTa => "collectors/int_valued_ta_locals.rs",
            Analysis::SpecAbi => "codegen/typed_abi.rs",
        }
    }
}

/// Actionability tier — what makes this a tool rather than a wall of text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tier {
    /// The developer can change the source: stop reassigning, don't capture
    /// it in a closure, keep the record contained.
    Fixable,
    /// Genuinely polymorphic at run time — correctly boxed, no action. Said
    /// explicitly so nobody chases it.
    InherentlyPolymorphic,
    /// Perry's limitation, not the program's. This tier doubles as a roadmap
    /// generator: `issue` names the tracking issue where one exists.
    CompilerLimitation,
    /// The value is denied *at this position* but a different, already-shipped
    /// mechanism carries its shape onward, so it is **not** a missed
    /// opportunity and must not be counted as one (#7170 R0).
    ///
    /// This tier exists because the other three all answer "who should fix
    /// it?", and for these rows the answer is "nobody — it is already fixed
    /// elsewhere". Filing them under `CompilerLimitation` (which is where a
    /// rule-1 alloc-site denial lands by default) put them in the roadmap
    /// bucket a scheduler reads, and #7152/#7170 were both scheduled off a
    /// number inflated by exactly that.
    ///
    /// The explicit rename keeps the serialized spelling identical to
    /// [`Tier::as_str`], for the reason `Analysis::PtrNumArray` documents:
    /// serde's kebab-case derive would spell this `served` while every
    /// heading and text row says `served-elsewhere`, so one tier would appear
    /// under two names in a single document and a consumer keying on either
    /// would see half the rows.
    #[serde(rename = "served-elsewhere")]
    Served,
}

impl Tier {
    /// Every tier, in report order. The text renderer enumerates *this* rather
    /// than a hand-written list, so a new tier cannot be silently unrendered.
    pub const ALL: [Tier; 4] = [
        Tier::Fixable,
        Tier::CompilerLimitation,
        Tier::Served,
        Tier::InherentlyPolymorphic,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Tier::Fixable => "fixable",
            Tier::InherentlyPolymorphic => "inherently-polymorphic",
            Tier::CompilerLimitation => "compiler-limitation",
            Tier::Served => "served-elsewhere",
        }
    }

    pub fn heading(self) -> &'static str {
        match self {
            Tier::Fixable => "Fixable in your source",
            Tier::InherentlyPolymorphic => "Inherently polymorphic (correctly boxed — no action)",
            Tier::CompilerLimitation => "Perry limitation (not your code)",
            Tier::Served => "Already served by another mechanism (not a missed opportunity)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Outcome {
    /// The proof succeeded — the value got an unboxed representation.
    ///
    /// **Selection is not application.** A `Selected` entry says an analysis
    /// proved the value; it says nothing about whether codegen went on to emit
    /// anything different for it. See [`Outcome::Consumed`].
    Selected,
    /// The proof failed — the value stays `Boxed`.
    Denied,
    /// Codegen actually *applied* a selected proof: it emitted the
    /// representation-specific, guard-free form at a real access site.
    ///
    /// This is the outcome that corresponds to emitted bytes. #7107 found by
    /// reading IR that `batch.ts` reports two `Ptr<Shape>` promotions while one
    /// of them (`totals`) keeps the guarded diamond at every access site — the
    /// entire binary saving came from the other. Counting `select()` calls
    /// cannot see that; counting consumption can.
    Consumed,
    /// Codegen reached a selected value and **dropped the proof**, emitting the
    /// same code it would have emitted without the analysis. `rule` names the
    /// mechanism; a selected value that is never consumed and never explicitly
    /// dropped simply had no access site to apply the proof at.
    Unconsumed,
}

/// One reported value.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Entry {
    pub module: String,
    pub function: String,
    pub region: RegionKind,
    pub position: Position,
    /// Source-level binding name. HIR keeps names through lowering; a value
    /// with no name (an allocation site that never bound a local) gets a
    /// synthetic description like `new Row(...)`.
    pub name: String,
    pub local_id: Option<u32>,
    pub analysis: Analysis,
    pub outcome: Outcome,
    /// The representation actually selected — `Ptr<Shape>`, `I32`, `Boxed`, …
    pub rep: String,
    /// The rule that denied it, in the collector's own numbering, e.g.
    /// `rule 2 (containment)`. `None` on a win.
    pub rule: Option<String>,
    /// Human-readable expansion of `rule`.
    pub reason: Option<String>,
    pub tier: Option<Tier>,
    /// Tracking issue for a `CompilerLimitation`, e.g. `#6952`.
    pub issue: Option<String>,
    /// Loop-nesting depth of the enclosing statement. **A proxy, not a
    /// profile** — see [`Entry::invoked_per_element`].
    pub loop_depth: u32,
    /// Set when this value's region is the callback of an iterating builtin
    /// (`Array.prototype.map`, `sort`, `reduce`, …). Such bodies have
    /// `loop_depth == 0` but run once per element, so loop depth alone
    /// ranks them wrong — #7034 §8 found 208 of 247 guard sites in exactly
    /// this position. Reported as its own column; never folded into
    /// `loop_depth`.
    pub invoked_per_element: Option<String>,
    /// Extra collector-specific context (class name, offending use site).
    pub detail: Option<String>,
    /// Byte offset in the module source, when the HIR node happens to carry
    /// one. Only `Expr::New` does today (#5253, captured for constructor
    /// TypeErrors), so this is populated for allocation sites and `None` for
    /// ordinary locals — HIR drops positions at lowering.
    pub byte_offset: Option<u32>,
    /// For [`Outcome::Consumed`]: which codegen lowering applied the proof
    /// (`ptr_shape_set`, `ptr_shape_update`, …). `None` for every other
    /// outcome.
    ///
    /// A first-class field rather than prose inside `detail`, because the
    /// census gates on it: each recorder must fire somewhere in the corpus, or
    /// it is a site nobody has ever watched work. Two of the six were in
    /// exactly that state when this field was added.
    pub site: Option<String>,
    /// For a [`Position::AllocSite`] entry: the syntactic position the
    /// allocation sits in (`return`, `constructor argument`, `object literal
    /// property value`, …).
    ///
    /// A first-class field rather than prose inside `detail`, for the same
    /// reason `site` is one: this is the bucket a scheduler counts. #7170 §5.1
    /// found the `constructor argument` label conflating genuine `new C(arg)`
    /// arguments (2.0% of sites) with the property values of anonymous-shape
    /// object literals (24.5%) — two different opportunities needing two
    /// different fixes, merged into one number by the label alone.
    pub alloc_context: Option<String>,
    /// For a [`Position::AllocSite`] entry: the site's index in its region's
    /// deterministic report walk.
    ///
    /// **A de-duplication discriminant, not a source location.** Anonymous
    /// object literals all render as `object literal { ... }` and all carry
    /// `byte_offset: 0` (`perry-hir/src/lower/expr_object.rs` never populates
    /// one for them), so before this field every unbound literal in a function
    /// collapsed onto a single row in [`Entry::dedup_key`] — PR #7171 measured
    /// suppressing 379 scaffolding rows *revealing* 62 previously-masked real
    /// ones, and suppressing a single record moving its row between buckets
    /// while changing the total by zero. The walk is a plain `Vec` traversal
    /// with no map iteration, so a function lowered twice (a boxed entry plus
    /// a typed clone) produces the same ordinals and still de-duplicates.
    pub alloc_ordinal: Option<u32>,
}

impl Entry {
    /// Ranking key: per-element callbacks first, then loop depth, then a
    /// stable name order. Deliberately NOT "loop depth alone" — see
    /// `invoked_per_element`.
    fn rank(&self) -> (u8, std::cmp::Reverse<u32>, &str, &str) {
        let hot = u8::from(self.invoked_per_element.is_none());
        (
            hot,
            std::cmp::Reverse(self.loop_depth),
            self.function.as_str(),
            self.name.as_str(),
        )
    }

    /// Identity for de-duplication. A function can be lowered more than once
    /// (a boxed entry plus a typed clone), which would otherwise double-count
    /// every denial in it.
    ///
    /// Two elements below are load-bearing, for different pairs:
    ///
    /// - **`site`** separates a `Consumed` entry from the `Selected` entry for
    ///   the same value (only consumed entries carry one), and separates the
    ///   several sites that consumed one value from each other. It is what the
    ///   per-site coverage gate counts.
    /// - **`outcome`** separates an `Unconsumed` entry from a `Denied` one.
    ///   Both carry `rule: Some(..)` and neither carries a `site`, so with a
    ///   shared rule name every other element matches — and "the proof was made
    ///   and thrown away" versus "the proof was never made" are opposite claims
    ///   about the same binding.
    ///
    /// **`outcome` is defensive, and currently cannot be observed to matter.**
    /// Say so rather than implying coverage that does not exist: today the two
    /// rule vocabularies are disjoint, and more fundamentally the collision is
    /// unreachable, because a DENIED value was never selected while an
    /// UNCONSUMED one always was — no binding can produce both. Removing
    /// `outcome` from this key therefore turns no gate red, and the only thing
    /// exercising it is the unit test that constructs the collision by hand.
    /// It is kept because a dedup key should carry the full discriminant: the
    /// cost is one tuple element, and the failure mode if a future rule name
    /// collides is silent data loss in the report that feeds the census.
    /// - **`alloc_ordinal`** separates the several unbound allocation sites of
    ///   one function from each other. It is `None` for every other position,
    ///   so no other tally moves. See [`Entry::alloc_ordinal`] for why the
    ///   pre-existing key could not tell them apart, and
    ///   [`masked_by_dedup`] for what is still being collapsed.
    ///
    ///   `alloc_context` is deliberately **not** in this key. It is a function
    ///   of the site, so the ordinal already implies it, and a second
    ///   enforcement point that no sabotage can kill is how #7171 ended up
    ///   with four green holes in its first pass. One discriminant, one test.
    #[allow(clippy::type_complexity)]
    fn dedup_key(
        &self,
    ) -> (
        String,
        String,
        String,
        Position,
        Analysis,
        Outcome,
        Option<String>,
        Option<String>,
        Option<u32>,
    ) {
        (
            self.module.clone(),
            self.function.clone(),
            self.name.clone(),
            self.position,
            self.analysis,
            self.outcome,
            self.rule.clone(),
            // `site` participates for CONSUMED entries only. One value consumed
            // at three access sites is genuinely three facts worth reporting:
            // the census gates on per-site coverage, and collapsing them here
            // would both hide that and leave the census's own per-value
            // reduction untestable end-to-end.
            //
            // Every other outcome keeps the pre-existing identity, so denial
            // and selection tallies (and therefore the baseline's `candidates`
            // column) are unchanged.
            self.site.clone(),
            self.alloc_ordinal,
        )
    }
}

// ── Attribution scope ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct Scope {
    module: String,
    function: String,
    region: RegionKind,
    invoked_per_element: Option<String>,
    /// #7170 R0: this region is a function that carries a **return-shape fact**
    /// (`collectors/ptr_shape_returns.rs`, #7107), so its `return new C(...)`
    /// sites already feed an existing mechanism.
    ///
    /// Set by exactly one caller — [`enter_function_region`], from
    /// `codegen/function.rs`, which is the only place that holds both the
    /// `FuncId` and `ModuleDispatchFacts`. Every other region (method, closure,
    /// module-init) leaves it `false`, and that is CORRECT rather than
    /// conservative: `collect_return_shape_functions` issues facts only for
    /// `hir.functions` entries, and the caller-side seed
    /// (`find_return_shape_candidates`) only fires on a bare `Expr::FuncRef`
    /// callee — which a closure call never is. #7170 §6 is precisely the
    /// measurement that closures are *not* served.
    return_shape_producer: bool,
}

thread_local! {
    static SCOPE: RefCell<Option<Scope>> = const { RefCell::new(None) };
}

/// RAII scope guard. Restores the previous scope on drop so a nested region
/// (a constructor inlined into a function body, say) cannot leak attribution
/// into its parent.
pub struct ScopeGuard {
    previous: Option<Scope>,
    /// `false` when the report is off — drop then does nothing at all.
    active: bool,
}

impl Drop for ScopeGuard {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let previous = self.previous.take();
        SCOPE.with(|s| *s.borrow_mut() = previous);
    }
}

/// Bracket a lowering region, inheriting the module name from the ambient
/// module scope opened by [`enter_module`] in `compile_module`.
///
/// Region callers deliberately do NOT name their own module: `hir.name` and
/// `strings.module_prefix()` are different spellings of the same module
/// (`batch.ts` vs `batch_ts`), and having both in one report made it look
/// like two modules.
pub(crate) fn enter_region(function: &str, region: RegionKind) -> ScopeGuard {
    if !enabled() {
        return ScopeGuard {
            previous: None,
            active: false,
        };
    }
    let module = current_module();
    enter(&module, function, region)
}

/// [`enter_region`] for a top-level function body, additionally recording
/// whether this function carries a **return-shape fact** (#7107).
///
/// Report-only: the flag is read by nothing but
/// [`region_is_return_shape_producer`], which only the `Ptr<Shape>` alloc-site
/// recorder consults. Like every other entry point here it is inert — and
/// allocation-free — when the report is off.
pub(crate) fn enter_function_region(function: &str, return_shape_producer: bool) -> ScopeGuard {
    if !enabled() {
        return ScopeGuard {
            previous: None,
            active: false,
        };
    }
    let scope = Scope {
        module: current_module(),
        function: function.to_string(),
        region: RegionKind::Function,
        invoked_per_element: None,
        return_shape_producer,
    };
    let previous = SCOPE.with(|s| s.borrow_mut().replace(scope));
    ScopeGuard {
        previous,
        active: true,
    }
}

/// Whether the region currently being lowered carries a return-shape fact.
///
/// `false` with no scope at all, which is the safe direction: an unattributed
/// site is reported as an ordinary rule-1 denial rather than silently written
/// off as served.
pub(crate) fn region_is_return_shape_producer() -> bool {
    SCOPE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|sc| sc.return_shape_producer)
            .unwrap_or(false)
    })
}

fn current_module() -> String {
    SCOPE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|sc| sc.module.clone())
            .unwrap_or_else(|| String::from("<unknown>"))
    })
}

/// Kind of the lowering region currently being emitted, for the one
/// [`select_explicit`] caller that knows its function name but not its region
/// kind (`slot_rep::note_canonical_local`, which holds only an `FnCtx`).
///
/// Same rule [`scope_or_unknown`] states for consumption: the region is taken
/// from the ambient scope rather than re-derived from a function name, which is
/// what makes `region == module-init` trustworthy. Before #7109 the caller
/// passed a hard-coded `Function`, which was invisible only because module-init
/// and closure bodies could not select a canonical rep at all.
pub(crate) fn current_region() -> RegionKind {
    SCOPE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|sc| sc.region)
            .unwrap_or(RegionKind::Function)
    })
}

/// Bracket a lowering region so collector denials inside it are attributed to
/// `function`. No-op (and allocation-free) when the report is off.
pub(crate) fn enter(module: &str, function: &str, region: RegionKind) -> ScopeGuard {
    if !enabled() {
        return ScopeGuard {
            previous: None,
            active: false,
        };
    }
    let scope = Scope {
        module: module.to_string(),
        function: function.to_string(),
        region,
        invoked_per_element: None,
        return_shape_producer: false,
    };
    let previous = SCOPE.with(|s| s.borrow_mut().replace(scope));
    ScopeGuard {
        previous,
        active: true,
    }
}

/// Like [`enter`], for a closure body. `func_id` resolves the per-element
/// callback role recorded by [`scan_module`] — the honest hotness column for
/// bodies that have no loop of their own (#7034 §8).
pub(crate) fn enter_closure(function: &str, func_id: u32) -> ScopeGuard {
    if !enabled() {
        return ScopeGuard {
            previous: None,
            active: false,
        };
    }
    let scope = Scope {
        module: current_module(),
        function: function.to_string(),
        region: RegionKind::Closure,
        invoked_per_element: per_element_role(Some(func_id)),
        // #7170 §6: a closure is never a return-shape producer — the caller-side
        // seed requires a bare `Expr::FuncRef` callee. This `false` is the
        // measurement, not a default.
        return_shape_producer: false,
    };
    let previous = SCOPE.with(|s| s.borrow_mut().replace(scope));
    ScopeGuard {
        previous,
        active: true,
    }
}

// ── Per-element callback registry ──────────────────────────────────────────

/// Closure `FuncId` -> the iterating builtin it is the callback of.
/// Populated once per module by [`scan_module`]; read when that closure's
/// region opens its scope. Keyed by id rather than symbol name because the
/// same source arrow can be lowered under several symbols (typed clones).
static PER_ELEMENT: OnceLock<Mutex<std::collections::HashMap<u32, &'static str>>> = OnceLock::new();

fn per_element_map() -> &'static Mutex<std::collections::HashMap<u32, &'static str>> {
    PER_ELEMENT.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

/// Record that closure `func_id` is the callback of `builtin`
/// (`Array.prototype.map`, …).
pub(crate) fn note_per_element_callback(func_id: u32, builtin: &'static str) {
    if !enabled() {
        return;
    }
    if let Ok(mut map) = per_element_map().lock() {
        map.insert(func_id, builtin);
    }
}

fn per_element_role(func_id: Option<u32>) -> Option<String> {
    let id = func_id?;
    per_element_map()
        .lock()
        .ok()
        .and_then(|m| m.get(&id).map(|s| (*s).to_string()))
}

// ── Sink ───────────────────────────────────────────────────────────────────

static SINK: OnceLock<Mutex<Vec<Entry>>> = OnceLock::new();

fn sink() -> &'static Mutex<Vec<Entry>> {
    SINK.get_or_init(|| Mutex::new(Vec::new()))
}

/// Push a fully-formed entry. Prefer [`deny`] / [`select`], which fill the
/// module/function from the active scope.
pub(crate) fn push(entry: Entry) {
    if !enabled() {
        return;
    }
    if let Ok(mut v) = sink().lock() {
        v.push(entry);
    }
}

/// Everything a denial needs beyond the ambient scope.
pub(crate) struct Denial<'a> {
    pub position: Position,
    pub name: &'a str,
    pub local_id: Option<u32>,
    pub analysis: Analysis,
    pub rule: &'a str,
    pub reason: &'a str,
    pub tier: Tier,
    pub issue: Option<&'a str>,
    pub loop_depth: u32,
    pub detail: Option<String>,
    pub byte_offset: Option<u32>,
}

/// Record a value that stayed `Boxed`, attributed to the active scope.
///
/// Callers **must** gate the call on [`enabled`] when building `reason` /
/// `detail` costs anything: the arguments are evaluated before this function
/// can early-return.
pub(crate) fn deny(d: Denial<'_>) {
    if !enabled() {
        return;
    }
    let (module, function, region, per_element) = SCOPE.with(|s| match s.borrow().as_ref() {
        Some(sc) => (
            sc.module.clone(),
            sc.function.clone(),
            sc.region,
            sc.invoked_per_element.clone(),
        ),
        None => (
            String::from("<unknown>"),
            String::from("<unknown>"),
            RegionKind::Function,
            None,
        ),
    });
    push(Entry {
        module,
        function,
        region,
        position: d.position,
        name: d.name.to_string(),
        local_id: d.local_id,
        analysis: d.analysis,
        outcome: Outcome::Denied,
        rep: String::from("Boxed"),
        rule: Some(d.rule.to_string()),
        reason: Some(d.reason.to_string()),
        tier: Some(d.tier),
        issue: d.issue.map(str::to_string),
        loop_depth: d.loop_depth,
        invoked_per_element: per_element,
        detail: d.detail,
        byte_offset: d.byte_offset,
        site: None,
        alloc_context: None,
        alloc_ordinal: None,
    });
}

/// [`deny`] for a [`Position::AllocSite`] row, carrying the two fields that
/// make an unbound allocation identifiable: the syntactic position it sits in
/// and its ordinal in the region's report walk.
///
/// A separate entry point rather than two more fields on [`Denial`], because
/// every other denial site would then have to spell `None` twice for a pair of
/// fields only this one can populate.
pub(crate) fn deny_alloc(d: Denial<'_>, alloc_context: &str, alloc_ordinal: u32) {
    if !enabled() {
        return;
    }
    let (module, function, region, per_element) = SCOPE.with(|s| match s.borrow().as_ref() {
        Some(sc) => (
            sc.module.clone(),
            sc.function.clone(),
            sc.region,
            sc.invoked_per_element.clone(),
        ),
        None => (
            String::from("<unknown>"),
            String::from("<unknown>"),
            RegionKind::Function,
            None,
        ),
    });
    push(Entry {
        module,
        function,
        region,
        position: d.position,
        name: d.name.to_string(),
        local_id: d.local_id,
        analysis: d.analysis,
        outcome: Outcome::Denied,
        rep: String::from("Boxed"),
        rule: Some(d.rule.to_string()),
        reason: Some(d.reason.to_string()),
        tier: Some(d.tier),
        issue: d.issue.map(str::to_string),
        loop_depth: d.loop_depth,
        invoked_per_element: per_element,
        detail: d.detail,
        byte_offset: d.byte_offset,
        site: None,
        alloc_context: Some(alloc_context.to_string()),
        alloc_ordinal: Some(alloc_ordinal),
    });
}

/// Record a denial for a named function in the ambient scope's **module**,
/// for sites that know their function but do not run inside a lowering
/// region — the specialized-ABI entry decision, which is taken in
/// `compile_module`'s own loop rather than during body lowering.
pub(crate) fn deny_named(function: &str, region: RegionKind, d: Denial<'_>) {
    if !enabled() {
        return;
    }
    push(Entry {
        module: current_module(),
        function: function.to_string(),
        region,
        position: d.position,
        name: d.name.to_string(),
        local_id: d.local_id,
        analysis: d.analysis,
        outcome: Outcome::Denied,
        rep: String::from("Boxed"),
        rule: Some(d.rule.to_string()),
        reason: Some(d.reason.to_string()),
        tier: Some(d.tier),
        issue: d.issue.map(str::to_string),
        loop_depth: d.loop_depth,
        invoked_per_element: None,
        detail: d.detail,
        byte_offset: d.byte_offset,
        site: None,
        alloc_context: None,
        alloc_ordinal: None,
    });
}

/// Open a module-wide fallback scope. Region scopes nest inside it and
/// restore it on drop, so a site with no region of its own still knows which
/// module it is in.
pub(crate) fn enter_module(module: &str) -> ScopeGuard {
    enter(module, "<module>", RegionKind::ModuleInit)
}

/// Record a value that *did* get an unboxed representation, attributed to the
/// active scope. Reporting wins matters: a report that only nags is less
/// useful, and less trusted, than one that shows the ratio.
pub(crate) fn select(
    position: Position,
    name: &str,
    local_id: Option<u32>,
    analysis: Analysis,
    rep: &str,
    loop_depth: u32,
    detail: Option<String>,
) {
    if !enabled() {
        return;
    }
    let (module, function, region, per_element) = SCOPE.with(|s| match s.borrow().as_ref() {
        Some(sc) => (
            sc.module.clone(),
            sc.function.clone(),
            sc.region,
            sc.invoked_per_element.clone(),
        ),
        None => (
            String::from("<unknown>"),
            String::from("<unknown>"),
            RegionKind::Function,
            None,
        ),
    });
    push(Entry {
        module,
        function,
        region,
        position,
        name: name.to_string(),
        local_id,
        analysis,
        outcome: Outcome::Selected,
        rep: rep.to_string(),
        rule: None,
        reason: None,
        tier: None,
        issue: None,
        loop_depth,
        invoked_per_element: per_element,
        detail,
        byte_offset: None,
        site: None,
        alloc_context: None,
        alloc_ordinal: None,
    });
}

/// Record a win from a site that already knows its own function and module
/// (an `FnCtx` holder), bypassing the thread-local scope.
pub(crate) fn select_explicit(
    function: &str,
    region: RegionKind,
    position: Position,
    name: &str,
    local_id: Option<u32>,
    analysis: Analysis,
    rep: &str,
) {
    if !enabled() {
        return;
    }
    push(Entry {
        module: current_module(),
        function: function.to_string(),
        region,
        position,
        name: name.to_string(),
        local_id,
        analysis,
        outcome: Outcome::Selected,
        rep: rep.to_string(),
        rule: None,
        reason: None,
        tier: None,
        issue: None,
        loop_depth: 0,
        invoked_per_element: None,
        detail: None,
        byte_offset: None,
        site: None,
        alloc_context: None,
        alloc_ordinal: None,
    });
}

// ── Consumption (#7106 follow-up) ──────────────────────────────────────────
//
// `select()` records that an analysis PROVED a value. Whether codegen then
// emitted anything different for it is a separate question with a separate
// answer, and the two were conflated until #7107 read the IR by hand.
//
// Nothing here may be recorded from a `select()`-adjacent site. Both entry
// points below are called from the codegen sites that *commit* to (or *drop*)
// the representation-specific lowering — the same `if` whose taken branch is
// what makes the guard diamond disappear from the emitted module. That is what
// keeps the count checkable against IR instead of against another counter.

/// Module / function / region of the lowering region currently being emitted.
///
/// Consumption is recorded from inside body lowering, which always runs under
/// the region scope its caller opened (`enter_region` / `enter_closure` guards
/// are held across the whole `lower_*` call). Taking the region from the
/// ambient scope rather than re-deriving it from a function name is what makes
/// `region == module-init` trustworthy — and module-init is the context whose
/// promotions are the ones that go unconsumed.
fn scope_or_unknown() -> (String, String, RegionKind) {
    SCOPE.with(|s| match s.borrow().as_ref() {
        Some(sc) => (sc.module.clone(), sc.function.clone(), sc.region),
        None => (
            String::from("<unknown>"),
            String::from("<unknown>"),
            RegionKind::Function,
        ),
    })
}

/// Record that codegen applied a selected proof at a real access site.
///
/// `site` names the lowering that consumed it (`class_field_get`,
/// `class_method_call`, …) so a reader can find the emitted sequence.
pub(crate) fn consume(
    position: Position,
    name: &str,
    local_id: Option<u32>,
    analysis: Analysis,
    rep: &str,
    site: &str,
) {
    if !enabled() {
        return;
    }
    let (module, function, region) = scope_or_unknown();
    push(Entry {
        module,
        function,
        region,
        position,
        name: name.to_string(),
        local_id,
        analysis,
        outcome: Outcome::Consumed,
        rep: rep.to_string(),
        rule: None,
        reason: None,
        tier: None,
        issue: None,
        loop_depth: 0,
        invoked_per_element: None,
        detail: Some(format!("consumed at {site}")),
        byte_offset: None,
        site: Some(site.to_string()),
        alloc_context: None,
        alloc_ordinal: None,
    });
}

/// Everything an unconsumed record needs beyond the ambient scope.
pub(crate) struct Unconsumed<'a> {
    pub position: Position,
    pub name: &'a str,
    pub local_id: Option<u32>,
    pub analysis: Analysis,
    pub rep: &'a str,
    /// The mechanism that dropped the proof, e.g. `module_init_context`.
    pub rule: &'a str,
    pub reason: &'a str,
    pub tier: Tier,
    pub issue: Option<&'a str>,
    pub detail: Option<String>,
}

/// Record that codegen reached a selected value and dropped its proof.
///
/// The distinction from [`deny`] is the whole point: a denial says the analysis
/// refused to prove the value, so no representation was ever selected. This
/// says the analysis DID prove it, the report counted it as a win, and codegen
/// then emitted byte-for-byte what it would have emitted anyway.
pub(crate) fn unconsumed(u: Unconsumed<'_>) {
    if !enabled() {
        return;
    }
    let (module, function, region) = scope_or_unknown();
    push(Entry {
        module,
        function,
        region,
        position: u.position,
        name: u.name.to_string(),
        local_id: u.local_id,
        analysis: u.analysis,
        outcome: Outcome::Unconsumed,
        rep: u.rep.to_string(),
        rule: Some(u.rule.to_string()),
        reason: Some(u.reason.to_string()),
        tier: Some(u.tier),
        issue: u.issue.map(str::to_string),
        loop_depth: 0,
        invoked_per_element: None,
        detail: u.detail,
        byte_offset: None,
        site: None,
        alloc_context: None,
        alloc_ordinal: None,
    });
}

/// How many entries the most recent [`take_entries`] collapsed.
///
/// #7170 R0: de-duplication is not free information hygiene — it is a lossy
/// step between "what the compiler recorded" and "what a scheduler counts",
/// and PR #7171 measured it changing which bucket a row lands in while leaving
/// the total at zero. The count is therefore reported rather than left
/// implicit, so a reader can see how much of the population is still being
/// folded away instead of having to assume it is none.
///
/// Set (not accumulated) by each drain; `0` before the first one.
static MASKED_BY_DEDUP: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Entries collapsed by the most recent [`take_entries`]. See
/// [`MASKED_BY_DEDUP`].
pub fn masked_by_dedup() -> usize {
    MASKED_BY_DEDUP.load(std::sync::atomic::Ordering::Relaxed)
}

/// Drain every recorded entry, de-duplicated and ranked. Called once by the
/// CLI after module codegen finishes.
pub fn take_entries() -> Vec<Entry> {
    let mut entries = match sink().lock() {
        Ok(mut v) => std::mem::take(&mut *v),
        Err(_) => Vec::new(),
    };
    let before = entries.len();
    let mut seen = std::collections::HashSet::new();
    entries.retain(|e| seen.insert(e.dedup_key()));
    MASKED_BY_DEDUP.store(before - entries.len(), std::sync::atomic::Ordering::Relaxed);
    entries.sort_by(|a, b| a.rank().cmp(&b.rank()));
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(function: &str, loop_depth: u32, per_element: Option<&str>) -> Entry {
        Entry {
            module: "m".into(),
            function: function.into(),
            region: RegionKind::Function,
            position: Position::Local,
            name: "v".into(),
            local_id: Some(1),
            analysis: Analysis::PtrShape,
            outcome: Outcome::Denied,
            rep: "Boxed".into(),
            rule: Some("rule 2 (containment)".into()),
            reason: Some("escapes".into()),
            tier: Some(Tier::Fixable),
            issue: None,
            loop_depth,
            invoked_per_element: per_element.map(str::to_string),
            detail: None,
            byte_offset: None,
            site: None,
            alloc_context: None,
            alloc_ordinal: None,
        }
    }

    /// #7034 §8: a loop-depth-only ranking puts iterating-builtin callbacks
    /// (which have no loop of their own but run per element) at the bottom.
    /// The rank must place them above a shallower explicit loop.
    #[test]
    fn per_element_callbacks_outrank_deeper_loops() {
        let callback = entry("closure_3", 0, Some("Array.prototype.map"));
        let loop_local = entry("aaa_outer", 2, None);
        assert!(
            callback.rank() < loop_local.rank(),
            "a per-element callback must not sort below a plain loop-depth-2 local"
        );
    }

    #[test]
    fn deeper_loops_outrank_shallower_ones() {
        assert!(entry("f", 3, None).rank() < entry("f", 1, None).rank());
    }

    /// One analysis, one spelling. The summary rows use [`Analysis::as_str`]
    /// and the per-entry `analysis` field uses serde; a consumer that keys on
    /// the serde spelling (the census does, to split canonical-slot into its
    /// three reps) must find the same string in both places.
    #[test]
    fn serialized_analysis_matches_as_str() {
        for analysis in Analysis::ALL {
            let serialized = serde_json::to_string(&analysis).unwrap();
            assert_eq!(
                serialized.trim_matches('"'),
                analysis.as_str(),
                "serde and as_str disagree for {analysis:?}"
            );
        }
    }

    /// One tier, one spelling — the same trap `serialized_analysis_matches_as_str`
    /// guards for analyses. A tier whose serde name differs from its `as_str`
    /// name appears twice in one document and a consumer keying on either sees
    /// half the rows.
    #[test]
    fn serialized_tier_matches_as_str() {
        for tier in Tier::ALL {
            let serialized = serde_json::to_string(&tier).unwrap();
            assert_eq!(
                serialized.trim_matches('"'),
                tier.as_str(),
                "serde and as_str disagree for {tier:?}"
            );
        }
    }

    /// `Tier::ALL` must actually enumerate the enum. The text renderer loops
    /// over it, so a tier missing here is a tier that is in the JSON and not in
    /// the human report.
    #[test]
    fn tier_all_lists_every_variant() {
        for tier in [
            Tier::Fixable,
            Tier::InherentlyPolymorphic,
            Tier::CompilerLimitation,
            Tier::Served,
        ] {
            assert!(Tier::ALL.contains(&tier), "{tier:?} missing from Tier::ALL");
        }
        assert_eq!(Tier::ALL.len(), 4);
    }

    #[test]
    fn dedup_key_collapses_a_function_lowered_twice() {
        let a = entry("f", 0, None);
        let b = entry("f", 0, None);
        assert_eq!(a.dedup_key(), b.dedup_key());
    }

    /// The trap this outcome exists to avoid, at the data-structure level.
    ///
    /// Built the way the RECORDERS build them — a consumed entry always carries
    /// a `site` — so the test cannot pass on a shape the compiler never emits.
    /// An earlier version omitted the site and was therefore vacuous: it
    /// asserted a property that `outcome` alone provided, while the real
    /// separation in a live build comes from `site`.
    #[test]
    fn dedup_key_separates_a_consumption_from_its_own_selection() {
        let mut selected = entry("f", 0, None);
        selected.outcome = Outcome::Selected;
        selected.rule = None;
        let mut consumed = selected.clone();
        consumed.outcome = Outcome::Consumed;
        consumed.site = Some("ptr_shape_set".into());
        assert_ne!(
            selected.dedup_key(),
            consumed.dedup_key(),
            "a consumption must survive de-duplication against its selection"
        );

        let kept = {
            let mut seen = std::collections::HashSet::new();
            [selected, consumed]
                .into_iter()
                .filter(|e| seen.insert(e.dedup_key()))
                .count()
        };
        assert_eq!(kept, 2, "de-duplication swallowed the consumption record");
    }

    /// One value consumed at several sites must stay several entries: the
    /// per-site coverage gate counts them, and collapsing them would hide a
    /// recorder that stopped firing.
    #[test]
    fn dedup_key_separates_two_sites_that_consumed_one_value() {
        let mut a = entry("f", 0, None);
        a.outcome = Outcome::Consumed;
        a.rule = None;
        a.site = Some("ptr_shape_set".into());
        let mut b = a.clone();
        b.site = Some("ptr_shape_update".into());
        assert_ne!(a.dedup_key(), b.dedup_key());
    }

    /// The pair `outcome` defends — constructed by hand, because the recorders
    /// cannot currently produce it.
    ///
    /// A denied value was never selected and an unconsumed one always was, so
    /// no binding emits both, and the sabotage arm that strips `outcome` from
    /// the key leaves the census green. This test is the ONLY thing exercising
    /// that element; it is deliberately not presented as gate coverage. See
    /// `dedup_key`'s note.
    #[test]
    fn dedup_key_separates_an_unconsumed_record_from_a_denial() {
        let denied = entry("f", 0, None);
        assert_eq!(denied.outcome, Outcome::Denied);
        assert!(denied.rule.is_some() && denied.site.is_none());
        let mut unconsumed = denied.clone();
        unconsumed.outcome = Outcome::Unconsumed;
        assert_ne!(
            denied.dedup_key(),
            unconsumed.dedup_key(),
            "with a shared rule name these differ ONLY by outcome"
        );
    }

    #[test]
    fn dedup_key_separates_different_rules() {
        let a = entry("f", 0, None);
        let mut b = entry("f", 0, None);
        b.rule = Some("rule 5 (module barrier)".into());
        assert_ne!(a.dedup_key(), b.dedup_key());
    }
}
