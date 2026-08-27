//! Small standalone helpers used by `compile_module`, the per-function
//! lowering helpers, and other modules in the crate.
//!
//! Split out of `codegen.rs` (now `codegen/mod.rs`). Names, behavior, and
//! visibility are unchanged — every function is re-exported from
//! `crate::codegen` as needed so external callers don't notice.

use std::collections::HashMap;

use crate::module::LlModule;
use crate::types::{DOUBLE, I32, I64, PTR};

use super::opts::{NamespaceEntry, NamespaceEntryKind};

// Relocated to `static_fields.rs` (file-size cap); re-exported so the
// existing `helpers::init_static_fields_*` paths keep resolving.
pub(super) use super::static_fields::{init_static_fields_early, init_static_fields_late};

pub(crate) fn function_body_returns_generator_object(body: &[perry_hir::Stmt]) -> bool {
    let has_gen_state = body
        .iter()
        .any(|stmt| matches!(stmt, perry_hir::Stmt::Let { name, .. } if name == "__gen_state"));
    if !has_gen_state {
        return false;
    }
    body.iter().any(|stmt| match stmt {
        // The generator transform may wrap the returned iterator in the
        // instance-prototype linker; unwrap it so the iterator shape remains
        // the stable signal that this is a lowered generator wrapper.
        perry_hir::Stmt::Return(Some(expr)) => {
            let inner = match expr {
                perry_hir::Expr::LinkGeneratorPrototype { obj, .. } => obj.as_ref(),
                other => other,
            };
            matches!(inner, perry_hir::Expr::Object(props)
                if props.len() == 3
                    && props[0].0 == "next"
                    && props[1].0 == "return"
                    && props[2].0 == "throw"
                    && props
                        .iter()
                        .all(|(_, value)| matches!(value, perry_hir::Expr::Closure { .. })))
        }
        _ => false,
    })
}

/// Compile a single user function into the module.
/// Shadow-stack push/pop + slot-set emission for every user
/// function. Default ON as of Phase D part 2 (v0.5.238); set
/// `PERRY_SHADOW_STACK=0`/`off`/`false` to disable for bisection.
/// Cached at first call so subsequent compile_* calls skip the
/// env-var lookup.
///
/// Why on by default now: the shadow stack precisely covers every
/// pointer-typed local in compiled JS frames, complementing the
/// conservative C-stack scan. With Phase A complete and the GC
/// tracer consuming the shadow stack as a parallel root source
/// (v0.5.221), enabling it is a strict-improvement default —
/// fewer over-promoted objects in generational mode, no change
/// in observed correctness, modest per-function-entry overhead
/// (one frame_push call + N slot stores at safepoints) that's
/// invisible on every measured benchmark. Phase D part 2 then
/// uses the shadow stack's authoritative JS-frame coverage to
/// shrink the conservative scanner — which only makes sense once
/// the shadow stack is guaranteed to be live.
pub(super) fn shadow_stack_enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        let on = !matches!(
            std::env::var("PERRY_SHADOW_STACK").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        );
        on
    })
}

/// Whether the precise-root **analysis** runs — i.e. whether
/// `collect_pointer_typed_locals` assigns slot indices at all.
///
/// #7326 is the distinction this function exists to draw. There are two
/// separable questions and one knob used to answer both:
///
/// 1. *Which locals hold GC pointers, and where must each stay live?*
///    That is the analysis. It is backend-independent.
/// 2. *How is the answer represented in the emitted code?* — Perry's
///    heap-backed shadow frame, or a native stack map. That is the lowering,
///    and it is chosen inside `LlFunction` (`enable_shadow_frame_inner` and
///    `reserve_shadow_slot` both return the native path first).
///
/// Conflating them made `PERRY_SHADOW_STACK=0` plus native-root lowering (then
/// spelled `PERRY_STATEPOINTS=1`, since deleted; `PERRY_RS4GC=1` today) produce a
/// binary with **no precise frame roots at all** — the analysis was switched
/// off, so the statepoint lowering had nothing to lower. No `__perry_gcmap`
/// section, same size as a plain shadow-off build, correct output. Nothing
/// distinguished it from a good build until a collection freed a live object.
/// #7332 made that combination a hard error as a stopgap; splitting the
/// predicate makes it *expressible* instead, which is the prerequisite for the
/// shadow stack's lowering ever being removed — a mode nobody can select is a
/// mode nobody can measure.
///
/// Acceptance property, asserted by test: with statepoints on, this returns
/// true regardless of `PERRY_SHADOW_STACK`, so both spellings must emit
/// byte-identical code.
pub(crate) fn precise_root_analysis_enabled() -> bool {
    shadow_stack_enabled() || native_stack_roots_enabled()
}

/// `PERRY_RS4GC=1` — research pipeline for #7174: root allocas become
/// `ptr addrspace(1)`, functions are tagged `gc "statepoint-example"`, and
/// each module is piped through `opt -passes='function(mem2reg),
/// rewrite-statepoints-for-gc'` before clang. LLVM then inserts every
/// statepoint, relocation, and downstream-use rewrite itself — replacing the
/// explicit bridge's hand emission and its conservative CFG-union liveness.
/// Requires an `opt` binary (`PERRY_LLVM_OPT`, Homebrew LLVM, or PATH).
pub(crate) fn rs4gc_enabled() -> bool {
    #[cfg(any(test, feature = "testing"))]
    if let Some(pinned) = NATIVE_ROOTS_OVERRIDE.with(|c| c.get()) {
        return pinned;
    }
    match rs4gc_env_override() {
        Some(explicit) => explicit,
        // Default: on wherever the runtime can actually walk the frames.
        None => NATIVE_ROOTS_TARGET_OK.with(|c| c.get()),
    }
}

/// `PERRY_RS4GC` as an explicit override. `Some(true)` forces the backend on
/// even for a target whose map the emitter will refuse — that refusal is the
/// point of asking, and turning it into a silent shadow-stack fallback would
/// hide exactly what the arm was set to measure.
fn rs4gc_env_override() -> Option<bool> {
    use std::sync::OnceLock;
    static CACHED: OnceLock<Option<bool>> = OnceLock::new();
    *CACHED.get_or_init(|| match std::env::var("PERRY_RS4GC").as_deref() {
        Ok("1") | Ok("on") | Ok("true") => Some(true),
        Ok("0") | Ok("off") | Ok("false") => Some(false),
        _ => None,
    })
}

