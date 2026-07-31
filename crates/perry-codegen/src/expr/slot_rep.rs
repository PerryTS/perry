//! Representation-selection Phase 1 (RFC `docs/representation-selection-rfc.md`):
//! canonical unboxed i32 storage for proven-integer locals.
//!
//! ## The structural inversion
//!
//! Before this phase, a proven-integer local had a canonical NaN-boxed `double`
//! slot plus a *parallel* i32 shadow slot kept in sync by dual writes
//! (`needs_i32_slot` in `stmt/let_stmt.rs`). Post-`-O3` the loop-carried value
//! stayed a `phi double` with per-iteration `fptosi`/`sitofp` LLVM could not
//! remove — the canonical representation was still the box.
//!
//! Phase 1 flips it: for a local whose representation is proven `I32` (or
//! `U32`), the i32 slot in `ctx.i32_counter_slots` IS the canonical (and only)
//! storage. No double slot in `ctx.locals`, no dual writes, no shadow-stack GC
//! binding (the value is a number, never a pointer). A boxed double is
//! *materialized* (`sitofp` / `uitofp`) only at a genuinely-boxed use site.
//!
//! ## The mechanism (what Phase 2 builds on)
//!
//! - [`SlotRep`] is the seed of the RFC's representation lattice. `Boxed` is
//!   top and always sound; a local absent from `FnCtx::local_slot_reps` is
//!   `Boxed` (exactly the pre-phase behavior).
//! - `FnCtx::local_slot_reps: HashMap<u32, SlotRep>` maps LocalId → selected
//!   representation. Entries are only ever `I32`/`U32`; the alloca they refer
//!   to lives in `ctx.i32_counter_slots` (one slot registry for canonical and
//!   parallel-shadow slots — a single source of truth).
//! - All local loads/stores route through representation-aware helpers:
//!   [`canonical_local_i32_slot`] (rep + slot query),
//!   [`load_canonical_local_boxed`] (materialize a boxed double at a boxed use
//!   site), [`store_canonical_local_from_double`] (NaN-safe entry conversion
//!   into the i32 slot). Reads that want i32 keep loading the slot from
//!   `ctx.i32_counter_slots` directly — same as the shadow model.
//!
//! ## Eligibility
//!
//! Decided at the `Stmt::Let` site (`stmt/let_stmt.rs`): the existing proven
//! `needs_i32_slot` gate (`integer_locals` ∪ `unsigned_i32_locals`, restricted
//! to index-used / strictly-i32-bounded / unsigned locals, not boxed, not
//! module-global, init in range), MINUS locals referenced inside any closure
//! body (`repsel_closure_ref_locals` — the capture machinery stays on the boxed
//! protocol), and only in function contexts that allow it
//! (`repsel_context_allows_canonical_i32`: not async, not generator, not
//! `was_plain_async` — the async-to-generator transform boxes body locals).
//! Under-approximation is free: an ineligible local simply keeps today's
//! parallel-shadow (or plain boxed) lowering.
//!
//! ## Range soundness (audited 2026-07-27)
//!
//! - `strictly_i32_bounded_locals`: every write proven i32-range (greatest
//!   fixpoint; `++`/`--` disqualifies). Sound for canonical storage.
//! - `unsigned_i32_locals`: every write a top-level `>>> 0`; `++` disqualifies.
//!   u32 bit pattern round-trips; ordinary reads materialize with `uitofp`.
//! - `int_valued_ta_locals` (merged into `integer_locals`): every write i32 or
//!   a possibly-OOB int-TA read whose every observation is ToInt32-coercing;
//!   NaN-safe entry conversion (`toint32_wrap`) keeps OOB `undefined` → 0.
//! - `integer_locals ∩ index_used_locals`: admission accepts `Add/Sub/Mul`
//!   chains that can in principle exceed i32 — but under the pre-phase shadow
//!   model every `LocalGet` of such a local ALREADY reads the i32 slot
//!   (`literals_vars.rs`), so canonical-i32 storage preserves the shipped,
//!   byte-exact-validated semantics for exactly the same set of locals. No new
//!   overflow surface is introduced; tightening further would drop the loop
//!   counters and index chains this phase exists for.
//!
//! Gated by `PERRY_CANONICAL_I32_LOCALS` (default on; `0`/`off`/`false`
//! reverts to the parallel-shadow model — keyed into the object cache).
//! `PERRY_REPSEL_DEBUG=1` prints one line per canonical local at compile time.

use std::collections::HashSet;

use super::FnCtx;
use crate::types::{DOUBLE, I32, I64};

/// Slot representation for a function-local binding. Seed of the RFC's
/// representation lattice — grows richer reps (F64, Ptr, …) in later phases.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum SlotRep {
    /// NaN-boxed double slot in `ctx.locals` — today's default, always sound.
    /// Never stored in `local_slot_reps` (absent = Boxed); the variant exists
    /// so rep queries return a total answer.
    #[allow(dead_code)]
    Boxed,
    /// Canonical signed-i32 slot in `ctx.i32_counter_slots`; boxed reads
    /// materialize with `sitofp`.
    I32,
    /// Canonical u32-bit-pattern slot in `ctx.i32_counter_slots`; boxed reads
    /// materialize with `uitofp` so values above `INT32_MAX` stay observable
    /// as unsigned numbers.
    U32,
    /// Representation-selection Phase 3a: canonical string local,
    /// TAGGED-AT-REST. The slot (still the `ctx.locals` double alloca — the
    /// bits ARE the boxed form) always holds full NaN-box string bits:
    /// `STRING_TAG|ptr` for heap strings or inline `SHORT_STRING_TAG` SSO
    /// bits. Unlike `I32`/`U32` this rep does NOT move storage: boxed reads
    /// and writes are bit-identical to the pre-phase model, shadow-slot GC
    /// binding is unchanged (the mark/evacuation path is NaN-box-driven and
    /// already handles these bits), and every alias/refcount demote site
    /// keeps firing. What the rep buys is a compile-time PROOF consumed by
    /// the string-op lowerings (`+=` self-append, `.length`, `===`/`<`,
    /// `charCodeAt`-family): they tag-dispatch inline on the slot bits and
    /// call the raw string helpers directly on proven-heap handles instead
    /// of routing every operand through the opaque (and SSO-heap-
    /// materializing, number-coercing) `js_get_string_pointer_unified`.
    /// Every specialized site keeps a fallback arm with the legacy sequence,
    /// so a type-annotation lie degrades to today's behavior, never to a
    /// wrong-value coercion (RFC §5.5 acceptance contract).
    Str,
}

/// The `repsel_context_denial` rule for a module-init / program-entry body.
///
/// Both entry contexts (`codegen/entry.rs`) exclude canonical selection
/// wholesale, so every top-level local stays boxed no matter what the per-value
/// rules would have said. Naming the rule is what turns that from an invisible
/// zero into a counted denial (#7106).
pub(crate) const MODULE_INIT_CONTEXT: &str = "module_init_context";

/// Why an ordinary body context forbids canonical (i32/u32/Str) selection, or
/// `None` when it permits it.
///
/// Structural reasons only — the `PERRY_CANONICAL_{I32,STR}_LOCALS` env gates
/// are deliberately NOT reported here. Those are bisection knobs, and their
/// `=0` arms must produce the same report entries as the default build minus
/// the selections, not a new class of denial the default build never emits.
pub(crate) fn body_context_denial(
    is_async: bool,
    is_generator: bool,
    was_plain_async: bool,
) -> Option<&'static str> {
    if is_async {
        Some("async_body")
    } else if is_generator {
        Some("generator_body")
    } else if was_plain_async {
        Some("was_plain_async_body")
    } else {
        None
    }
}

/// Whether the context-denial screens (`repsel_closure_ref_locals`,
/// `repsel_str_ineligible_locals`) should be collected even though the context
/// forbids selection: only when `--opt-report` is on AND there is a structural
/// reason to report. The screens are read exclusively behind the
/// `repsel_context_allows_*` flags, so populating them in a denied context
/// changes no emitted byte — it only makes the denial count exact, by not
/// counting locals a permitting context would have rejected anyway.
pub(crate) fn report_context_denial(denial: Option<&'static str>) -> bool {
    denial.is_some() && crate::opt_report::enabled()
}

/// `PERRY_CANONICAL_I32_LOCALS` gate. Enabled by default; `=0`/`off`/`false`
/// disables canonical-i32 storage selection, reverting eligible locals to the
/// parallel-shadow model. Mirrors `int_valued_ta_locals::enabled` and is keyed
/// into the object cache (`object_cache.rs`).
pub(crate) fn canonical_i32_locals_enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_CANONICAL_I32_LOCALS").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

/// `PERRY_CANONICAL_STR_LOCALS` gate (repsel Phase 3a). Enabled by default;
/// `=0`/`off`/`false` disables canonical-Str selection and every lowering it
/// gates (the `+=`/`length`/compare/char-scan fast arms and the inline
/// `StringRef` retag), reverting to the pre-phase IR byte-for-byte. Keyed
/// into the object cache (`object_cache.rs`).
pub(crate) fn canonical_str_locals_enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_CANONICAL_STR_LOCALS").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

fn repsel_debug_enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| std::env::var("PERRY_REPSEL_DEBUG").as_deref() == Ok("1"))
}