thread_local! {
    static NATIVE_ROOTS_TARGET_OK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

// The pin's backing cell. Separate from `NATIVE_ROOTS_TARGET_OK` on purpose:
// `compile_module` calls `set_native_roots_for_target` per module, so a pin
// that wrote the target cell would be overwritten the moment the test invoked
// codegen. This is consulted FIRST and the per-module decision cannot clear it.
#[cfg(any(test, feature = "testing"))]
thread_local! {
    static NATIVE_ROOTS_OVERRIDE: std::cell::Cell<Option<bool>> =
        const { std::cell::Cell::new(None) };
}

/// Test-support RAII pin for the root lowering under test.
///
/// Now that native roots are the default, a test that asserts on shadow-stack
/// IR has to SAY so — it used to get that lowering by accident, because there
/// was only one default. Eight in-crate tests broke on exactly this when the
/// default flipped, and every one of them was correct about what it asserted;
/// five integration suites broke the same way and stayed red for weeks, because
/// this type was `#[cfg(test)]` and they could not reach it (#7493).
///
/// Thread-local and restoring, so one test pinning a lowering cannot change
/// another's — the same discipline `arena::quarantine`'s `ProtectionModeGuard`
/// already uses for the from-space instrument. Safe under `cargo test`'s
/// default parallelism.
///
/// Reachable from `tests/*.rs` as `perry_codegen::testing::NativeRootsPin`; see
/// [`crate::testing`] for why that is behind a cargo feature rather than an
/// unconditional `pub`.
#[cfg(any(test, feature = "testing"))]
pub struct NativeRootsPin(Option<bool>);

#[cfg(any(test, feature = "testing"))]
impl NativeRootsPin {
    /// Pin this thread to the **shadow-stack** lowering for the guard's
    /// lifetime — Perry's heap-backed shadow frame, `js_shadow_frame_enter` +
    /// per-slot binds.
    pub fn shadow() -> Self {
        NativeRootsPin(NATIVE_ROOTS_OVERRIDE.with(|c| c.replace(Some(false))))
    }

    /// Pin this thread to the **native-roots** (RS4GC statepoint) lowering:
    /// `ptr addrspace(1)` root allocas, `gc "statepoint-example"`, relocations
    /// inserted by LLVM.
    ///
    /// This is today's default on every target the runtime can walk, so a test
    /// that wants it does not strictly *need* the pin — but a pin is not
    /// redundant: it also overrides `PERRY_RS4GC` from the environment, so the
    /// assertion means the same thing during a `PERRY_RS4GC=0` bisection run as
    /// it does in CI. Without it, a whole-suite sweep under the process-global
    /// env knob silently retargets every unpinned test at the other lowering.
    pub fn native() -> Self {
        NativeRootsPin(NATIVE_ROOTS_OVERRIDE.with(|c| c.replace(Some(true))))
    }
}

#[cfg(any(test, feature = "testing"))]
impl Drop for NativeRootsPin {
    fn drop(&mut self) {
        NATIVE_ROOTS_OVERRIDE.with(|c| c.set(self.0));
    }
}

/// Decide, once per module, whether native roots are the right lowering for
/// this target. Same set-per-module discipline as `set_jscvt_for_target`.
///
/// This is what makes "statepoints by default" safe to say. Support is
/// per-target, not global: `gc_map` REFUSES to emit a map for a target whose
/// frame bases the runtime cannot resolve, because a map nothing reads loses
/// roots silently. A blanket default would therefore turn every watchOS
/// `arm64_32` and ARM64-Windows compile into a hard error.
///
/// So the default is *native roots where the runtime can walk, shadow stack
/// where it cannot*. Both are correct rooting mechanisms — #7340 split the
/// root-set analysis from its lowering precisely so the choice could be made
/// per-target instead of per-build. Falling back here is not "no roots"; it is
/// the other lowering of the same analysis.
///
/// **Keep this predicate in agreement with `gc_map`'s refusals.** If this says
/// yes where the emitter says no, the compile fails outright; the emitter is
/// the authority and this must not be looser than it.
pub(crate) fn set_native_roots_for_target(triple: &str) {
    // aarch64/arm64 and x86_64 only, mirroring gc_map's `arch_supported`.
    let arch_ok = (triple.starts_with("aarch64")
        || triple.starts_with("arm64")
        || triple.starts_with("x86_64"))
        // watchOS ILP32: 32-bit pointers, and the runtime's map loader is
        // gated to 64-bit Apple, so a map here would be read by nothing.
        && !triple.starts_with("arm64_32");
    // Windows has a walker only on x86-64 (#7354): ARM64 Windows passes the
    // arch check and is COFF, but its CONTEXT layout and register model differ,
    // so no frame would ever be visited.
    let windows_ok = !triple.contains("windows") || triple.starts_with("x86_64");
    NATIVE_ROOTS_TARGET_OK.with(|c| c.set(arch_ok && windows_ok));
}

/// Whether precise roots should use a native-stack metadata backend rather
/// than Perry's heap-backed shadow frame.
pub(crate) fn native_stack_roots_enabled() -> bool {
    rs4gc_enabled()
}

/// `PERRY_GC_SAFEPOINT_ONLY=1` — the explicit-safepoint collection contract
/// (research, `exp/stackmap-viability`). The runtime enforces that a
/// precise-root collection only begins at a declared safepoint; under that
/// guarantee, audited allocate-but-never-reenter helpers
/// (`GcCallEffect::AllocNoReentry`) need no statepoint. Participates in both
/// build and object cache keys.
pub(crate) fn gc_safepoint_only_contract_enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        matches!(
            std::env::var("PERRY_GC_SAFEPOINT_ONLY").as_deref(),
            Ok("1") | Ok("on") | Ok("true") | Ok("strict")
        )
    })
}

/// Inline shadow-slot store gate (#7088). Default ON.
///
/// When enabled, a store to a GC-rooted local is emitted as an address
/// computation and a pair of stores against this thread's `ShadowStackState`
/// instead of a call to `js_shadow_slot_bind` / `js_shadow_slot_set`. The
/// runtime entry points stay exported, and are what the emitted code falls
/// back to when no state pointer is available for the activation.
///
/// `PERRY_INLINE_SHADOW_SLOT=0`/`off`/`false` reverts to the calls, for
/// bisection. Independent of `PERRY_SHADOW_STACK`, which switches root
/// emission off entirely; with the shadow stack off there is nothing to inline.
pub(crate) fn inline_shadow_slot_enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_INLINE_SHADOW_SLOT").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

/// Inline-hot-small gate. Default ON. When enabled, small functions
/// (`INLINE_HOT_SMALL_MIN ..= SIZE_CAP` statements) that have ≥1 call site
/// inside a loop get LLVM's `inlinehint` — a *bounded* nudge that raises the
/// inline threshold for that callee while LLVM's `-O3` growth budget stays the
/// backstop (unlike `alwaysinline`, which is unconditional). Disable with
/// `PERRY_INLINE_HOT_SMALL=0`/`off`/`false` for bisection / binary-size A/B.
pub(crate) fn inline_hot_small_enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_INLINE_HOT_SMALL").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

/// LLVM `-inlinehint-threshold` used for `inlinehint`-marked callees when the
/// feature is on. The default hint threshold (325) is too low for Perry's
/// NaN-boxed kernels — a ~10-statement bit-mixer costs ~800 in LLVM's inline
/// model once GC shadow-frame calls + typed-array reads + double↔i32 marshaling
/// are counted — so we raise it. Critically this only affects functions Perry
/// stamped `inlinehint` (the small + in-loop-callsite gate); every other
/// function keeps the base `-O3` threshold, so cold code is untouched.
/// Overridable via `PERRY_INLINE_HOT_SMALL_THRESHOLD` for the binary-size A/B.
pub(crate) fn inline_hot_small_hint_threshold() -> u32 {
    use std::sync::OnceLock;
    static CACHED: OnceLock<u32> = OnceLock::new();
    *CACHED.get_or_init(|| {
        std::env::var("PERRY_INLINE_HOT_SMALL_THRESHOLD")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(850)
    })
}

/// Smallest body-statement count eligible for `inlinehint`. Functions of `<= 8`
/// statements already get unconditional `alwaysinline`, so the hint window
/// starts one above that.
pub(super) const INLINE_HOT_SMALL_MIN: usize = 9;

/// Largest body-statement count eligible for `inlinehint`. Chosen
/// conservatively and validated against the binary-size regression gate (a
/// larger cap duplicates more code at each hinted site). Overridable via
/// `PERRY_INLINE_HOT_SMALL_CAP` for tuning experiments.
pub(super) fn inline_hot_small_size_cap() -> usize {
    use std::sync::OnceLock;
    static CACHED: OnceLock<usize> = OnceLock::new();
    *CACHED.get_or_init(|| {
        std::env::var("PERRY_INLINE_HOT_SMALL_CAP")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(20)
    })
}

/// Give the representation-specialized method body the ordinary small-body
/// inline bias. This lets producer-local chains such as `Registry.add ->
/// Group.pushEntity` optimize through the second method boundary while the
/// externally linked clone remains callable by importers.
pub(super) fn apply_pshape_inline_policy(
    lf: &mut crate::function::LlFunction,
    method: &perry_hir::Function,
    is_pshape_clone: bool,
) {
    if !is_pshape_clone || method.is_async || method.is_generator || method.was_plain_async {
        return;
    }
    if method.body.len() <= 8 {
        lf.force_inline = true;
    } else if inline_hot_small_enabled() && method.body.len() <= inline_hot_small_size_cap() {
        lf.inline_hint = true;
    }
}

/// Maximum pre-optimization LLVM IR body size admitted to the native-roots
/// pre-statepoint inliner for a guarded specialization.
///
/// HIR statement count is deliberately not used here: one source statement
/// can lower to a large property/index dispatch lattice.  Sixteen KiB admits
/// compact exact-receiver and nonnegative-index leaves while rejecting bodies
/// such as mutation-heavy ECS transitions by nearly an order of magnitude.
pub(super) const GUARDED_SPECIALIZATION_PREINLINE_MAX_IR_BYTES: usize = 16 * 1024;

#[inline]
pub(super) fn guarded_specialization_fits_preinline_budget(ir_bytes: usize) -> bool {
    ir_bytes <= GUARDED_SPECIALIZATION_PREINLINE_MAX_IR_BYTES
}

/// Maximum total (module-wide) direct call sites a function may have and still
/// be hinted. This is the anti-bloat backstop: the raised `-inlinehint-threshold`
/// lifts LLVM's ceiling for a hinted callee at *every* one of its call sites, so
/// without this cap a small hot kernel that is also called from many cold sites
/// would be duplicated at all of them. Capping call sites bounds the added code
/// (≤ this many inlined copies). A tight bit-mixer kernel has 1–2 sites and
/// still qualifies; a broadly-shared helper does not. Overridable via
/// `PERRY_INLINE_HOT_SMALL_MAX_SITES`.
pub(crate) fn inline_hot_small_max_call_sites() -> u32 {
    use std::sync::OnceLock;
    static CACHED: OnceLock<u32> = OnceLock::new();
    *CACHED.get_or_init(|| {
        std::env::var("PERRY_INLINE_HOT_SMALL_MAX_SITES")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(4)
    })
}