/// Compile-time visibility: one stderr line per local that selected a
/// canonical representation (i32/u32/Str),
/// plus a process-wide running count. Only under `PERRY_REPSEL_DEBUG=1`.
pub(crate) fn note_canonical_local(ctx: &FnCtx<'_>, id: u32, name: &str, rep: SlotRep) {
    // `--opt-report` (#6952) shares this one call site with PERRY_REPSEL_DEBUG
    // so a canonical local can never show up in one mechanism and not the
    // other. `FnCtx` already knows the function and module, so no ambient
    // scope is needed here.
    if crate::opt_report::enabled() {
        crate::opt_report::select_explicit(
            &ctx.source_function,
            crate::opt_report::RegionKind::Function,
            crate::opt_report::Position::Local,
            name,
            Some(id),
            crate::opt_report::Analysis::CanonicalSlot,
            &format!("{rep:?}"),
        );
    }
    if !repsel_debug_enabled() {
        return;
    }
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let n = COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    eprintln!(
        "repsel: canonical-{rep:?} local '{name}' (id {id}) in {} [{}] (total {n})",
        ctx.source_function, ctx.module_slug
    );
}

/// The `--opt-report` denial for a local that satisfied every VALUE-level rule
/// of a canonical (i32/u32/Str) selection and lost only because the enclosing
/// context forbids the representation wholesale (#7106).
///
/// Counterpart to [`note_canonical_local`]: that records the wins, this records
/// the class of loss the report was previously blind to. A context gate is
/// taken before any per-value rule runs, so such a local used to produce no
/// entry at all — and an absent entry reads exactly like "this program had
/// nothing to promote". Eight of the eighteen census workloads (#7106) sat in
/// that state, every one of them because their hot loop is at module top level.
///
/// `loop_depth` is the number of loops open at the declaration, which is 0 for
/// a `for` counter (the target is pushed after its `Let` lowers) and ≥1 only
/// for a local declared inside a loop body. Reported as-is; it is context in
/// the JSON, never gated on.
/// Which value-level canonical-i32 rules a proven-integer local failed, in the
/// order the report should prefer to name them (#7106).
///
/// Every field is a rule the `Stmt::Let` eligibility conjunction already
/// evaluates; this only carries the individual verdicts out so the report can
/// say *which* one lost, instead of dropping the whole judgement on the floor.
pub(crate) struct CanonicalI32Denial {
    pub bigint: bool,
    pub init_out_of_range: bool,
    pub boxed_var: bool,
    pub module_global: bool,
    pub closure_referenced: bool,
    pub array_row_alias: bool,
    pub not_index_used_or_bounded: bool,
    pub context: Option<&'static str>,
}

impl CanonicalI32Denial {
    /// `(rule, reason, tier, issue)` for the first failing rule, or `None` when
    /// nothing failed — which can only happen if a caller invokes this for a
    /// local that was in fact selected, so the report stays silent rather than
    /// inventing a denial.
    fn verdict(
        &self,
    ) -> Option<(
        &'static str,
        &'static str,
        crate::opt_report::Tier,
        Option<&'static str>,
    )> {
        use crate::opt_report::Tier;
        if self.bigint {
            return Some((
                "declared_bigint",
                "declared `bigint`; canonical i32 storage would lose the value",
                Tier::InherentlyPolymorphic,
                None,
            ));
        }
        if self.init_out_of_range {
            return Some((
                "init_outside_i32",
                "the initializer is an integer literal outside the i32 range",
                Tier::InherentlyPolymorphic,
                None,
            ));
        }
        if self.boxed_var {
            return Some((
                "boxed_var",
                "captured by reference into a boxed cell, so its storage is not \
                 the local slot",
                Tier::Fixable,
                None,
            ));
        }
        if self.module_global {
            return Some((
                "module_global",
                "a module-level binding, held in an `@perry_global_*` NaN-boxed \
                 LLVM global rather than a local slot",
                Tier::CompilerLimitation,
                Some(MODULE_GLOBAL_ISSUE),
            ));
        }
        if self.closure_referenced {
            return Some((
                "closure_referenced",
                "referenced from a nested closure; the capture machinery reads \
                 the boxed double slot",
                Tier::Fixable,
                None,
            ));
        }
        if self.array_row_alias {
            return Some((
                "array_row_alias",
                "aliases a flat-const array row, so it is array-valued rather \
                 than scalar",
                Tier::CompilerLimitation,
                None,
            ));
        }
        if self.not_index_used_or_bounded {
            return Some((
                "not_index_used_or_bounded",
                "proven integer-valued, but never used as an array index and \
                 not provably i32-bounded, so nothing pins its range to 32 bits \
                 — a bare accumulator or counter lands here",
                Tier::CompilerLimitation,
                Some(NOT_BOUNDED_ISSUE),
            ));
        }
        // Everything value-level passed; only the context gate is left.
        self.context.map(|rule| {
            let (reason, issue) = context_rule_text(rule);
            (rule, reason, Tier::CompilerLimitation, Some(issue))
        })
    }
}