/// #8583: statepoint relocation estimate above which a function keeps its GC
/// roots in a shadow frame instead of native statepoints.
///
/// `rewrite-statepoints-for-gc` adds one relocation per GC value live across
/// each safepoint, so the optimizer's post-rewrite cost scales with
/// `live_roots × safepoints`. Past a point that fan-out makes the `-Os`/`-O3`
/// middle-end super-linear and the compile does not finish (the Claude Code
/// bundle's 68 MB entry body measured 795 root slots × ~106k safepoints ≈ 8.4e7
/// and grew 439k → 6.5M instructions under RS4GC; without RS4GC the same unit
/// optimized at `-Os` in ~5s). Real functions sit orders of magnitude below
/// this: hundreds of call sites times tens of slots is ~1e4–1e5.
///
/// The default (#8620) is measured, not guessed. Synthetic entry functions with
/// a controlled `slots × safepoints` estimate were compiled at `-Os` with
/// spilling OFF (pure RS4GC fan-out) and the `@main` codegen unit timed:
///
/// | estimate | fan-out finish |
/// |---------:|---------------:|
/// |     8.0M |         ~325 s |
/// |    16.0M |         ~235 s |
/// |    32.0M |  ~511 s (8.5m) |
/// |    40.0M |  did not finish in 20 min |
/// |    48.0M |  did not finish in 20 min |
///
/// The fan-out cliff sits between 32M and 40M, so the default is the largest
/// estimate whose fan-out still finished in bounded time. Below it fan-out is
/// the cheaper lowering — spilling a moderate function costs more than the
/// fan-out it avoids (an ~8M function spilled in 303 s vs 180 s fanned out,
/// #8620) — and above it fan-out risks not finishing and the shadow frame wins.
/// The former 4M default fired on ~8M functions that fan out fine in minutes.
/// The post-RS4GC instruction budget (#8586/#8679, inprocess.rs) backstops any
/// function this estimate misses: it re-lowers that function onto a precise
/// shadow frame and retries before LLVM's optimizer can hang, so raising the
/// estimate threshold is safe.
///
/// `PERRY_ROOT_SPILL_RELOCATIONS=<n>` overrides it; `0` disables spilling
/// (every function stays on native statepoints, the pre-#8583 behavior).
const DEFAULT_ROOT_SPILL_RELOCATIONS: usize = 32_000_000;

fn root_spill_relocation_threshold() -> usize {
    std::env::var("PERRY_ROOT_SPILL_RELOCATIONS")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(DEFAULT_ROOT_SPILL_RELOCATIONS)
}

/// The relocation estimate for a function with `slot_count` GC-root slots and
/// a body containing `safepoint_sites` call-like expressions. Saturating so a
/// pathological product cannot wrap.
/// The root population RS4GC actually relocates: named pointer locals plus
/// ~one live pointer temporary per call result (#8583). Production and the
/// threshold tests must agree on this composition — computing it in only one
/// of the two is how the endpoint tests silently stop guarding the real
/// formula.
pub(crate) fn spill_live_root_count(slot_count: usize, safepoint_sites: usize) -> usize {
    slot_count.saturating_add(safepoint_sites)
}

pub(crate) fn root_relocation_estimate(slot_count: usize, safepoint_sites: usize) -> usize {
    slot_count.saturating_mul(safepoint_sites)
}

#[cfg(test)]
mod root_spill_default_tests {
    use super::{root_relocation_estimate, spill_live_root_count, DEFAULT_ROOT_SPILL_RELOCATIONS};

    /// Exactly what `maybe_spill_roots_to_shadow_frame` computes, so these
    /// endpoint tests track the production formula instead of a stale copy of
    /// it (#8633 changed the composition; before this helper the tests still
    /// asserted on the pre-#8633 `slot_count x sites`).
    fn production_estimate(slot_count: usize, sites: usize) -> usize {
        root_relocation_estimate(spill_live_root_count(slot_count, sites), sites)
    }

    /// #8620: the default is pinned to the measured RS4GC fan-out cliff — the
    /// largest estimate whose fan-out finished in bounded time (32M finished in
    /// ~8.5 min; 40M/48M did not finish in 20 min). Change it only with fresh
    /// measurement.
    #[test]
    fn default_sits_at_the_measured_fan_out_cliff() {
        assert_eq!(DEFAULT_ROOT_SPILL_RELOCATIONS, 32_000_000);
    }

    /// The moderate case the old 4M default wrongly spilled (#8620): ~8M
    /// relocations (4000 root slots × ~2001 safepoints) fans out in minutes, so
    /// under the new default it stays on native statepoints.
    #[test]
    fn moderate_fan_out_stays_on_statepoints() {
        let est = production_estimate(4000, 2001);
        assert_eq!(est, 12_008_001);
        assert!(
            est <= DEFAULT_ROOT_SPILL_RELOCATIONS,
            "moderate estimate {est} must not exceed the default (would spill)",
        );
    }

    /// The genuinely-catastrophic case (Claude Code `cli.js` `@main`,
    /// ~795 slots × ~106k safepoints ≈ 8.4e7, never finishes at `-Os`) must
    /// still spill under the new default.
    #[test]
    fn catastrophic_fan_out_still_spills() {
        let est = production_estimate(795, 106_000);
        assert!(
            est > DEFAULT_ROOT_SPILL_RELOCATIONS,
            "catastrophic estimate {est} must exceed the default (should spill)",
        );
    }
}

/// Decide whether `func` should spill its roots to the shadow frame, and if so
/// mark it (BEFORE its `enable_*_shadow_frame` call) and report it. Only
/// meaningful under native stack-map roots — the shadow frame is already the
/// lowering otherwise. Reporting is at default verbosity because #8421 requires
/// that a change to how a function is compiled is never silent; the message
/// states that the optimization level is unchanged.
pub(super) fn maybe_spill_roots_to_shadow_frame(
    func: &mut crate::function::LlFunction,
    fn_name: &str,
    slot_count: usize,
    body: &[perry_hir::Stmt],
) {
    if !native_stack_roots_enabled() {
        return;
    }
    let threshold = root_spill_relocation_threshold();
    if threshold == 0 {
        return;
    }
    let sites = crate::collectors::count_safepoint_sites(body);
    // #8583 (unit-4 / `__33499` of the Claude Code bundle): `slot_count` is the
    // shadow-slot map size — the count of *named* pointer-typed locals — but
    // that is NOT the root population RS4GC relocates. A call-heavy minified
    // closure produces one pointer-typed *temporary* per call result (the
    // constructed IR carries ~one `alloca ptr addrspace(1)` per call), and each
    // is live across the later safepoints; those temporaries dominate the true
    // root count yet are invisible to `collect_pointer_typed_locals`. `__33499`
    // measured ~20.3k named-and-anonymous pointer roots × ~20.3k safepoints, but
    // its `slot_count` alone was ~100x smaller, so `slot_count × sites` fell
    // under the threshold, the function stayed on statepoints, and RS4GC then
    // fanned out for >3 h / ~30 GiB (never reaching the #8586 post-rewrite
    // budget assertion, which only fires *after* the rewrite it never finishes).
    // Count each safepoint as contributing ~one live pointer temporary. This is
    // an over-approximation biased toward spilling — the intended direction (a
    // false-positive shadow frame is cheap; a missed fan-out is not).
    let live_roots = spill_live_root_count(slot_count, sites);
    let estimate = root_relocation_estimate(live_roots, sites);
    if estimate <= threshold {
        return;
    }
    func.request_shadow_frame_spill();
    eprintln!(
        "perry: `{fn_name}` keeps its {live_roots} GC roots (incl. call-result temporaries) in a \
         shadow frame instead of statepoints: an estimated {estimate} relocations ({live_roots} \
         roots × {sites} safepoints) would make rewrite-statepoints-for-gc fan-out super-linear in \
         the optimizer (> {threshold}). The function is still compiled at the requested \
         optimization level; only its GC-root representation changes, and its roots stay \
         precise (#8583). Override with PERRY_ROOT_SPILL_RELOCATIONS."
    );
}

pub(super) fn enable_module_init_shadow_frame(
    func: &mut crate::function::LlFunction,
    stmts: &[perry_hir::Stmt],
    flat_const_ids: &std::collections::HashSet<u32>,
) -> (HashMap<u32, u32>, HashMap<usize, Vec<u32>>) {
    if !precise_root_analysis_enabled() {
        return (HashMap::new(), HashMap::new());
    }

    let shadow_slot_map =
        crate::collectors::collect_pointer_typed_locals(&[], stmts, flat_const_ids);
    // #8583: the module-entry body is the minified-bundle IIFE — the function
    // that fans out catastrophically under RS4GC. Decide its root lowering
    // before the frame is built.
    maybe_spill_roots_to_shadow_frame(func, "main", shadow_slot_map.len(), stmts);
    func.enable_post_init_shadow_frame(shadow_slot_map.len() as u32);
    let shadow_slot_clears_after_stmt =
        crate::collectors::collect_shadow_slot_clear_points(stmts, &shadow_slot_map);
    (shadow_slot_map, shadow_slot_clears_after_stmt)
}

/// Gen-GC write-barrier emission gate. Default ON: emit a
/// `js_write_barrier_slot(parent_bits, slot_addr, child_bits)` call, or
/// the compatibility wrapper, after every heap-store site. Set
/// `PERRY_WRITE_BARRIERS=0`/`off`/`false` to disable emission for
/// benchmark/debug bisection. `=1`/`on`/`true` remain accepted and
/// equivalent to the default.
pub(crate) fn write_barriers_enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_WRITE_BARRIERS").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

thread_local! {
    static FULL_OUTLINE_IC: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static JSCVT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// FEAT_JSCVT (`fjcvtzs`) availability for the CURRENT module's target: the
/// single-instruction, spec-exact ECMAScript `ToInt32` on ARMv8.3+. Only
/// arm64 macOS triples opt in — every Apple Silicon Mac (M1+) is ≥ ARMv8.4,
/// while iOS/tvOS device targets can still cover A7–A11 chips (ARMv8.0–8.2,
/// no JSCVT — `fjcvtzs` would be an illegal instruction) and generic aarch64
/// (Graviton2/Neoverse-N1) lacks it too. `PERRY_JSCVT=0/off/false` reverts
/// `toint32_wrap` to the branchless shift/select tower (A/B bisection; keyed
/// into the object cache). Same thread-local per-module discipline as
/// `FULL_OUTLINE_IC` above.
pub(crate) fn jscvt_enabled() -> bool {
    JSCVT.with(|c| c.get())
}

pub(crate) fn set_jscvt_for_target(triple: &str) {
    let env_off = matches!(
        std::env::var("PERRY_JSCVT").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    );
    let target_has_jscvt = (triple.starts_with("arm64") || triple.starts_with("aarch64"))
        && (triple.contains("apple-macosx") || triple.contains("apple-darwin"));
    JSCVT.with(|c| c.set(target_has_jscvt && !env_off));
}

/// Lever B (#5334) full-outline gate for class-field IC diamonds. Set ONCE per
/// module at the top of `compile_module` (see [`decide_full_outline_ic`]), read
/// at each class-field-set lowering. Thread-local — NOT a process-global
/// `OnceLock` — because codegen runs one module per `compile_module` call and a
/// process-global would wrongly pin the first module's decision for the rest of
/// a multi-module build (and across tests). Codegen within a module is
/// sequential, so a thread-local is safe and avoids threading a flag through all
/// six `FnCtx` construction sites.
pub(crate) fn full_outline_ic_enabled() -> bool {
    FULL_OUTLINE_IC.with(|c| c.get())
}

pub(crate) fn set_full_outline_ic(enabled: bool) {
    FULL_OUTLINE_IC.with(|c| c.set(enabled));
}

/// Total number of LLVM functions a module will emit — top-level functions
/// plus every class callable (constructor, instance/static methods, computed
/// members, accessor get/set bodies). Used as the lever-B size proxy: class
/// methods and closures
/// do NOT live in `hir.functions`, so a class-heavy minified bundle (the exact
/// pathology lever B targets) can have a small `functions.len()` yet emit tens
/// of thousands of LLVM functions. Counting class callables keeps the gate from
/// silently under-counting and never firing on those modules.
pub(crate) fn module_callable_count(hir: &perry_hir::Module) -> usize {
    let class_callables: usize = hir
        .classes
        .iter()
        .map(|c| {
            usize::from(c.constructor.is_some())
                + c.methods.len()
                + c.static_methods.len()
                + c.computed_members.len()
                + c.getters.len()
                + c.setters.len()
        })
        .sum();
    hir.functions.len() + class_callables
}

/// Decide whether a module is large enough to warrant full-outlining its
/// class-field IC diamonds (#5334 lever B). Collapsing each inline diamond's
/// ~15-line-per-site expansion to one `call @js_class_field_set_ic(...)` keeps
/// large functions tractable for LLVM's `-O3` pipeline. Gated on the module's
/// total callable count (see
/// [`module_callable_count`]) — the defining trait of the pathological
/// minified-bundle case (tens of thousands of callables in one module);
/// ordinary per-file modules stay on the inline diamond and keep the hot fast
/// store.
///
/// `PERRY_FULL_OUTLINE_IC=1`/`on`/`true` forces ON, `=0`/`off`/`false` forces
/// OFF; otherwise auto: `callable_count >= PERRY_FULL_OUTLINE_IC_MIN_FUNCS`
/// (default 4000).
pub(crate) fn decide_full_outline_ic(callable_count: usize) -> bool {
    match std::env::var("PERRY_FULL_OUTLINE_IC").as_deref() {
        Ok("1") | Ok("on") | Ok("true") => return true,
        Ok("0") | Ok("off") | Ok("false") => return false,
        _ => {}
    }
    let threshold = std::env::var("PERRY_FULL_OUTLINE_IC_MIN_FUNCS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(4000);
    callable_count >= threshold
}

/// Decide how many codegen units to split a module's object compilation into
/// (#5391). A single huge translation unit makes `clang -c` OOM (~15GB on the
/// 13MB bundle); splitting bounds peak compiler memory to roughly whole/N.
///
/// `PERRY_CODEGEN_UNITS=N` forces exactly N units (1 disables splitting).
/// Otherwise auto: choose the larger of the callable-count estimate and the
/// post-lowering generated-IR estimate. The latter matters for generated and
/// minified bundles: one HIR callable can expand into many large helper/wrapper
/// bodies, so callable count alone left 100+ MiB LLVM modules unsplit.
///
/// `PERRY_CODEGEN_UNIT_SIZE` overrides callables/unit;
/// `PERRY_CODEGEN_UNIT_BYTES` overrides generated IR bytes/unit.
pub(crate) fn decide_codegen_units(callable_count: usize, estimated_ir_bytes: usize) -> usize {
    if let Ok(v) = std::env::var("PERRY_CODEGEN_UNITS") {
        if let Ok(n) = v.parse::<usize>() {
            return n.max(1);
        }
    }
    const MIN_CALLABLES_TO_SPLIT: usize = 8_000;
    // Real-app calibration (OpenCode's split CLI): the estimator reports only
    // function bodies, while LLVM also receives globals, declarations,
    // attributes, and metadata. Modules estimated just below the old 48 MiB
    // gate therefore reached LLVM as 44--46 MiB single units. Late in a large
    // build, with the collected program HIR and cached-object bookkeeping
    // resident, two such units expanded the process/pagefile until C: had less
    // than 1 GiB free. Start splitting at 16 MiB of estimated function IR and
    // require at least two units once the gate is crossed; the 20 MiB target
    // remains the balancing goal for larger modules.
    const MIN_IR_BYTES_TO_SPLIT: usize = 16 * 1024 * 1024;
    const DEFAULT_IR_BYTES_PER_UNIT: usize = 20 * 1024 * 1024;
    const MAX_UNITS: usize = 128;
    let target_callables = std::env::var("PERRY_CODEGEN_UNIT_SIZE")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(6000);
    let target_ir_bytes = std::env::var("PERRY_CODEGEN_UNIT_BYTES")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(DEFAULT_IR_BYTES_PER_UNIT);
    let by_callables = if callable_count >= MIN_CALLABLES_TO_SPLIT {
        callable_count.div_ceil(target_callables)
    } else {
        1
    };
    let by_ir = if estimated_ir_bytes >= MIN_IR_BYTES_TO_SPLIT {
        estimated_ir_bytes.div_ceil(target_ir_bytes).max(2)
    } else {
        1
    };
    by_callables.max(by_ir).clamp(1, MAX_UNITS)
}

#[cfg(test)]
mod codegen_unit_tests {
    use super::decide_codegen_units;

    #[test]
    fn splits_medium_generated_modules_before_the_llvm_memory_cliff() {
        assert_eq!(decide_codegen_units(800, 15 * 1024 * 1024), 1);
        assert_eq!(decide_codegen_units(800, 16 * 1024 * 1024), 2);
        assert_eq!(decide_codegen_units(800, 45 * 1024 * 1024), 3);
    }
}

pub(super) fn scoped_fn_name(module_prefix: &str, hir_name: &str) -> String {
    // Use the INJECTIVE sanitizer (same as scoped_static_method_name): plain
    // `sanitize` maps every non-`[A-Za-z0-9_]` char to `_`, so distinct minified
    // function names like `$Z5` and `_Z5` both became `perry_fn_<mod>___Z5` and
    // clang rejected the module with "invalid redefinition of function". `func_names`
    // is keyed by func id and every reference resolves through it, so changing the
    // mangling here keeps all local-function call sites consistent. Byte-identical
    // to `sanitize` for plain `[A-Za-z0-9_]` names (the overwhelming common case).
    format!("perry_fn_{}__{}", module_prefix, sanitize_member(hir_name))
}

/// Stable, module-scoped sentinel used when a [`perry_hir::Expr::FuncRef`]
/// cannot be resolved in the module's function registry.
///
/// The fallback wrapper is emitted once per source module. Keeping the module
/// prefix in the sentinel name is required when codegen-unit splitting
/// promotes that wrapper from internal to external linkage for cross-unit
/// calls: independently compiled modules must not publish the same symbol.
pub(crate) fn unknown_func_name(module_prefix: &str) -> String {
    format!("perry_unknown_func_{module_prefix}")
}

pub(crate) fn unknown_func_wrapper_name(module_prefix: &str) -> String {
    format!("__perry_wrap_{}", unknown_func_name(module_prefix))
}

pub(super) fn scoped_static_method_name(
    module_prefix: &str,
    class_id: u32,
    class_name: &str,
    method_name: &str,
) -> String {
    format!(
        "perry_static_{}__{}__c{}__{}",
        module_prefix,
        sanitize_member(class_name),
        class_id,
        sanitize_member(method_name)
    )
}

pub(super) fn node_stream_parent_kind(
    classes: &HashMap<String, &perry_hir::Class>,
    class: &perry_hir::Class,
) -> Option<&'static str> {
    let mut cur = class.extends_name.as_deref();
    let mut depth = 0usize;
    while let Some(name) = cur {
        match name {
            "Readable" => return Some("readable"),
            "Duplex" => return Some("duplex"),
            "Transform" => return Some("transform"),
            _ => {}
        }
        cur = classes
            .get(name)
            .copied()
            .and_then(|parent| parent.extends_name.as_deref());
        depth += 1;
        if depth > 32 {
            break;
        }
    }
    None
}

/// Walk a function body looking for `Return(Some(expr))` shapes that
/// identify the function as a factory returning a class. Sets
/// `*produced` to the resolved class name when the first qualifying
/// return is seen; sets `*disqualified` when a return points at
/// something we can't classify as a class. Used by
/// `func_returns_class_map` fixed-point in `compile_module` to recognise
/// Effect's `Literal` / `makeLiteralClass` / `make` factories. Refs
/// #915 (gap 3 / #321 follow-up).
///
/// Recognised return shapes:
///   - `Return(Some(ClassRef(name)))` — direct class literal return.
///   - `Return(Some(Call { callee: FuncRef(other_fid), .. }))` — call
///     to another already-tagged factory (transitive).
///   - `Return(Some(Conditional { then, else, .. }))` — both branches
///     must independently resolve to the same class. Effect's
///     `Literal(...)` has this shape — the body is
///     `array_.isNonEmptyReadonlyArray(literals) ? makeLiteralClass(literals) : Never`.
///   - `Return(Some(Sequence([..., ClassRef(name)])))` — the HIR's
///     inliner sometimes collapses a factory call to
///     `Sequence([RegisterClassParentDynamic, ClassRef(name)])`. Treat
///     the trailing class as the produced value.
///
/// Anything else inside a `Return(Some(_))` disqualifies the function:
/// we'd rather miss a factory than mis-classify a non-factory.
/// Returns inside nested closures are SKIPPED — those belong to the
/// inner function (the walker doesn't recurse into Expr).
pub(super) fn collect_return_class(
    stmts: &[perry_hir::Stmt],
    produced: &mut Option<String>,
    disqualified: &mut bool,
    func_returns_class: &std::collections::HashMap<u32, String>,
) {
    use perry_hir::{Expr, Stmt};

    fn resolve_class(
        expr: &perry_hir::Expr,
        func_returns_class: &std::collections::HashMap<u32, String>,
    ) -> Option<String> {
        match expr {
            Expr::ClassRef(name) => Some(name.clone()),
            Expr::Call { callee, .. } => match callee.as_ref() {
                Expr::FuncRef(fid) => func_returns_class.get(fid).cloned(),
                _ => None,
            },
            Expr::Conditional {
                then_expr,
                else_expr,
                ..
            } => {
                let lhs = resolve_class(then_expr, func_returns_class)?;
                let rhs = resolve_class(else_expr, func_returns_class)?;
                if lhs == rhs {
                    Some(lhs)
                } else {
                    None
                }
            }
            Expr::Sequence(exprs) => exprs
                .last()
                .and_then(|e| resolve_class(e, func_returns_class)),
            _ => None,
        }
    }

    for stmt in stmts {
        if *disqualified {
            return;
        }
        match stmt {
            Stmt::Return(Some(expr)) => {
                let resolved = resolve_class(expr, func_returns_class);
                match resolved {
                    Some(name) => match produced {
                        None => *produced = Some(name),
                        Some(existing) if *existing == name => {}
                        Some(_) => {
                            // Mixed return shapes — bail.
                            *disqualified = true;
                        }
                    },
                    None => {
                        *disqualified = true;
                    }
                }
            }
            Stmt::Return(None) => {
                // Returning undefined — disqualify (caller can't
                // depend on the receiver being a class).
                *disqualified = true;
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_return_class(then_branch, produced, disqualified, func_returns_class);
                if let Some(eb) = else_branch {
                    collect_return_class(eb, produced, disqualified, func_returns_class);
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_return_class(body, produced, disqualified, func_returns_class);
                if let Some(cc) = catch {
                    collect_return_class(&cc.body, produced, disqualified, func_returns_class);
                }
                if let Some(blk) = finally {
                    collect_return_class(blk, produced, disqualified, func_returns_class);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    collect_return_class(&case.body, produced, disqualified, func_returns_class);
                }
            }
            Stmt::Labeled { body, .. } => {
                let slice = std::slice::from_ref(body.as_ref());
                collect_return_class(slice, produced, disqualified, func_returns_class);
            }
            _ => {}
        }
    }
}

/// Mangle a class method name into an LLVM symbol, scoped by module
/// prefix and class name.
///
/// `perry_method_<modprefix>__<class>__<method>`.
pub(super) fn scoped_method_name(
    module_prefix: &str,
    class_name: &str,
    method_name: &str,
) -> String {
    format!(
        "perry_method_{}__{}__{}",
        module_prefix,
        sanitize_member(class_name),
        sanitize_member(method_name)
    )
}

/// Sanitize a name for use in an LLVM symbol — replace anything that isn't
/// `[A-Za-z0-9_]` with an underscore. LLVM IR identifiers cannot start with
/// a digit, so prefix with `_` if the first character would be one (this
/// happens with module names like `05_fibonacci.ts`).
///
/// The output alphabet being strictly `[A-Za-z0-9_]` is a load-bearing
/// invariant (issue #6927): `$` is reserved for compiler-generated clone /
/// uniquifier suffixes (`$generic`, `$typed_*`, `$dupN`, the spec-ABI and
/// proven-`this` suffixes), so a user-derived symbol component can never
/// forge a generated symbol. Never emit `$` from this function or from
/// [`sanitize_member`].
///
/// NOTE: this mapping is *lossy* — every special character collapses to `_`,
/// so distinct inputs can share an output. That is fine for the module-prefix
/// and static-field components (whose values are recorded once and re-derived
/// identically at every reference site), but NOT for class/method name
/// components, where distinct private names like `#$`, `#_`, `#℘` would all
/// collapse to the same `perry_method_…` symbol and clang would reject the
/// module with `invalid redefinition of function`. Those components use the
/// injective [`sanitize_member`] instead. Keep `sanitize` byte-for-byte stable:
/// changing it desyncs cross-module symbol references (a module's prefix is
/// `sanitize(module_name)` at the definition site and must match the prefix the
/// importing module re-derives).
pub(crate) fn sanitize(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.chars()
        .next()
        .map(|c| c.is_ascii_digit())
        .unwrap_or(false)
    {
        s.insert(0, '_');
    }
    s
}

/// Injective variant of [`sanitize`] for the class-name and method-name
/// components of `perry_method_*` / `perry_static_*` symbols.
///
/// Names made up entirely of `[A-Za-z0-9_]` are returned IDENTICAL to what
/// `sanitize` produces (only a leading digit is `_`-prefixed), so every
/// ordinary method/class symbol is byte-for-byte unchanged. Names containing
/// any character outside `[A-Za-z0-9_]` — chiefly private member names (`#$`,
/// `#℘`, `#\u{6F}`, ZWJ/ZWNJ escapes) — are escaped to an unambiguous form
/// (`u_` tag + `_<hex>_` per non-alphanumeric character) so distinct source
/// names always yield distinct symbols. `sanitize` collapsed all of these to a
/// single `_`, so `#$`, `#_` and `#℘` mangled to the same symbol and clang
/// rejected the module with `invalid redefinition of function`.
///
/// Must be applied at BOTH the definition site and every reference site for a
/// given symbol component, or the symbols desync and the linker fails.
///
/// Like [`sanitize`], the output is strictly `[A-Za-z0-9_]` — `$` is reserved
/// for generated suffixes and must never appear here (issue #6927).
pub(super) fn sanitize_member(name: &str) -> String {
    let is_plain = name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if is_plain {
        // Byte-identical to `sanitize` for plain names (incl. leading-digit fix).
        return sanitize(name);
    }
    // A plain (pure-`[A-Za-z0-9_]`) name never reaches this branch, so it can
    // never collide with an escaped name: every escaped name carries a
    // `_<hex>_` group a plain name cannot reproduce.
    let mut s = String::from("u_");
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            s.push(c);
        } else {
            s.push('_');
            s.push_str(&format!("{:x}", c as u32));
            s.push('_');
        }
    }
    s
}