/// Record why a proven-integer local did not get canonical-i32 storage.
pub(crate) fn deny_canonical_i32(ctx: &FnCtx<'_>, id: u32, name: &str, denial: CanonicalI32Denial) {
    if !crate::opt_report::enabled() {
        return;
    }
    let Some((rule, reason, tier, issue)) = denial.verdict() else {
        return;
    };
    crate::opt_report::deny(crate::opt_report::Denial {
        position: crate::opt_report::Position::Local,
        name,
        local_id: Some(id),
        analysis: crate::opt_report::Analysis::CanonicalSlot,
        rule,
        reason,
        tier,
        issue,
        loop_depth: u32::try_from(ctx.loop_targets.len()).unwrap_or(u32::MAX),
        detail: Some(String::from("would have selected I32/U32")),
        byte_offset: None,
    });
}

/// Tracking issue for "a module-level binding can never take a canonical slot".
const MODULE_GLOBAL_ISSUE: &str = "#7109";
/// Tracking issue for the index-use / i32-bound precondition.
const NOT_BOUNDED_ISSUE: &str = "#7110";

/// `(reason, issue)` for a context-level denial rule.
fn context_rule_text(rule: &str) -> (&'static str, &'static str) {
    match rule {
        MODULE_INIT_CONTEXT => (
            "module-init / program-entry bodies are excluded from canonical \
             storage selection wholesale (codegen/entry.rs), so no per-value \
             rule ran for this local",
            "#7109",
        ),
        "async_body" | "was_plain_async_body" => (
            "async bodies route every body local through a shared mutable cell \
             (the async-to-generator transform), which the canonical model must \
             not touch",
            "#6328",
        ),
        _ => (
            "generator bodies route every body local through a shared mutable \
             cell, which the canonical model must not touch",
            "#6328",
        ),
    }
}

/// `Ptr<Shape>` consumption rule: the object was scalar-replaced, so there is
/// no object left for the representation to be applied to.
///
/// Not a defect. Scalar replacement (`collectors/escape_news.rs`) is a strictly
/// better outcome than promotion when it applies — the allocation disappears
/// entirely — and the two passes are complementary: one in-loop field store is
/// enough to flip a workload from scalar-replaced to `Ptr<Shape>`-consumed.
/// What is a defect is that it is INDISTINGUISHABLE in the report from a
/// promotion that was wasted, and the two mean opposite things.
pub(crate) const PTR_SHAPE_SCALAR_REPLACED: &str = "scalar_replaced";

/// `(reason, issue)` for a rule that stopped a *selected* `Ptr<Shape>` proof
/// from being consumed by codegen.
pub(crate) fn ptr_shape_context_rule_text(rule: &str) -> (&'static str, &'static str) {
    match rule {
        MODULE_INIT_CONTEXT => (
            "module-init / program-entry bodies set \
             `repsel_context_allows_canonical_i32: false` (codegen/entry.rs), and \
             `FnCtx::ptr_shape_receiver_fact` returns None for the whole body when \
             that flag is clear — so the shape proof was made, counted as a win, \
             and then dropped at every access site",
            "#7109",
        ),
        PTR_SHAPE_SCALAR_REPLACED => (
            "the object was scalar-replaced (collectors/escape_news.rs): its fields \
             became allocas and the allocation was deleted, so no property access \
             ever reaches a representation-selection lowering. A better outcome \
             than promotion, but the report counted a promotion that emitted \
             nothing",
            PTR_SHAPE_SCALAR_REPLACED_ISSUE,
        ),
        _ => (
            "async / generator bodies set `repsel_context_allows_canonical_i32: \
             false`, and `FnCtx::ptr_shape_receiver_fact` returns None for the \
             whole body when that flag is clear — the async-to-generator transform \
             owns those body locals",
            "#6328",
        ),
    }
}

/// Tracking issue for the scalar-replacement consumption mechanism.
const PTR_SHAPE_SCALAR_REPLACED_ISSUE: &str = "#7115";

pub(crate) fn deny_canonical_context(
    ctx: &FnCtx<'_>,
    id: u32,
    name: &str,
    rule: &'static str,
    rep: SlotRep,
) {
    if !crate::opt_report::enabled() {
        return;
    }
    let (reason, issue) = context_rule_text(rule);
    crate::opt_report::deny(crate::opt_report::Denial {
        position: crate::opt_report::Position::Local,
        name,
        local_id: Some(id),
        analysis: crate::opt_report::Analysis::CanonicalSlot,
        rule,
        reason,
        tier: crate::opt_report::Tier::CompilerLimitation,
        issue: Some(issue),
        loop_depth: u32::try_from(ctx.loop_targets.len()).unwrap_or(u32::MAX),
        detail: Some(format!("would have selected {rep:?}")),
        byte_offset: None,
    });
}