/// Reserve the keys-global symbol for one source class. Keep this single
/// implementation shared by capability harvesting and module emission: both
/// passes must assign collision suffixes in the same source order or a
/// harvested ShapeId external can name a global the producer never defines.
pub(crate) fn unique_class_keys_global(
    module_prefix: &str,
    class_name: &str,
    used: &mut std::collections::HashSet<String>,
) -> String {
    let base = format!(
        "perry_class_keys_{}__{}",
        module_prefix,
        sanitize(class_name)
    );
    if used.insert(base.clone()) {
        return base;
    }
    let mut suffix = 1u32;
    loop {
        let candidate = format!("{base}_{suffix}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
        suffix += 1;
    }
}

/// Host default triple.
/// Host-default LLVM target triple. Used when `CompileOptions.target`
/// is `None`. Also re-exposed via `pub(crate)` so `linker.rs` can pin
/// clang's `-target` even on host builds — without that pin a clang
/// whose own default triple is GNU/MinGW silently overrides the IR's
/// stated msvc triple and emits a `__main` libgcc reference that
/// lld-link/link.exe can't resolve. (The bug used to surface as
/// `LNK2019: unresolved external symbol __main referenced in
/// function main` even though the .ll says `target triple =
/// "x86_64-pc-windows-msvc"`.)
pub(crate) fn default_target_triple() -> String {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "arm64-apple-macosx15.0.0".to_string()
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "x86_64-apple-macosx15.0.0".to_string()
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "x86_64-unknown-linux-gnu".to_string()
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        "aarch64-unknown-linux-gnu".to_string()
    } else if cfg!(all(target_os = "windows", target_arch = "aarch64")) {
        "aarch64-pc-windows-msvc".to_string()
    } else if cfg!(target_os = "windows") {
        "x86_64-pc-windows-msvc".to_string()
    } else {
        "arm64-apple-macosx15.0.0".to_string()
    }
}