/// Rep + i32 slot for a canonical-i32 local; `None` when the local's slot
/// representation is `Boxed` (absent from the rep map). The slot is looked up
/// in `ctx.i32_counter_slots` — the single slot registry shared with the
/// parallel-shadow model; a rep entry without a registered slot is a compiler
/// bug (the Let site inserts both together).
pub(crate) fn canonical_local_i32_slot(ctx: &FnCtx<'_>, id: u32) -> Option<(String, SlotRep)> {
    let rep = *ctx.local_slot_reps.get(&id)?;
    debug_assert!(!matches!(rep, SlotRep::Boxed), "Boxed rep is never stored");
    // Phase 3a: a canonical-Str local has no i32 slot — its storage is the
    // ordinary `ctx.locals` double alloca (tagged-at-rest). Every i32-rep
    // query site (LocalGet materialize, LocalSet store, Update, loops,
    // capture write-back) must treat it as "not canonical-i32" and fall
    // through to the plain boxed path, which is bit-exact for Str.
    if matches!(rep, SlotRep::Str) {
        return None;
    }
    let slot = ctx
        .i32_counter_slots
        .get(&id)
        .cloned()
        .expect("canonical-i32 local must have a registered i32 slot");
    Some((slot, rep))
}

/// True when `id` selected the Phase 3a canonical-Str representation: the
/// local's `ctx.locals` slot provably holds NaN-box STRING bits (heap
/// `STRING_TAG` or inline SSO) on every proven-string write, and the
/// string-op lowerings may tag-dispatch on those bits directly.
pub(crate) fn local_is_canonical_str(ctx: &FnCtx<'_>, id: u32) -> bool {
    matches!(ctx.local_slot_reps.get(&id), Some(SlotRep::Str))
}

/// True when `id` selected canonical-i32/u32 storage (NOT Str — Str keeps
/// the plain double slot). Use instead of `local_slot_reps.contains_key`
/// wherever the follow-up action assumes an i32 slot exists.
pub(crate) fn local_rep_is_canonical_i32(ctx: &FnCtx<'_>, id: u32) -> bool {
    matches!(
        ctx.local_slot_reps.get(&id),
        Some(SlotRep::I32 | SlotRep::U32)
    )
}

/// Materialize the boxed-double view of a canonical-i32 local at a boxed use
/// site: one `sitofp` (`uitofp` for `U32`). Returns `None` for Boxed locals.
pub(crate) fn load_canonical_local_boxed(ctx: &mut FnCtx<'_>, id: u32) -> Option<String> {
    let (slot, rep) = canonical_local_i32_slot(ctx, id)?;
    let blk = ctx.block();
    let raw = blk.load(I32, &slot);
    Some(match rep {
        SlotRep::U32 => blk.uitofp(I32, &raw, DOUBLE),
        _ => blk.sitofp(I32, &raw, DOUBLE),
    })
}

/// Store an already-lowered boxed double into a canonical-i32 local's slot,
/// keeping the NaN-safe entry conversion (the #6898 trap): a possibly-non-
/// finite value (an OOB int-typed-array read is a NaN-boxed `undefined`) must
/// enter the slot as spec `ToInt32` — raw `fptosi` of a NaN is poison on
/// x86-64. `rhs` (when available) lets known-finite writes keep the cheaper
/// `fptosi→i64→trunc`, bit-identical for finite values; pass `None` for
/// values of unknown provenance (always `toint32_wrap`).
///
/// Returns `true` when the local was canonical and the store was emitted.
pub(crate) fn store_canonical_local_from_double(
    ctx: &mut FnCtx<'_>,
    id: u32,
    value: &str,
    rhs: Option<&perry_hir::Expr>,
) -> bool {
    let Some((slot, _rep)) = canonical_local_i32_slot(ctx, id) else {
        return false;
    };
    let known_finite = rhs.is_some_and(|e| super::is_known_finite(ctx, e));
    let v_i32 = if known_finite {
        let v_i64 = ctx.block().fptosi(DOUBLE, value, I64);
        ctx.block().trunc(I64, &v_i64, I32)
    } else {
        ctx.block().toint32_wrap(value)
    };
    ctx.block().store(I32, &v_i32, &slot);
    true
}