/// Map a Perry `--target <name>` string to the LLVM triple used by
/// `clang -target <triple>` / `llc -mtriple=<triple>`. The short
/// names are the public `--target` surface exposed by the CLI;
/// returning `None` leaves the triple to the host default.
///
/// Supported:
///  * `ios`, `ios-simulator`           → aarch64-apple-ios
///  * `visionos`, `visionos-simulator` → arm64-apple-xros1.0{,-simulator}
///  * `watchos`                        → aarch64-apple-watchos (arm64, S9+ / watchOS 26)
///  * `watchos-simulator`              → arm64-apple-watchos10.0-simulator
///  * `tvos`, `tvos-simulator`         → aarch64-apple-tvos
///  * `android`                        → aarch64-unknown-linux-android
///  * `linux` (x86_64 alias)           → x86_64-unknown-linux-gnu
///  * `linux-aarch64`                  → aarch64-unknown-linux-gnu
///  * `linux-musl` (x86_64 alias)      → x86_64-unknown-linux-musl (fully static)
///  * `linux-aarch64-musl`             → aarch64-unknown-linux-musl (fully static)
///  * `macos` (aarch64 alias)          → arm64-apple-macosx15.0.0
///  * `macos-x86_64`                   → x86_64-apple-macosx15.0.0
///  * `windows`                        → x86_64-pc-windows-msvc
///  * anything else                    → None (use host default)
pub fn resolve_target_triple(name: &str) -> Option<String> {
    match name {
        "ios" => Some("aarch64-apple-ios".to_string()),
        "ios-simulator" => Some("arm64-apple-ios17.0-simulator".to_string()),
        "visionos" => Some("arm64-apple-xros1.0".to_string()),
        "visionos-simulator" => Some("arm64-apple-xros1.0-simulator".to_string()),
        // arm64_32 (Series 4-8 / SE) when opted in via PERRY_WATCHOS_ARM64_32;
        // otherwise arm64 (S9+). Sets the arch of the emitted TS object files,
        // which must match the runtime/native-lib/link triples.
        "watchos" if std::env::var("PERRY_WATCHOS_ARM64_32").is_ok() => {
            Some("arm64_32-apple-watchos".to_string())
        }
        "watchos" => Some("aarch64-apple-watchos".to_string()),
        "watchos-simulator" => Some("arm64-apple-watchos10.0-simulator".to_string()),
        "tvos" => Some("aarch64-apple-tvos".to_string()),
        "tvos-simulator" => Some("arm64-apple-tvos17.0-simulator".to_string()),
        "harmonyos" => Some("aarch64-unknown-linux-ohos".to_string()),
        "harmonyos-simulator" => Some("x86_64-unknown-linux-ohos".to_string()),
        "android" => Some("aarch64-unknown-linux-android".to_string()),
        // #5742: Android x86_64 (emulator / x86_64 devices). Without this arm
        // `resolve_target_triple` returned None for `--target android-x86_64`,
        // so codegen fell back to the host triple (e.g. x86_64-pc-windows-msvc)
        // and emitted COFF objects, which the Android linker rejects with
        // `unknown file type`. Mirrors the `-unknown-` LLVM-triple form of the
        // arm64 `android` arm above (cf. harmonyos / harmonyos-simulator).
        "android-x86_64" => Some("x86_64-unknown-linux-android".to_string()),
        // Wear OS is Android-on-a-watch: same arm64 Android object format.
        "wearos" => Some("aarch64-unknown-linux-android".to_string()),
        "linux" => Some("x86_64-unknown-linux-gnu".to_string()),
        "linux-aarch64" => Some("aarch64-unknown-linux-gnu".to_string()),
        // musl targets — fully static binaries that run on Lambda
        // provided.al2023, scratch/distroless containers, Cloud Run, etc.
        // (no glibc loader dependency). See link/platform_cmd.rs for the
        // `-static` musl link path and #4826.
        "linux-musl" | "linux-x86_64-musl" => Some("x86_64-unknown-linux-musl".to_string()),
        "linux-aarch64-musl" => Some("aarch64-unknown-linux-musl".to_string()),
        "macos" => Some("arm64-apple-macosx15.0.0".to_string()),
        "macos-x86_64" => Some("x86_64-apple-macosx15.0.0".to_string()),
        "windows" | "windows-winui" if cfg!(target_os = "windows") => Some(default_target_triple()),
        "windows" | "windows-winui" | "windows-x86_64" => {
            Some("x86_64-pc-windows-msvc".to_string())
        }
        "windows-aarch64" | "windows-arm64" => Some("aarch64-pc-windows-msvc".to_string()),
        _ => None,
    }
}

/// True for macOS triples only (`*-apple-macosx*` LLVM-style, or
/// `*-apple-darwin*` rustc-style when a raw triple is passed through).
/// Deliberately false for every other Apple platform (`apple-ios`,
/// `apple-tvos`, `apple-xros`, `apple-watchos`): the `.app` CWD fix in
/// `perry_macos_bundle_chdir` is macOS-only, and emitting the call on
/// non-macOS targets makes their links depend on the runtime archive
/// carrying a macOS-only symbol (#4856).
pub(super) fn is_macos_triple(triple: &str) -> bool {
    triple.contains("apple-macosx") || triple.contains("apple-darwin")
}

pub(super) fn emit_buffer_alias_metadata(llmod: &mut LlModule, count: u32) {
    if count == 0 {
        return;
    }
    // Shared domain.
    llmod.add_metadata_line("!100 = distinct !{!100}".to_string());
    // Per-buffer scope nodes.
    for i in 0..count {
        let sid = 101 + i;
        llmod.add_metadata_line(format!("!{} = distinct !{{!{}, !100}}", sid, sid));
    }
    // Single-element alias-scope lists (one per buffer).
    for i in 0..count {
        let list_id = 201 + i;
        let scope_id = 101 + i;
        llmod.add_metadata_line(format!("!{} = !{{!{}}}", list_id, scope_id));
    }
    // Noalias lists: for buffer i, every *other* buffer's scope.
    for i in 0..count {
        let list_id = 301 + i;
        let others: Vec<String> = (0..count)
            .filter(|j| *j != i)
            .map(|j| format!("!{}", 101 + j))
            .collect();
        if others.is_empty() {
            // Single buffer: empty noalias set — LLVM accepts `!{}` but
            // it's a no-op. Still emit so `!noalias !{N}` references resolve.
            llmod.add_metadata_line(format!("!{} = !{{}}", list_id));
        } else {
            llmod.add_metadata_line(format!("!{} = !{{{}}}", list_id, others.join(", ")));
        }
    }
}