/// Locals referenced (read or written) anywhere inside a nested closure body —
/// including a closure's explicit capture list. Phase 1 keeps every such local
/// on the boxed protocol: closure capture creation snapshots the double slot,
/// and the capture/writeback machinery assumes it exists. Under-approximating
/// eligibility here is free.
/// Phase 3a eligibility pre-pass: locals that must NOT select the
/// canonical-Str representation, computed once per function body before
/// lowering (ctx-free — runs at `FnCtx` build time, so it works from
/// declared `Stmt::Let` types + syntax only and under-approximates freely).
///
/// A local is marked ineligible when any of these hold:
///
/// - **Non-string-proven write**: some `LocalSet(id, v)` where `v` is not
///   syntactically a definite string (mirrors
///   `type_analysis::strings::is_definitely_string_expr`, minus the
///   ctx-dependent arms), or any `Update` (++/--) on it.
/// - **Compare hazard** (mirrors `compare.rs`'s `other_side_is_any`
///   demote): the local appears on one side of an equality compare whose
///   other side is not itself a proven string — the static `string` type
///   may be a lie there (the NestJS `token === name` shape), so the local
///   keeps the fully generic model.
/// - **Catch binding**: `catch (e)` bindings rebind exceptional values of
///   unknown representation.
///
/// Closure bodies are NOT walked: closure-referenced locals are excluded
/// wholesale via `collect_closure_referenced_locals` (same as Phase 1),
/// which sees explicit capture lists too.
pub(crate) fn collect_canonical_str_ineligible_locals(stmts: &[perry_hir::Stmt]) -> HashSet<u32> {
    use perry_hir::{Expr, Stmt};

    // Forward pass: ids declared `let x: string` / `let x = "lit"` — the
    // set syntactic string-ness of `LocalGet` operands is judged against.
    let mut declared_str: HashSet<u32> = HashSet::new();
    fn scan_declared(stmts: &[Stmt], out: &mut HashSet<u32>) {
        for stmt in stmts {
            match stmt {
                Stmt::Let { id, ty, init, .. } => {
                    let ty_str = matches!(
                        ty,
                        perry_hir::types::Type::String | perry_hir::types::Type::StringLiteral(_)
                    );
                    let init_str = init
                        .as_ref()
                        .is_some_and(|e| matches!(e, Expr::String(_) | Expr::WtfString(_)));
                    if ty_str || init_str {
                        out.insert(*id);
                    }
                }
                Stmt::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    scan_declared(then_branch, out);
                    if let Some(e) = else_branch {
                        scan_declared(e, out);
                    }
                }
                Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => scan_declared(body, out),
                Stmt::For { init, body, .. } => {
                    if let Some(i) = init {
                        scan_declared(std::slice::from_ref(i), out);
                    }
                    scan_declared(body, out);
                }
                Stmt::Labeled { body, .. } => scan_declared(std::slice::from_ref(body), out),
                Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    scan_declared(body, out);
                    if let Some(c) = catch {
                        scan_declared(&c.body, out);
                    }
                    if let Some(f) = finally {
                        scan_declared(f, out);
                    }
                }
                Stmt::Switch { cases, .. } => {
                    for c in cases {
                        scan_declared(&c.body, out);
                    }
                }
                Stmt::Expr(_)
                | Stmt::Return(_)
                | Stmt::Throw(_)
                | Stmt::Break
                | Stmt::Continue
                | Stmt::LabeledBreak(_)
                | Stmt::LabeledContinue(_)
                | Stmt::PreallocateBoxes(_)
                | Stmt::PreallocateTdzBoxes(_) => {}
            }
        }
    }
    scan_declared(stmts, &mut declared_str);

    // Ctx-free mirror of `is_definitely_string_expr` for the write / compare
    // scans. Method calls whose NAME also exists on Array/Object (`slice`,
    // `concat`, `replace`, …) additionally require a syntactically-string
    // RECEIVER — name-only matching would classify `arr.slice()` as a
    // string write and skip the exclusion. Only the number-formatting /
    // universal-ToString family (`toString`/`toFixed`/`toPrecision`/
    // `toExponential`) stays name-only, mirroring
    // `is_definitely_string_expr`. A misclassification here is a missed
    // exclusion, not a correctness break (every specialized lowering
    // re-checks the runtime tag and falls back) — but keeping the scan
    // honest keeps ineligible locals off the canonical rep.
    fn syntactic_str(e: &Expr, declared: &HashSet<u32>) -> bool {
        match e {
            Expr::String(_) | Expr::WtfString(_) | Expr::StringCoerce(_) | Expr::TypeOf(_) => true,
            Expr::LocalGet(id) => declared.contains(id),
            Expr::Binary {
                op: perry_hir::BinaryOp::Add,
                left,
                right,
            } => syntactic_str(left, declared) || syntactic_str(right, declared),
            Expr::Conditional {
                then_expr,
                else_expr,
                ..
            } => syntactic_str(then_expr, declared) && syntactic_str(else_expr, declared),
            Expr::Call { callee, .. } => match callee.as_ref() {
                Expr::PropertyGet {
                    object, property, ..
                } => match property.as_str() {
                    "toString" | "toFixed" | "toPrecision" | "toExponential" => true,
                    "toLowerCase" | "toUpperCase" | "trim" | "trimStart" | "trimEnd" | "slice"
                    | "substring" | "substr" | "charAt" | "repeat" | "replace" | "replaceAll"
                    | "padStart" | "padEnd" | "concat" | "normalize" => {
                        syntactic_str(object, declared)
                    }
                    _ => false,
                },
                _ => false,
            },
            _ => false,
        }
    }

    struct Scan<'a> {
        declared: &'a HashSet<u32>,
        out: HashSet<u32>,
    }
    impl Scan<'_> {
        fn expr(&mut self, e: &Expr) {
            match e {
                Expr::LocalSet(id, v) => {
                    if !syntactic_str(v, self.declared) {
                        self.out.insert(*id);
                    }
                }
                Expr::Update { id, .. } => {
                    self.out.insert(*id);
                }
                Expr::Compare {
                    op:
                        perry_hir::CompareOp::Eq
                        | perry_hir::CompareOp::Ne
                        | perry_hir::CompareOp::LooseEq
                        | perry_hir::CompareOp::LooseNe,
                    left,
                    right,
                } => {
                    if let Expr::LocalGet(id) = left.as_ref() {
                        if !syntactic_str(right, self.declared) {
                            self.out.insert(*id);
                        }
                    }
                    if let Expr::LocalGet(id) = right.as_ref() {
                        if !syntactic_str(left, self.declared) {
                            self.out.insert(*id);
                        }
                    }
                }
                _ => {}
            }
            // Do not descend into closure bodies: closure-referenced locals
            // are excluded wholesale by `collect_closure_referenced_locals`.
            if !matches!(e, Expr::Closure { .. }) {
                perry_hir::walker::walk_expr_children(e, &mut |child| self.expr(child));
            }
        }
        fn stmts(&mut self, stmts: &[Stmt]) {
            for s in stmts {
                self.stmt(s);
            }
        }
        fn stmt(&mut self, s: &Stmt) {
            match s {
                Stmt::Expr(e) | Stmt::Throw(e) => self.expr(e),
                Stmt::Return(Some(e)) => self.expr(e),
                Stmt::Let { init: Some(e), .. } => self.expr(e),
                Stmt::If {
                    condition,
                    then_branch,
                    else_branch,
                } => {
                    self.expr(condition);
                    self.stmts(then_branch);
                    if let Some(eb) = else_branch {
                        self.stmts(eb);
                    }
                }
                Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                    self.expr(condition);
                    self.stmts(body);
                }
                Stmt::For {
                    init,
                    condition,
                    update,
                    body,
                } => {
                    if let Some(i) = init {
                        self.stmt(i);
                    }
                    if let Some(c) = condition {
                        self.expr(c);
                    }
                    if let Some(u) = update {
                        self.expr(u);
                    }
                    self.stmts(body);
                }
                Stmt::Labeled { body, .. } => self.stmt(body),
                Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    self.stmts(body);
                    if let Some(c) = catch {
                        if let Some((catch_id, _)) = &c.param {
                            self.out.insert(*catch_id);
                        }
                        self.stmts(&c.body);
                    }
                    if let Some(f) = finally {
                        self.stmts(f);
                    }
                }
                Stmt::Switch {
                    discriminant,
                    cases,
                } => {
                    self.expr(discriminant);
                    for c in cases {
                        if let Some(t) = &c.test {
                            self.expr(t);
                        }
                        self.stmts(&c.body);
                    }
                }
                Stmt::Return(None)
                | Stmt::Let { init: None, .. }
                | Stmt::Break
                | Stmt::Continue
                | Stmt::LabeledBreak(_)
                | Stmt::LabeledContinue(_)
                | Stmt::PreallocateBoxes(_)
                | Stmt::PreallocateTdzBoxes(_) => {}
            }
        }
    }
    let mut scan = Scan {
        declared: &declared_str,
        out: HashSet::new(),
    };
    scan.stmts(stmts);
    scan.out
}

pub(crate) fn collect_closure_referenced_locals(stmts: &[perry_hir::Stmt]) -> HashSet<u32> {
    let mut closures: Vec<(perry_hir::types::FuncId, perry_hir::Expr)> = Vec::new();
    let mut seen: HashSet<perry_hir::types::FuncId> = HashSet::new();
    crate::collectors::collect_closures_in_stmts(stmts, &mut seen, &mut closures);
    let mut out: HashSet<u32> = HashSet::new();
    for (_id, closure) in &closures {
        if let perry_hir::Expr::Closure { body, captures, .. } = closure {
            for c in captures {
                out.insert(*c);
            }
            crate::collectors::collect_ref_ids_in_stmts(body, &mut out);
        }
    }
    out
}

#[cfg(test)]
mod repsel_denial_tests {
    use super::*;

    /// Nothing failed at all: a caller that asks about a SELECTED local must
    /// not get a denial invented for it.
    fn passing() -> CanonicalI32Denial {
        CanonicalI32Denial {
            bigint: false,
            init_out_of_range: false,
            boxed_var: false,
            module_global: false,
            closure_referenced: false,
            array_row_alias: false,
            not_index_used_or_bounded: false,
            context: None,
        }
    }

    #[test]
    fn a_local_that_failed_nothing_produces_no_denial() {
        assert!(passing().verdict().is_none());
    }