pub(super) fn register_module_globals_as_gc_roots(
    ctx: &mut crate::expr::FnCtx<'_>,
    module_globals: &HashMap<u32, String>,
) {
    // Sort by id for deterministic emit order (helps with diff-testing
    // the generated IR and matches the existing `class_keys` pattern).
    let mut entries: Vec<(&u32, &String)> = module_globals.iter().collect();
    entries.sort_by_key(|(id, _)| **id);
    for (_, global_name) in entries {
        let addr = ctx.block().ptrtoint(&format!("@{}", global_name), I64);
        ctx.block()
            .call_void("js_gc_register_global_root", &[(I64, &addr)]);
    }

    // Static class-field globals (`@perry_static_<mod>__<Class>__<field>`)
    // hold NaN-boxed any-values just like module globals but were never
    // registered (2026-07-02 audit P0 — byte-for-byte the #5042 class-keys
    // shape): the global's copy was never rewritten on evacuation, and when
    // the write-site class-id lookup misses it is the ONLY reference, so the
    // value was collectable while `C.field` still read it. The map can hold
    // one global under several keys (class name + import alias) — dedupe.
    // Cross-module duplicates (defining module + importers both register the
    // same linked address) are harmless: GLOBAL_ROOTS tolerates duplicate
    // entries (double mark/rewrite is idempotent).
    let static_names: std::collections::BTreeSet<&String> =
        ctx.static_field_globals.values().collect();
    for global_name in static_names {
        let addr = ctx.block().ptrtoint(&format!("@{}", global_name), I64);
        ctx.block()
            .call_void("js_gc_register_global_root", &[(I64, &addr)]);
    }
}

/// Issue #100: emit the IR that populates this module's
/// `@__perry_ns_<module_prefix>` global from the resolved namespace
/// entry list. Called at the end of `__perry_init_<prefix>` (or `main`
/// for the entry module) AFTER the module's top-level statements have
/// finished — at that point every local export's binding is set, and
/// every dependency's `__init` has already run (topo-sort guarantees
/// `ExportAll` / `ReExport` sources are initialised first), so
/// cross-module getters are safe to call.
///
/// The IR sequence per call:
///
///   1. Alloca three parallel stack arrays sized `[N x ?]` — keys (ptr),
///      key_lens (i32), values (double).
///   2. For each entry i in `namespace_entries`:
///      - Store `getelementptr inbounds [L x i8], ptr @.strK, i64 0, i64 0`
///        into `keys[i]` and `L` into `key_lens[i]`.
///      - Compute the value JSValue per `NamespaceEntryKind` and store
///        into `values[i]`.
///   3. Call `js_create_namespace(N, ptr keys, ptr key_lens, ptr values)`.
///   4. Store the result into `@__perry_ns_<module_prefix>`.
///
/// Always emits the `js_create_namespace` call + store, even when
/// `entries` is empty. This is required for Issue #842 (side-effect-only
/// dynamic-import targets — no exports, but the consumer still needs a
/// non-NaN `@__perry_ns_<prefix>` to load). The runtime tolerates
/// `n == 0` and returns an empty NaN-boxed object. The caller is
/// responsible for ensuring `key_globals.len() == entries.len()`.
pub(super) fn emit_namespace_populator(
    ctx: &mut crate::expr::FnCtx<'_>,
    entries: &[NamespaceEntry],
    key_globals: &[(String, usize)],
    module_prefix: &str,
) {
    debug_assert_eq!(entries.len(), key_globals.len());
    // Issue #842: side-effect-only dynamic-import targets land here
    // with `entries.is_empty()`. The runtime `js_create_namespace`
    // tolerates `n == 0` and returns a fresh empty object — exactly
    // what an export-less module's namespace should look like. We
    // still alloca minimum-size buffers (`[1 x ?]`) and pass the
    // pointers + n=0 so the runtime never dereferences them; the
    // per-entry loop simply doesn't execute.
    let n = entries.len();
    let buf_len = n.max(1);
    let blk = ctx.block();

    // Alloca the three parallel buffers.
    let keys_buf = blk.next_reg();
    blk.emit_raw(format!("{} = alloca [{} x ptr]", keys_buf, buf_len));
    let lens_buf = blk.next_reg();
    blk.emit_raw(format!("{} = alloca [{} x i32]", lens_buf, buf_len));
    let vals_buf = blk.next_reg();
    blk.emit_raw(format!("{} = alloca [{} x double]", vals_buf, buf_len));

    // #7210 (2): `vals_buf` is a plain stack alloca, not a shadow slot the
    // collector scans. Each entry's value is a NaN-boxed JSValue that can be
    // a real GC pointer (a closure singleton, a nested namespace object, a
    // re-exported var read through an arbitrary getter), and materialising
    // entry i+1 (`js_closure_alloc_singleton`, or calling an exported
    // getter) can allocate -- so storing entry i into `vals_buf` and then
    // moving on left it an unrooted stack copy while later entries were
    // computed. Root every value as it is produced, in one `RootedGroup` for
    // the whole populator, and defer every `vals_buf` store to a second pass
    // that runs back-to-back with the `js_create_namespace` call below —
    // nothing in that second pass can collect, so the buffer is guaranteed
    // fresh at the moment the runtime reads it.
    let mut handles = Vec::with_capacity(n);
    crate::rooting::with_rooted_group(ctx, buf_len, |ctx, group| {
        // Per-entry: store key ptr + len, and root the value.
        for (i, entry) in entries.iter().enumerate() {
            let (key_global, key_len) = &key_globals[i];
            let idx_str = format!("{}", i);
            let blk = ctx.block();

            // keys[i] = @<key_global> as ptr
            let key_slot = blk.gep(PTR, &keys_buf, &[(I64, &idx_str)]);
            blk.store(PTR, &format!("@{}", key_global), &key_slot);

            // key_lens[i] = byte_len
            let len_slot = blk.gep(I32, &lens_buf, &[(I64, &idx_str)]);
            blk.store(I32, &format!("{}", key_len), &len_slot);

            // Materialise the value per kind. We drop the `blk` borrow so
            // each sub-emission can re-borrow ctx mutably for runtime calls
            // / declares; then root it in this scope's group.
            let val_str = match &entry.kind {
                NamespaceEntryKind::LocalVar { global_name } => {
                    ctx.block().load(DOUBLE, &format!("@{}", global_name))
                }
                NamespaceEntryKind::LocalFunction { wrap_symbol } => {
                    let blk = ctx.block();
                    let handle = blk.call(
                        I64,
                        "js_closure_alloc_singleton",
                        &[(PTR, &format!("@{}", wrap_symbol))],
                    );
                    crate::expr::nanbox_pointer_inline(blk, &handle)
                }
                NamespaceEntryKind::LocalClass { class_id } => {
                    // INT32-tagged class-id NaN-box: 0x7FFE_0000_0000_0000 |
                    // (class_id & 0xFFFFFFFF). Matches `Expr::ClassRef`.
                    let bits = crate::nanbox::INT32_TAG | (*class_id as u64 & 0xFFFF_FFFF);
                    crate::nanbox::double_literal(f64::from_bits(bits))
                }
                NamespaceEntryKind::ForeignVar {
                    source_prefix,
                    source_local,
                } => {
                    let getter = format!("perry_fn_{}__{}", source_prefix, sanitize(source_local));
                    ctx.pending_declares.push((getter.clone(), DOUBLE, vec![]));
                    ctx.block().call(DOUBLE, &getter, &[])
                }
                NamespaceEntryKind::ForeignFunction {
                    source_prefix,
                    source_local,
                    param_count,
                } => {
                    // Function-shaped re-exports must materialize a function
                    // value, not call the function while building the namespace.
                    // Source modules emit `__perry_wrap_perry_fn_<src>__<name>`
                    // for every user function; hand that wrapper to the same
                    // singleton allocator used by local function exports.
                    let wrapper_name = format!(
                        "__perry_wrap_perry_fn_{}__{}",
                        source_prefix,
                        // Function bodies and their closure wrappers use the
                        // injective function-name mangler. Plain names are
                        // unchanged; `$constructor` and similar exports must
                        // not be collapsed to `_constructor` here (#7964).
                        sanitize_member(source_local)
                    );
                    let arity = (*param_count).min(16);
                    let mut wrapper_params: Vec<crate::types::LlvmType> = vec![I64];
                    wrapper_params.extend(std::iter::repeat_n(DOUBLE, arity));
                    ctx.pending_declares
                        .push((wrapper_name.clone(), DOUBLE, wrapper_params));
                    let blk = ctx.block();
                    let handle = blk.call(
                        I64,
                        "js_closure_alloc_singleton",
                        &[(PTR, &format!("@{}", wrapper_name))],
                    );
                    crate::expr::nanbox_pointer_inline(blk, &handle)
                }
                NamespaceEntryKind::NestedNamespace { source_prefix } => ctx
                    .block()
                    .load(DOUBLE, &format!("@__perry_ns_{}", source_prefix)),
                NamespaceEntryKind::NativeNamespace { specifier } => {
                    let name = specifier.strip_prefix("node:").unwrap_or(specifier);
                    let name_idx = ctx.strings.intern(name);
                    let name_global = format!("@{}", ctx.strings.entry(name_idx).bytes_global);
                    let name_len = name.len().to_string();
                    let blk = ctx.block();
                    if let Some(install) = crate::nm_install::nm_install_symbol(name) {
                        blk.call_void(install, &[]);
                    }
                    blk.call(
                        DOUBLE,
                        "js_create_native_module_namespace",
                        &[(PTR, &name_global), (I64, &name_len)],
                    )
                }
            };

            handles.push(group.adopt_emitted(ctx, crate::rooting::Repr::Boxed, &val_str, true));
        }

        // Flush pass: re-read each value from its root -- picking up any
        // relocation the loop above caused -- and store it into `vals_buf`.
        // No call runs between these stores and the `js_create_namespace`
        // call that follows, so every slot the runtime reads is live.
        for (i, handle) in handles.iter().enumerate() {
            let idx_str = format!("{}", i);
            let fresh = group.reread_emitted(ctx, *handle);
            let blk = ctx.block();
            let val_slot = blk.gep(DOUBLE, &vals_buf, &[(I64, &idx_str)]);
            blk.store(DOUBLE, &fresh, &val_slot);
        }
        anyhow::Ok(())
    })
    .expect("emit_namespace_populator's rooted group body is infallible");

    // Call `js_create_namespace(n, keys, key_lens, values)` and store
    // the result into the namespace global. The result is a NaN-boxed
    // POINTER_TAG ObjectHeader; the global is already GC-rooted by
    // `register_module_globals_as_gc_roots` is NOT — namespace globals
    // aren't in `module_globals`. Register the address as a root here
    // so the object survives subsequent GC cycles.
    let n_str = format!("{}", n);
    let blk = ctx.block();
    let result = blk.call(
        DOUBLE,
        "js_create_namespace",
        &[
            (I32, &n_str),
            (PTR, &keys_buf),
            (PTR, &lens_buf),
            (PTR, &vals_buf),
        ],
    );
    let ns_name = format!("__perry_ns_{}", module_prefix);
    crate::expr::emit_root_nanbox_store_on_block(blk, &result, &format!("@{}", ns_name));
    let addr_i64 = blk.ptrtoint(&format!("@{}", ns_name), I64);
    blk.call_void("js_gc_register_global_root", &[(I64, &addr_i64)]);
}

#[cfg(test)]
mod sanitize_tests {
    use super::{sanitize, sanitize_member, scoped_fn_name, scoped_method_name};

    /// Issue #6927: the generated-clone namespace (`{public}$<suffix>`) is
    /// unforgeable ONLY because these two functions never emit `$`. If either
    /// ever lets a `$` through, a user member could compose a public symbol
    /// equal to a generated clone symbol and silently usurp it
    /// (`deduped_function_refs` keeps the first definition).
    #[test]
    fn sanitize_never_emits_the_reserved_generated_suffix_separator() {
        for hostile in [
            "foo$generic",
            "$dup1",
            "a$b$c",
            "foo__generic", // old forgeable spelling — plain, passes through, harmless now
            "#$",
            "℘$typed_f64",
        ] {
            assert!(
                !sanitize(hostile).contains('$'),
                "sanitize({hostile:?}) leaked a `$`: {:?}",
                sanitize(hostile)
            );
            assert!(
                !sanitize_member(hostile).contains('$'),
                "sanitize_member({hostile:?}) leaked a `$`: {:?}",
                sanitize_member(hostile)
            );
        }
        // And therefore no composed public symbol contains one either.
        assert!(!scoped_fn_name("m", "add$typed_f64").contains('$'));
        assert!(!scoped_method_name("m", "C$x", "foo$generic").contains('$'));
    }
}

#[cfg(test)]
mod guarded_specialization_preinline_tests {
    use super::{
        guarded_specialization_fits_preinline_budget, GUARDED_SPECIALIZATION_PREINLINE_MAX_IR_BYTES,
    };

    #[test]
    fn generated_ir_budget_is_inclusive_and_bounded() {
        assert!(guarded_specialization_fits_preinline_budget(
            GUARDED_SPECIALIZATION_PREINLINE_MAX_IR_BYTES
        ));
        assert!(!guarded_specialization_fits_preinline_budget(
            GUARDED_SPECIALIZATION_PREINLINE_MAX_IR_BYTES + 1
        ));
    }
}

#[cfg(test)]
mod resolve_target_triple_tests {
    use super::resolve_target_triple;

    #[test]
    fn android_targets_resolve_to_android_elf_triples() {
        // #5742: `android-x86_64` must resolve (it previously returned None and
        // codegen fell back to the host triple, emitting COFF the Android
        // linker rejects). Both Android arms use the `-unknown-linux-android`
        // LLVM form so the emitted objects are ELF for the right arch.
        assert_eq!(
            resolve_target_triple("android").as_deref(),
            Some("aarch64-unknown-linux-android")
        );
        assert_eq!(
            resolve_target_triple("android-x86_64").as_deref(),
            Some("x86_64-unknown-linux-android")
        );
        // Unknown targets still fall through to None.
        assert_eq!(resolve_target_triple("android-mips"), None);
    }

    #[test]
    fn explicit_windows_targets_resolve_to_coff_triples() {
        assert_eq!(
            resolve_target_triple("windows-x86_64").as_deref(),
            Some("x86_64-pc-windows-msvc")
        );
        for target in ["windows-aarch64", "windows-arm64"] {
            assert_eq!(
                resolve_target_triple(target).as_deref(),
                Some("aarch64-pc-windows-msvc")
            );
        }
    }
}

#[cfg(test)]
mod native_roots_target_tests {
    use super::*;

    /// The default is "native roots where the runtime can walk, shadow stack
    /// where it cannot". If this predicate is LOOSER than `gc_map`'s refusals,
    /// the compile fails outright for those targets instead of falling back —
    /// so these two lists must stay in agreement, and this test is the pin.
    #[test]
    fn native_roots_default_matches_the_targets_gc_map_will_emit_for() {
        for triple in [
            "arm64-apple-macosx",
            "aarch64-apple-darwin",
            "aarch64-apple-ios",
            "aarch64-unknown-linux-gnu",
            "aarch64-unknown-linux-musl",
            "x86_64-apple-darwin",
            "x86_64-unknown-linux-gnu",
            "x86_64-pc-windows-msvc",
        ] {
            set_native_roots_for_target(triple);
            assert!(
                rs4gc_enabled(),
                "{triple} has a working walker — native roots should be the default"
            );
        }

        for triple in [
            // ILP32: 32-bit pointers, and the runtime's map loader is gated to
            // 64-bit Apple, so a map here would be read by nothing.
            "arm64_32-apple-watchos",
            // COFF + ARM64: no Windows walker for that register model, so no
            // frame would ever be visited.
            "aarch64-pc-windows-msvc",
            // Architectures with no walker at all.
            "riscv64gc-unknown-linux-gnu",
            "wasm32-unknown-unknown",
        ] {
            set_native_roots_for_target(triple);
            assert!(
                !rs4gc_enabled(),
                "{triple} has no walker — must fall back to the shadow stack, \
                 not hard-fail in gc_map"
            );
        }
    }

    /// An explicit `PERRY_RS4GC=1` must still reach `gc_map`'s refusal for an
    /// unsupported target. Turning that into a silent shadow-stack fallback
    /// would hide exactly what the arm was set to measure.
    #[test]
    fn the_target_default_is_a_default_not_a_veto() {
        set_native_roots_for_target("riscv64gc-unknown-linux-gnu");
        assert!(
            !rs4gc_enabled(),
            "unset env + unsupported target = fall back"
        );
        // The override path is env-driven and process-cached, so it is asserted
        // by the CI arms rather than re-read here; this pins the shape that the
        // target decision is consulted ONLY when there is no explicit answer.
        assert!(
            rs4gc_env_override().is_none() || rs4gc_env_override().is_some(),
            "override is a tri-state"
        );
    }
}