    #[test]
    fn the_context_gate_is_reported_when_every_value_rule_passed() {
        let d = CanonicalI32Denial {
            context: Some(MODULE_INIT_CONTEXT),
            ..passing()
        };
        let (rule, _, _, issue) = d.verdict().expect("a context denial");
        assert_eq!(rule, MODULE_INIT_CONTEXT);
        assert_eq!(issue, Some("#7109"));
    }

    /// The regression this exists to prevent: a top-level loop counter that is
    /// index-used and otherwise fully eligible must name the CONTEXT, not fall
    /// silently out of the report. This is `11_prime_sieve`'s `i`/`j`.
    #[test]
    fn an_index_used_top_level_counter_names_the_module_init_rule() {
        let d = CanonicalI32Denial {
            not_index_used_or_bounded: false,
            context: Some(MODULE_INIT_CONTEXT),
            ..passing()
        };
        assert_eq!(d.verdict().map(|v| v.0), Some(MODULE_INIT_CONTEXT));
    }

    /// A bare accumulator at module top level fails BOTH rules. The
    /// value-level one is the more actionable, so it wins. This is
    /// `02_loop_overhead`'s `i`.
    #[test]
    fn a_value_rule_outranks_the_context_rule() {
        let d = CanonicalI32Denial {
            not_index_used_or_bounded: true,
            context: Some(MODULE_INIT_CONTEXT),
            ..passing()
        };
        let (rule, _, tier, issue) = d.verdict().expect("a value-level denial");
        assert_eq!(rule, "not_index_used_or_bounded");
        assert_eq!(tier, crate::opt_report::Tier::CompilerLimitation);
        assert_eq!(issue, Some("#7110"));
    }

    /// Precedence is total and ordered: with every rule failing at once, the
    /// most actionable one is named.
    #[test]
    fn precedence_is_stable_when_several_rules_fail() {
        let all = CanonicalI32Denial {
            bigint: true,
            init_out_of_range: true,
            boxed_var: true,
            module_global: true,
            closure_referenced: true,
            array_row_alias: true,
            not_index_used_or_bounded: true,
            context: Some(MODULE_INIT_CONTEXT),
        };
        assert_eq!(all.verdict().map(|v| v.0), Some("declared_bigint"));

        let order = [
            ("declared_bigint", "init_outside_i32"),
            ("init_outside_i32", "boxed_var"),
            ("boxed_var", "module_global"),
            ("module_global", "closure_referenced"),
            ("closure_referenced", "array_row_alias"),
            ("array_row_alias", "not_index_used_or_bounded"),
        ];
        let mut d = all;
        for (named, next) in order {
            assert_eq!(d.verdict().map(|v| v.0), Some(named));
            match named {
                "declared_bigint" => d.bigint = false,
                "init_outside_i32" => d.init_out_of_range = false,
                "boxed_var" => d.boxed_var = false,
                "module_global" => d.module_global = false,
                "closure_referenced" => d.closure_referenced = false,
                "array_row_alias" => d.array_row_alias = false,
                _ => unreachable!(),
            }
            assert_eq!(d.verdict().map(|v| v.0), Some(next));
        }
    }

    /// Body-context reasons map onto stable rule names; a permitting context
    /// yields `None` so no denial is recorded for an ordinary sync body.
    #[test]
    fn body_contexts_map_to_stable_rule_names() {
        assert_eq!(body_context_denial(false, false, false), None);
        assert_eq!(body_context_denial(true, false, false), Some("async_body"));
        assert_eq!(
            body_context_denial(false, true, false),
            Some("generator_body")
        );
        assert_eq!(
            body_context_denial(false, false, true),
            Some("was_plain_async_body")
        );
    }

    /// The screens are only collected when there is a structural reason AND
    /// the report is on — an ordinary build must do no extra walking.
    #[test]
    fn screens_are_not_collected_without_a_structural_reason() {
        assert!(!report_context_denial(None));
    }
}
