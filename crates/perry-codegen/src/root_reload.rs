//! Re-read a shadow slot below every collection point that can run under it
//! (#7280).
//!
//! # The rule
//!
//! `docs/src/internals/gc-rooting-invariant.md` states it, and the second half
//! is the half that keeps getting dropped:
//!
//! > Any GC-managed value that is live across a collection point must be
//! > reachable from a root before that point. **A value read out of a root and
//! > held in an SSA register across a call is not rooted.** It is a copy, and
//! > the collector cannot see copies.
//!
//! A shadow slot has property (2) — the collector *rewrites* it on evacuation —
//! but a register loaded from it beforehand has only property (1), liveness.
//! The object survives, at a new address, and the register names the old one:
//!
//! ```llvm
//!   %r7 = alloca double
//!   store double %arg1, ptr %r7
//!   call void @js_shadow_slot_bind(i32 0, ptr %r7)   ; %r7 IS a root
//!   %r8 = load double, ptr %r7                       ; a COPY of the root
//!   %r9 = call double @build_schema()                 ; evacuates; rewrites %r7
//!   %r10 = call double @js_object_assign_one(double %r8, ...)  ; from-space
//! ```
//!
//! That is verbatim from `zod`'s object-spread lowering, and the very next
//! statement in the same function re-loads `%r7` — because a *fresh* lowering
//! of the same local emits a fresh load. The bug is never that codegen cannot
//! re-read the slot; it is that within one lowering the load happens once, at
//! the top, and the register is carried across everything that follows.
//!
//! # Why this is a pass and not another fix
//!
//! Because the shape is not in one lowering. Measured over the two IR corpora
//! (`scripts/gc_root_dominance_corpus.sh`, `scripts/gc_root_dominance_dep_corpus.sh`),
//! `--stale-registers --moving-only` reports 116 and 370 stale uses; 102 and 271
//! of them are this one shape, spread across the object-literal spread, the
//! inline class-field store, the numeric element store, `new`, property
//! define, `instanceof`, the closure-call family and more. `index_set.rs` alone
//! lowers `object` before `value` at fifteen separate arms. Fixing them one at
//! a time is what #7291 and #7299 did, correctly, without moving the acceptance
//! arm at all.
//!
//! So the fix is stated once, over the emitted CFG, where the property is
//! decidable:
//!
//! > For a load out of a shadow slot, every use that a collection point can
//! > reach re-reads the slot instead.
//!
//! # Why re-loading is sound, and when it is not
//!
//! Re-reading a slot is NOT unconditionally safe, and `rooting/temp_root.rs`'s
//! [`operand_is_reloadable`] documents exactly why: re-lowering a local reads
//! its value *now*, and "now" is after the later arguments have run — any of
//! which may have reassigned it. `new C(g, bump())` where `bump()` assigns `g`
//! must pass the pre-`bump()` `g`; re-lowering hands it the post-`bump()` one,
//! which is a miscompile rather than a rooting fix.
//!
//! That objection is about the SOURCE program's assignments, and at this level
//! they are visible: an assignment to a shadow-slotted local is a `store` to
//! its alloca in this function. (A local captured *and mutated* by a closure is
//! boxed instead — `boxed_vars.rs` — so it has no plain shadow slot to reload.)
//! So the rule carries its own side condition:
//!
//! > …unless a store to that slot can also run on the way, in which case the
//! > register is left alone.
//!
//! Both halves are path questions over the real CFG, not line order, and both
//! are answered here the same way `scripts/gc_root_dominance_check.py` answers
//! them — a back-edge round trip is not an intra-iteration path, because
//! re-entering the load's block re-executes the load and produces a different
//! dynamic value.
//!
//! Left alone means left as it is today: the 17 curated and 39 dependency-scale
//! residuals where the slot is reassigned in the window are genuinely the
//! temp-root case, and they keep their existing behaviour rather than getting a
//! wrong one.
//!
//! # Cost
//!
//! A reload is one load from a stack slot. Where it was not needed — no call
//! between the two points — LLVM's EarlyCSE/GVN forwards it away, because with
//! no intervening call nothing can clobber the alloca. Where it *was* needed
//! the intervening call is opaque, LLVM must keep the load, and that is the
//! whole point. So the pass is close to free exactly where it is redundant.
//!
//! # Two things a slot load is not, and both are the same bug (#7664)
//!
//! `--statepoints` (#7663) pointed this rule at the NATIVE root lowering — the
//! one that actually ships — and reported 21 `unrooted` hazards. Seventeen were
//! shapes this pass looked straight through, because the rule above is stated
//! over *the load's own register* and both shapes hold the value elsewhere:
//!
//! 1. **The root is a global, not an alloca.** A string literal lowers to
//!    `load double, ptr @<mod>_.str.N.handle`. That global IS a registered root
//!    (`js_gc_register_global_root`, `codegen/string_pool.rs`), so the string is
//!    never swept — and an evacuating cycle REWRITES it while a register loaded
//!    beforehand keeps the pre-move address. Same property-(2) failure as a
//!    shadow slot, same one-load fix. `expr/temp_root.rs` already calls this
//!    `OperandProtection::Reload` and its soundness argument carries verbatim:
//!    the only writer of a handle global in generated code is
//!    `__perry_init_strings_*`, so re-reading cannot observe a later
//!    assignment. (The store side-condition below still runs, so the init
//!    function is excluded by the analysis rather than by that argument.)
//!
//! 2. **The register that goes stale is DERIVED from the load, not the load.**
//!    `this.count++` lowers to
//!
//!    ```llvm
//!      %r8  = load double, ptr %r7           ; the root slot
//!      %r9  = bitcast double %r8 to i64
//!      %r10 = and i64 %r9, 281474976710655   ; the unmasked receiver
//!      %r14 = call double @js_object_get_field_by_name_f64(i64 %r10, i64 %r13)
//!      call void @js_object_set_field_by_name(i64 %r10, i64 %r13, double %r16)
//!    ```
//!
//!    `%r8`'s only use is the `bitcast`, which is ABOVE the collecting call, so
//!    the rule as originally stated had nothing to rewrite and this function
//!    took zero reloads. The value that crosses the GET is `%r10`. Under the
//!    native lowering the same shape reads
//!    `%rN.rs4i = ptrtoint ptr addrspace(1) %s to i64`: LLVM relocates the
//!    `addrspace(1)` pointer and rewrites its uses, but it cannot touch an
//!    `i64` copy, so the unmask is where the value leaves the tracked domain
//!    for good. #7280's zod-`clone` shape and #7240's literal shape, meeting in
//!    one function.
//!
//! So the rule is restated over the value's *derivation* rather than its
//! register:
//!
//! > For a value read out of a collector-rewritten location — a shadow slot or
//! > a string-handle global — and any value derived from it by pure bit ops,
//! > every use that a collection point can reach re-materialises the whole
//! > derivation instead.
//!
//! A derivation ("recipe") is only extended through instructions that are pure
//! functions of their operands (`TRANSPARENT_BIN`, `TRANSPARENT_CAST`) and
//! whose every register operand is already in the same single root's recipe.
//! That makes a recipe self-contained — a load plus bit ops on constants — so
//! it materialises at any point in the function with no dominance question, and
//! re-executing it is by construction the same function of the same root
//! evaluated against the address the collector wrote back. Anything else (a
//! phi, a call, an operand from a second root) is not extended through and is
//! left exactly as it is today.
//!
//! Cost is unchanged in kind: where the window is empty nothing is inserted;
//! where it is not, the intervening call is opaque so LLVM must keep the
//! reload — and the re-derived `bitcast`/`and` above it are pure and fold.
//!
//! [`operand_is_reloadable`]: crate::expr::temp_root
//! [`operand_is_reloadable`]: crate::rooting

use std::collections::{HashMap, HashSet, VecDeque};

use crate::function::LlFunction;
use crate::inst::LlInst;
use crate::types::LlvmType;

/// Runtime helpers that provably cannot allocate, run user code, or poll, and
/// which generated code emits often enough that treating them as collection
/// points would put a reload between every pair of instructions.
///
/// **This list is deliberately a SUBSET of `NONCOLLECTING` in
/// `scripts/gc_root_dominance_check.py`**, which is the authority and names the
/// runtime line proving each entry. A subset is the safe direction: a helper
/// missing from here is treated as collecting, which inserts a reload the
/// checker would not have demanded — a load, not a bug. A helper that is here
/// and is NOT in the checker's set would be the unsafe direction, so the
/// invariant to preserve on edit is one-way containment.
const NON_COLLECTING: &[&str] = &[
    // shadow stack / roots
    "js_shadow_slot_bind",
    "js_shadow_slot_set",
    "js_shadow_frame_enter",
    "js_shadow_frame_push",
    "js_shadow_frame_pop",
    "js_shadow_state_addr",
    "js_gc_temp_root_push",
    "js_gc_temp_root_get",
    "js_gc_temp_root_set",
    "js_gc_temp_root_truncate",
    // layout / barrier bookkeeping
    "js_gc_init_typed_shape_layout",
    "js_gc_declare_typed_shape_layout",
    "js_gc_forget_object_layout",
    // The two real slot-layout note exports. `js_gc_layout_note_slot` used to
    // stand here, and no such symbol has ever existed in the tree: the runtime
    // spells them `js_gc_note_slot_layout` (`gc/layout.rs:814`) and
    // `js_gc_note_slot_layout_aware` (`:833`), which is how
    // `gc_call_effects.rs` and every test names them. The typo was
    // safe-direction — an absent helper is treated as collecting, which costs
    // a reload rather than losing one — but it meant EVERY emitted note forced
    // a reload, including the one per guarded array element store
    // (`expr/index_set_guarded.rs`), which is #5094's hot path.
    "js_gc_note_slot_layout",
    // `_aware` is `js_gc_note_slot_layout` plus an early return when neither
    // the new nor the old bits are pointer-bearing, so it does strictly LESS
    // than the entry point above. Same reasoning the checker already accepts
    // for `declare` vs `init`.
    "js_gc_note_slot_layout_aware",
    "js_write_barrier",
    "js_write_barrier_root_nanbox",
    "perry_transition_cache_base",
    "js_transition_ic_note_hit",
    "js_transition_ic_spill_append",
    "js_write_barrier_slot",
    "js_write_barrier_slot_validated_parent",
    "js_gc_register_global_root",
    // pure value predicates / bit twiddling.
    //
    // `js_value_is_object`, `js_value_is_string` and `js_typeof_tag` used to sit
    // here, and — like `js_gc_layout_note_slot` above them — none is a symbol
    // this tree exports. Six such names had accumulated (the three here plus
    // `js_runtime_write_barrier_slot`, `js_typed_feedback_shape_guard` and
    // `js_typed_feedback_note`). They cost nothing on their own, because a name
    // that matches no callee never fires; what they cost is camouflage — they
    // made a REAL transposition indistinguishable from an aspirational entry.
    // `every_non_collecting_entry_is_a_real_runtime_export` now rejects both.
    "js_is_truthy",
    "js_nanbox_get_pointer",
    // inline-cache guards: pure reads
    "js_typed_feedback_closure_direct_call_guard",
    "js_closure_exact_func_guard",
    "js_object_own_method_cache_miss",
    // verified non-allocating bookkeeping stores/reads
    "js_closure_set_capture_bits",
    "js_closure_set_box_capture_ptr",
    "js_closure_get_capture_bits",
    "js_closure_set_capture_ptr",
    "js_closure_get_capture_ptr",
    "js_box_set_bits",
    "js_box_set_bits_trusted_no_barrier",
    "js_box_get_bits",
    "js_i32_box_set",
    "js_bool_box_set",
    "js_tdz_suppress_begin",
    "js_tdz_suppress_end",
    "js_array_note_numeric_write",
    "js_array_declare_all_pointer_elements",
    "js_array_live_head",
    "js_array_length",
    "js_object_mark_class",
    "js_class_object_pin_parent",
    "js_new_target_get",
    "js_new_target_set",
    "js_ctor_return_override",
];

/// Guard against a pathological function turning this pass into the compile's
/// bottleneck: the reachability walk is O(blocks) per slot load, so the product
/// is what matters. Above the cap the function keeps today's IR — the pass is
/// an improvement, not a correctness precondition, so declining is safe.
///
/// Sized from the corpora: the largest function in the dependency-scale corpus
/// (`zod`'s parse core) is ~1400 blocks with ~90 slot loads, an order of
/// magnitude under this.
const MAX_BLOCK_LOAD_PRODUCT: usize = 8_000_000;

/// How long a derivation may get before the pass declines to re-materialise it.
///
/// The masks this exists for are two steps (`bitcast` then `and`); the NaN-box
/// re-tag adds an `or`. Eight is well clear of that and bounds both the
/// fixpoint below and the instructions any one rewrite can insert.
const MAX_RECIPE: usize = 8;

/// Integer bit ops that are pure functions of their operands, so re-executing
/// one reproduces the derivation against whatever the collector last wrote.
///
/// Deliberately NOT arithmetic. `and`/`or`/`xor` are the NaN-box mask and
/// re-tag, which is the entire population this pass needs; admitting `add`
/// would be sound by the same argument but buys nothing today and widens what
/// a reader has to check.
const TRANSPARENT_BIN: &[&str] = &["and", "or", "xor"];

/// Conversions that are pure re-interpretations of a bit pattern.
const TRANSPARENT_CAST: &[&str] = &["bitcast", "ptrtoint", "inttoptr", "trunc", "zext", "sext"];

/// The runtime accessor a closure-capture READ lowers to: `js_closure_get_capture_bits(ptr,
/// idx)`. Every call site (#7725) reads its `ptr` from `try_current_closure_ptr_value` — itself
/// a load-of-a-shadow-slot recipe when `current_closure_slot` is bound (`codegen/closure.rs`) —
/// so a call whose `ptr` operand already belongs to a reloadable recipe is itself a reloadable
/// derivation of that SAME recipe, exactly like the `and`-mask step that already extends it.
/// Re-CALLING it below a collection point is sound under the same argument #7664's string-handle
/// reload uses: the closure struct is a first-class root (rooted at `current_closure_slot`,
/// relocated/rewritten on evacuation), so re-reading its capture array returns the
/// post-relocation value — UNLESS a generic or declared-box capture setter to
/// the same index ran in the window, which is the store side-condition below.
const CAPTURE_GET_CALLEE: &str = "js_closure_get_capture_bits";
/// The write half. Not itself collecting (`NON_COLLECTING` above), but its side effect on the
/// closure's capture slot must invalidate a [`CAPTURE_GET_CALLEE`] reload the same way a `store`
/// invalidates a shadow-slot one — `stores_to` carries a synthetic per-index key for exactly
/// that (#7725).
fn is_capture_set_callee(callee: &str) -> bool {
    matches!(
        callee,
        "js_closure_set_capture_bits" | "js_closure_set_box_capture_ptr"
    )
}

/// A synthetic "location" key for capture-slot index `idx` — never a real pointer token (no `%`
/// / `@` sigil), so it cannot collide with an actual shadow-slot register or handle-global name.
/// One key per (function, index): every capture read in a given function reads the SAME
/// closure's SAME index, since `current_closure_ptr_value` always re-derives from this
/// function's own `%this_closure`.
fn capture_slot_key(idx: &str) -> String {
    format!("$closure_capture:{idx}")
}

/// The literal capture-slot index from `js_closure_{get,set}_capture_bits`'s second argument, if
/// it is a compile-time constant. Every real emission site (`literals_vars.rs`, `closure.rs`,
/// `array_push.rs`, `instance_misc1.rs`, `lower_array_method.rs`, `expr/mod.rs`) formats a `u32`
/// directly, so this always succeeds in practice — but a register operand there would name no
/// fixed location, so `None` leaves the call opaque exactly like any other call whose result is
/// not a reloadable derivation, rather than guessing.
fn literal_capture_idx(args: &[(LlvmType, String)]) -> Option<&str> {
    let (_, idx) = args.get(1)?;
    (!idx.is_empty() && !idx.starts_with('%')).then_some(idx.as_str())
}

fn is_collecting(callee: &str) -> bool {
    if callee.starts_with("llvm.") {
        return false;
    }
    !NON_COLLECTING.contains(&callee)
}

/// Is `name` (no `@`) a string-literal handle global — `<mod>_.str.<N>.handle`?
///
/// Kept in step with `REWRITTEN_LOAD_RE`'s `strhandle` alternative in
/// `scripts/gc_root_dominance_check.py`, which is what reports the hazard this
/// recognises. Narrow on purpose: `@perry_global_*` is a module-level variable
/// the PROGRAM assigns, so re-reading it could observe a later assignment
/// instead of the value the call was given — that population needs rooting, not
/// reloading, and is deliberately not matched here.
fn is_string_handle_global(name: &str) -> bool {
    let rest = match name.strip_suffix(".handle") {
        Some(r) => r,
        None => return false,
    };
    match rest.rsplit_once("_.str.") {
        Some((head, digits)) => {
            !head.is_empty() && !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
        }
        None => false,
    }
}

/// A load's pointer operand, if it names a location whose current value may be
/// re-read anywhere in the function. Returns the operand token unchanged
/// (sigil included) so a slot and a global are one currency from here on —
/// which is also what makes the store side-condition cover both.
fn reloadable_ptr(ptr: &str, slots: &HashSet<String>) -> Option<String> {
    if let Some(reg) = ptr.strip_prefix('%') {
        return slots.contains(reg).then(|| ptr.to_string());
    }
    if let Some(name) = ptr.strip_prefix('@') {
        return is_string_handle_global(name).then(|| ptr.to_string());
    }
    None
}

/// One instruction, in the vocabulary this pass needs.
struct Facts {
    /// Register this instruction defines, without the `%`. Read by the
    /// derivation fixpoint (#7664), which needs the def side of the graph.
    result: Option<String>,
    /// Registers it reads, without the `%`. Only operands — never `result`.
    uses: Vec<String>,
    /// `Some((dst, ty, ptr))` for `dst = load ty, ptr <ptr>` where `<ptr>` is a
    /// reloadable location. `ptr` keeps its sigil: `%r7` for a shadow slot,
    /// `@…_.str.N.handle` for a string-literal handle global (#7664).
    load_of: Option<(String, LlvmType, String)>,
    /// Location this instruction stores into, sigil included — the same
    /// currency as `load_of`'s third field, so a store to a handle global
    /// disqualifies a reload exactly as a store to a slot does.
    stores_to: Option<String>,
    /// A pure bit op a derivation may be extended through (#7664).
    transparent: bool,
    /// `Some(key)` when this instruction is a [`CAPTURE_GET_CALLEE`] call with a literal index
    /// — `transparent` is also true, and `key` (from [`capture_slot_key`]) REPLACES the recipe's
    /// inherited `root_ptr` from this step onward, so the group's invalidation condition becomes
    /// "was this index SET in the window" rather than "was the closure-ptr slot stored to"
    /// (#7725). Kept separate from `transparent` because every other transparent op inherits its
    /// root unchanged; this is the one case that doesn't.
    capture_get_key: Option<String>,
    collecting: bool,
    /// A `phi` cannot have an instruction inserted before it. Neither can an
    /// inline block label (#7305's `invoke` continuation, emitted as a `Raw`
    /// `<name>:` line inside the same builder block): inserting there would put
    /// an instruction between a terminator and its label.
    is_phi: bool,
    /// Successor block labels, for the CFG.
    succs: Vec<String>,
}

/// Apply the reload rule to every function in `module`. Returns the number of
/// operands rewritten, which the unit tests assert on so a pass that silently
/// stops firing is a failure rather than a no-op.
pub(crate) fn apply_to_module(module: &mut crate::module::LlModule) -> usize {
    let mut total = 0;
    for f in module.functions_mut() {
        total += apply_to_function(f);
    }
    total
}

/// A value the pass can re-materialise: a load out of a collector-rewritten
/// location, or anything derived from one by pure bit ops (#7664).
struct Reloadable {
    /// The register it defines, without the `%`.
    reg: String,
    /// The location whose store invalidates the recipe, sigil included.
    root_ptr: String,
    /// The defining instructions to re-emit, in order. `recipe.last()` is
    /// always `pos`; `recipe[0]` is always the root load.
    recipe: Vec<(usize, usize)>,
}

pub(crate) fn apply_to_function(func: &mut LlFunction) -> usize {
    // NOT an early return on an empty bind set. A function with no shadow slot
    // can still load a string-handle global, and #7664's `main` cases are
    // exactly that; returning here is how the whole `strhandle` population
    // stayed invisible to a pass that was already computing everything it
    // needed to see it.
    let slots: HashSet<String> = func
        .reg_counter()
        .shadow_slot_allocas()
        .iter()
        .map(|s| s.trim_start_matches('%').to_string())
        .collect();

    let blocks = func.blocks_mut();
    if blocks.is_empty() {
        return 0;
    }

    let facts: Vec<Vec<Facts>> = blocks
        .iter()
        .map(|b| b.insts().iter().map(|i| facts_of(i, &slots)).collect())
        .collect();

    let label_index: HashMap<&str, usize> = blocks
        .iter()
        .enumerate()
        .map(|(i, b)| (b.label.as_str(), i))
        .collect();

    // ── The reloadable values ───────────────────────────────────────────────
    // Seeded with the root loads, then closed forward over pure bit ops. The
    // fixpoint is bounded by MAX_RECIPE rather than run to convergence: a
    // recipe longer than that is declined anyway, so a further round could not
    // admit anything.
    let mut values: Vec<Reloadable> = Vec::new();
    let mut by_reg: HashMap<String, usize> = HashMap::new();
    for (bi, fb) in facts.iter().enumerate() {
        for (ii, f) in fb.iter().enumerate() {
            if let Some((dst, _, ptr)) = &f.load_of {
                // A register defined twice is not SSA; if it happened, keep the
                // first and leave the rest alone rather than guessing.
                if by_reg.contains_key(dst) {
                    continue;
                }
                by_reg.insert(dst.clone(), values.len());
                values.push(Reloadable {
                    reg: dst.clone(),
                    root_ptr: ptr.clone(),
                    recipe: vec![(bi, ii)],
                });
            }
        }
    }
    if values.is_empty() {
        return 0;
    }
    for _ in 1..MAX_RECIPE {
        let mut grew = false;
        for (bi, fb) in facts.iter().enumerate() {
            for (ii, f) in fb.iter().enumerate() {
                if !f.transparent {
                    continue;
                }
                let dst = match &f.result {
                    Some(d) => d,
                    None => continue,
                };
                if by_reg.contains_key(dst) || f.uses.is_empty() {
                    continue;
                }
                // EVERY register operand must already be reloadable, and from
                // the SAME root. Anything else — an operand this pass cannot
                // reproduce, or a second root with its own store
                // side-condition — is left alone. One-sided by construction.
                let mut root: Option<&str> = None;
                let mut recipe: Vec<(usize, usize)> = Vec::new();
                let mut ok = true;
                for u in &f.uses {
                    let src = match by_reg.get(u) {
                        Some(&i) => &values[i],
                        None => {
                            ok = false;
                            break;
                        }
                    };
                    match root {
                        None => root = Some(&src.root_ptr),
                        Some(r) if r == src.root_ptr => {}
                        Some(_) => {
                            ok = false;
                            break;
                        }
                    }
                    for step in &src.recipe {
                        if !recipe.contains(step) {
                            recipe.push(*step);
                        }
                    }
                }
                let root = match (ok, root) {
                    (true, Some(r)) => r.to_string(),
                    _ => continue,
                };
                // #7725: a capture-GET call whose ptr operand is already reloadable extends the
                // SAME recipe, but its own store side-condition is "was THIS INDEX set", not
                // "was the closure-ptr slot stored to" — the ptr slot never is (it is written
                // once, at closure entry). Swap the key here so every step from this call
                // onward — including a further transparent cast like `bitcast i64 to double` —
                // is invalidated by the right condition.
                let root = f.capture_get_key.clone().unwrap_or(root);
                recipe.push((bi, ii));
                if recipe.len() > MAX_RECIPE {
                    continue;
                }
                by_reg.insert(dst.clone(), values.len());
                values.push(Reloadable {
                    reg: dst.clone(),
                    root_ptr: root,
                    recipe,
                });
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    if blocks.len().saturating_mul(values.len()) > MAX_BLOCK_LOAD_PRODUCT {
        return 0;
    }

    let succs: Vec<Vec<usize>> = facts
        .iter()
        .map(|fb| {
            let mut out: Vec<usize> = Vec::new();
            for f in fb {
                for s in &f.succs {
                    if let Some(&i) = label_index.get(s.as_str()) {
                        if !out.contains(&i) {
                            out.push(i);
                        }
                    }
                }
            }
            out
        })
        .collect();

    // Where a use must be rewritten, and with what. The recipe instructions are
    // CLONED here rather than referenced: they are re-read from `blocks` while
    // `facts` still indexes them, and the application phase below mutates those
    // very vectors.
    struct Rewrite {
        blk: usize,
        insn: usize,
        from: String,
        recipe: Vec<LlInst>,
    }
    let mut rewrites: Vec<Rewrite> = Vec::new();

    // ★ The walk is anchored at the ROOT LOAD, and every value derived from it
    // shares that one walk — including values DEFINED BELOW a store to the root.
    //
    // Anchoring a derived value at its own definition looks more precise and is
    // WRONG, because the recipe's validity is a property of the window since the
    // ROOT LOAD, not since the derivation. `main` in this shape:
    //
    //     %r22 = load ptr addrspace(1), ptr %r14   ; the root load
    //     %r23 = or  i64 %r22, POINTER_TAG
    //     store ptr addrspace(1) null, ptr %r14    ; ← the scope-end slot CLEAR
    //     %r25 = and i64 %r23, MASK                ; derived, defined BELOW it
    //     …
    //     call … @js_object_get_field_ic_miss(i64 %r25, …)
    //
    // A walk starting at `%r25` never sees the clear, so it re-materialised
    // `load %r14` at the use — reading a slot the program had just nulled, and
    // turning a class object's static read into `undefined`. Caught by an A/B
    // against the branch point on `test_gap_class_expr_identity`, not by the
    // dominance checker, which cannot see a value-correctness bug.
    //
    // Grouping by root load also puts the cost back at O(blocks × loads).
    //
    // Keyed on `(recipe[0], root_ptr)`, not `recipe[0]` alone — #7725's capture-GET extension is
    // the one case where a chain's `root_ptr` CHANGES partway through (the closure-ptr slot
    // becomes a synthetic per-index key once the derivation passes through
    // `js_closure_get_capture_bits`), so two sub-chains sharing one root LOAD can need two
    // different invalidation conditions. Before #7725 every member of a `recipe[0]` group had
    // the identical `root_ptr` by construction (it was always inherited, never overridden), so
    // this is additive: it can only split a group that would otherwise have mixed two
    // conditions under one, never change the grouping of any existing (non-capture) chain.
    let mut groups: HashMap<((usize, usize), String), Vec<usize>> = HashMap::new();
    for (i, v) in values.iter().enumerate() {
        groups
            .entry((v.recipe[0], v.root_ptr.clone()))
            .or_default()
            .push(i);
    }
    let mut group_keys: Vec<((usize, usize), String)> = groups.keys().cloned().collect();
    group_keys.sort_unstable();

    for key in group_keys {
        let members = &groups[&key];
        let (lb, li) = key.0;
        let slot = key.1;
        let by_use: HashMap<&str, usize> = members
            .iter()
            .map(|&m| (values[m].reg.as_str(), m))
            .collect();
        // Forward reachability from the root load, never re-entering its own
        // block: a back edge re-executes the load, so the value on the far side
        // is a different dynamic instance and not this one.
        //
        // `collect_in[b]` / `store_in[b]`: can a collecting call / a store to
        // `slot` have run on some path from the load to the TOP of block `b`?
        let mut collect_in = vec![false; facts.len()];
        let mut store_in = vec![false; facts.len()];
        let mut seen = vec![false; facts.len()];
        let mut queue: VecDeque<usize> = VecDeque::new();

        // Tail of the load's own block, from just after the load.
        let mut c = false;
        let mut s = false;
        for f in facts[lb].iter().skip(li + 1) {
            c |= f.collecting;
            s |= f.stores_to.as_deref() == Some(slot.as_str());
        }
        for &succ in &succs[lb] {
            if succ == lb {
                continue;
            }
            if !seen[succ] || (c && !collect_in[succ]) || (s && !store_in[succ]) {
                collect_in[succ] |= c;
                store_in[succ] |= s;
                seen[succ] = true;
                queue.push_back(succ);
            }
        }
        while let Some(b) = queue.pop_front() {
            let mut oc = collect_in[b];
            let mut os = store_in[b];
            for f in &facts[b] {
                oc |= f.collecting;
                os |= f.stores_to.as_deref() == Some(slot.as_str());
            }
            for &succ in &succs[b] {
                if succ == lb {
                    continue;
                }
                let grew = !seen[succ] || (oc && !collect_in[succ]) || (os && !store_in[succ]);
                if grew {
                    collect_in[succ] |= oc;
                    store_in[succ] |= os;
                    seen[succ] = true;
                    queue.push_back(succ);
                }
            }
        }

        for (ub, fb) in facts.iter().enumerate() {
            // Only blocks the load can reach without re-entering its own block,
            // plus the load's own block below the load.
            if ub != lb && !seen[ub] {
                continue;
            }
            let (mut c, mut s) = if ub == lb {
                (false, false)
            } else {
                (collect_in[ub], store_in[ub])
            };
            let start = if ub == lb { li + 1 } else { 0 };
            for (ui, f) in fb.iter().enumerate().skip(start) {
                if c && !s && !f.is_phi {
                    // One instruction can read several values of the same root
                    // — `js_object_set_field_by_name(recv, key, …)` when both
                    // came out of one slot. Each gets its own recipe; the
                    // application phase renames every operand of an instruction
                    // before inserting any of them.
                    let mut seen_here: Vec<&str> = Vec::new();
                    for u in &f.uses {
                        let m = match by_use.get(u.as_str()) {
                            Some(&m) => m,
                            None => continue,
                        };
                        if seen_here.contains(&u.as_str()) {
                            continue;
                        }
                        seen_here.push(u.as_str());
                        let recipe: Vec<LlInst> = values[m]
                            .recipe
                            .iter()
                            .map(|&(rb, ri)| blocks[rb].insts()[ri].clone())
                            .collect();
                        rewrites.push(Rewrite {
                            blk: ub,
                            insn: ui,
                            from: values[m].reg.clone(),
                            recipe,
                        });
                    }
                }
                c |= f.collecting;
                s |= f.stores_to.as_deref() == Some(slot.as_str());
            }
        }
    }

    if rewrites.is_empty() {
        return 0;
    }

    // Apply back-to-front within each block so earlier indices stay valid.
    rewrites.sort_by(|a, b| (a.blk, a.insn).cmp(&(b.blk, b.insn)).reverse());
    let n = rewrites.len();
    let counter = func.reg_counter_rc();
    // ★ Only the insertions AT OR ABOVE the post-init splice point move it.
    // Counting the ones below it too pushes the splice past the end of the
    // entry block, `to_ir` clamps it, and the whole post-init region — which
    // for a post-init-frame function contains `js_shadow_frame_enter` itself —
    // lands after every `js_shadow_slot_bind` in the body. See
    // `LlFunction::note_entry_block_insertions`; the symptom is indistinguishable
    // from the bug this pass exists to fix, which is how it was found.
    //
    // A rewrite inserts its whole RECIPE, not one load, so the count is in
    // instructions rather than in rewrites (#7664).
    let boundary = func.entry_init_boundary();
    let entry_inserts = match boundary {
        None => 0,
        Some(b) => rewrites
            .iter()
            .filter(|r| r.blk == 0 && r.insn <= b)
            .map(|r| r.recipe.len())
            .sum(),
    };
    {
        let blocks = func.blocks_mut();
        // One instruction can carry TWO stale operands — `js_object_assign_one`
        // takes a receiver and a value, and `index_set.rs` lowers `object`
        // before `value`, so both can be loads out of their own slots. Renaming
        // then inserting one at a time is wrong for that shape: the insert
        // shifts the target down, so the next `rename_operand` addresses the
        // reload just inserted, silently matches nothing, and leaves the second
        // operand stale while emitting a dead load. Rename EVERY operand of an
        // instruction first, then insert that instruction's reloads.
        let mut i = 0;
        while i < rewrites.len() {
            let (blk, insn) = (rewrites[i].blk, rewrites[i].insn);
            let mut j = i;
            while j < rewrites.len() && rewrites[j].blk == blk && rewrites[j].insn == insn {
                j += 1;
            }
            let insts = blocks[blk].insts_mut();
            let mut reloads: Vec<LlInst> = Vec::new();
            for r in &rewrites[i..j] {
                let (steps, fresh) = materialize(&r.recipe, &counter);
                rename_operand(&mut insts[insn], &r.from, fresh.trim_start_matches('%'));
                reloads.extend(steps);
            }
            // Insert after renaming, so every operand was addressed against the
            // original instruction. Order among the recipes is immaterial: each
            // defines its own fresh registers and reads only the root location.
            for reload in reloads.into_iter().rev() {
                insts.insert(insn, reload);
            }
            i = j;
        }
    }
    func.note_entry_block_insertions(entry_inserts);
    n
}

/// Re-emit a derivation with fresh registers, returning the instructions in
/// order and the register the last one defines.
///
/// A recipe is self-contained by construction — a load from the root location
/// plus pure bit ops whose every register operand is an earlier step — so the
/// only rewriting needed is step-to-step: each step's operands are renamed to
/// the fresh names of the steps it consumed. The root pointer (`%slot` or
/// `@…handle`) is not a step, is never in the map, and is therefore carried
/// through untouched, which is exactly what makes this a RE-READ.
fn materialize(
    recipe: &[LlInst],
    counter: &std::rc::Rc<crate::block::RegCounter>,
) -> (Vec<LlInst>, String) {
    let mut out: Vec<LlInst> = Vec::with_capacity(recipe.len());
    let mut renames: Vec<(String, String)> = Vec::with_capacity(recipe.len());
    let mut last = String::new();
    for step in recipe {
        let mut step = step.clone();
        for (old, new) in &renames {
            rename_operand(&mut step, old, new);
        }
        let old_dst = match inst_result(&step) {
            Some(d) => d,
            // Only loads and pure bit ops become recipe steps, and all three
            // define a register. Bail rather than emit a step whose result
            // nothing can name.
            None => return (Vec::new(), last),
        };
        let fresh = format!("%r{}", counter.next());
        set_inst_result(&mut step, &fresh);
        renames.push((old_dst, fresh.trim_start_matches('%').to_string()));
        last = fresh;
        out.push(step);
    }
    (out, last)
}

/// The register an instruction defines, without the `%`. Recipe-shaped
/// instructions only; everything else answers `None` and is declined.
fn inst_result(inst: &LlInst) -> Option<String> {
    let dst = match inst {
        LlInst::Raw(text) => {
            let (lhs, _) = text.split_once(" = ")?;
            let lhs = lhs.trim();
            if !lhs.starts_with('%') || lhs.contains(char::is_whitespace) {
                return None;
            }
            lhs
        }
        LlInst::Bin { dst, .. } | LlInst::Cast { dst, .. } | LlInst::Load { dst, .. } => dst,
        // #7725: a capture-GET call can now be a recipe step (see `CAPTURE_GET_CALLEE`), so
        // `materialize` needs to name and rename its result exactly like a load or a bit op.
        // `dst` is `None` only for a void call, which is never `transparent` and so never
        // reaches here.
        LlInst::Call { dst: Some(dst), .. } => dst,
        _ => return None,
    };
    Some(dst.trim_start_matches('%').to_string())
}

/// Point an instruction's result at `fresh` (which carries its `%`).
fn set_inst_result(inst: &mut LlInst, fresh: &str) {
    match inst {
        LlInst::Raw(text) => {
            if let Some((lhs, rhs)) = text.split_once(" = ") {
                let indent: String = lhs.chars().take_while(|c| c.is_whitespace()).collect();
                *text = format!("{indent}{fresh} = {rhs}");
            }
        }
        LlInst::Bin { dst, .. } | LlInst::Cast { dst, .. } | LlInst::Load { dst, .. } => {
            *dst = fresh.to_string();
        }
        LlInst::Call {
            dst: dst @ Some(_), ..
        } => {
            *dst = Some(fresh.to_string());
        }
        _ => {}
    }
}

fn facts_of(inst: &LlInst, slots: &HashSet<String>) -> Facts {
    let mut uses = Vec::new();
    let mut result = None;
    let mut load_of = None;
    let mut stores_to = None;
    let mut collecting = false;
    let mut is_phi = false;
    let mut transparent = false;
    let mut capture_get_key = None;
    let mut succs = Vec::new();

    let reg = |s: &str| -> Option<String> {
        s.strip_prefix('%')
            .map(|r| r.to_string())
            .filter(|r| !r.is_empty())
    };
    let use_op = |uses: &mut Vec<String>, s: &str| {
        if let Some(r) = reg(s) {
            uses.push(r);
        }
    };

    match inst {
        LlInst::Raw(text) => return raw_facts(text, slots),
        LlInst::Bin { dst, op, a, b, .. } => {
            result = reg(dst);
            use_op(&mut uses, a);
            use_op(&mut uses, b);
            transparent = TRANSPARENT_BIN.contains(op);
        }
        LlInst::FNeg { dst, a, .. } => {
            result = reg(dst);
            use_op(&mut uses, a);
        }
        LlInst::FCmp { dst, a, b, .. } | LlInst::ICmp { dst, a, b, .. } => {
            result = reg(dst);
            use_op(&mut uses, a);
            use_op(&mut uses, b);
        }
        LlInst::Alloca { dst, .. } => result = reg(dst),
        LlInst::Load { dst, ty, ptr, .. } => {
            result = reg(dst);
            use_op(&mut uses, ptr);
            if let (Some(d), Some(p)) = (reg(dst), reloadable_ptr(ptr, slots)) {
                load_of = Some((d, *ty, p));
            }
        }
        LlInst::Store { val, ptr, .. } => {
            use_op(&mut uses, val);
            use_op(&mut uses, ptr);
            stores_to = Some(ptr.clone());
        }
        LlInst::Cast { dst, op, v, .. } => {
            result = reg(dst);
            use_op(&mut uses, v);
            transparent = TRANSPARENT_CAST.contains(op);
        }
        LlInst::Select {
            dst, cond, a, b, ..
        } => {
            result = reg(dst);
            use_op(&mut uses, cond);
            use_op(&mut uses, a);
            use_op(&mut uses, b);
        }
        LlInst::Call {
            dst,
            callee,
            args,
            gc_leaf,
            ..
        } => {
            result = dst.as_deref().and_then(reg);
            for (_, v) in args {
                use_op(&mut uses, v);
            }
            collecting = !gc_leaf && is_collecting(callee);
            // #7725: the two capture-bits halves. GET extends a derivation like a transparent
            // bit op (see CAPTURE_GET_CALLEE); SET's side effect on the capture slot has to
            // invalidate that derivation the way a store does, via the shared `stores_to`
            // machinery.
            if callee == CAPTURE_GET_CALLEE {
                if let Some(idx) = literal_capture_idx(args) {
                    transparent = true;
                    capture_get_key = Some(capture_slot_key(idx));
                }
            } else if is_capture_set_callee(callee) {
                if let Some(idx) = literal_capture_idx(args) {
                    stores_to = Some(capture_slot_key(idx));
                }
            }
        }
        LlInst::CallIndirect {
            dst,
            fptr,
            args,
            gc_leaf,
            ..
        } => {
            result = reg(dst);
            use_op(&mut uses, fptr);
            for (_, v) in args {
                use_op(&mut uses, v);
            }
            collecting = !gc_leaf;
        }
        LlInst::AsmBarrier => {}
        LlInst::Br { label } => succs.push(label.clone()),
        LlInst::CondBr { cond, t, f } => {
            use_op(&mut uses, cond);
            succs.push(t.clone());
            succs.push(f.clone());
        }
        LlInst::Ret { val, .. } => use_op(&mut uses, val),
        LlInst::RetVoid | LlInst::Unreachable => {}
        LlInst::Gep { dst, ptr, idxs, .. } => {
            result = reg(dst);
            use_op(&mut uses, ptr);
            for (_, v) in idxs {
                use_op(&mut uses, v);
            }
        }
        LlInst::Phi { dst, pairs, .. } => {
            result = reg(dst);
            is_phi = true;
            for (v, _) in pairs {
                use_op(&mut uses, v);
            }
        }
    }

    Facts {
        result,
        uses,
        load_of,
        stores_to,
        transparent,
        capture_get_key,
        collecting,
        is_phi,
        succs,
    }
}

/// `Raw` is the escape hatch `emit_raw` writes through, so its facts come from
/// the rendered line. Everything here is one-sided towards doing nothing: an
/// operand form this does not recognise contributes no rewrite, and a call it
/// cannot name is treated as collecting.
fn raw_facts(text: &str, slots: &HashSet<String>) -> Facts {
    let mut uses = Vec::new();
    let mut result = None;
    let mut load_of = None;
    let mut stores_to = None;
    let mut succs = Vec::new();

    let body = text.trim_start();
    let (lhs, rhs) = match body.split_once(" = ") {
        Some((l, r)) if l.starts_with('%') && !l.contains(' ') => {
            result = Some(l.trim_start_matches('%').to_string());
            (Some(l), r)
        }
        _ => (None, body),
    };
    let _ = lhs;

    for tok in registers_in(rhs) {
        uses.push(tok);
    }
    // A bare `<label>:` line is an inline block header, not an instruction.
    // Treat it like a phi for insertion purposes — nothing may go above it.
    let is_label = rhs.ends_with(':') && !rhs.contains(' ');
    let is_phi = rhs.starts_with("phi ") || is_label;

    if rhs.starts_with("load ") && !rhs.contains("volatile") && !rhs.contains("atomic") {
        // `load <ty>, ptr %slot[, …]` or `load <ty>, ptr @…handle[, …]`
        if let Some((ty, rest)) = rhs["load ".len()..].split_once(", ptr ") {
            let ty = ty.trim();
            let ptr = rest
                .split(|c: char| c == ',' || c.is_whitespace())
                .next()
                .unwrap_or("");
            if let (Some(d), Some(p)) = (result.clone(), reloadable_ptr(ptr, slots)) {
                if let Some(t) = static_llvm_type(ty) {
                    load_of = Some((d, t, p));
                }
            }
        }
    }
    if rhs.starts_with("store ") {
        if let Some((_, ptr_rest)) = rhs.rsplit_once(", ptr ") {
            let ptr = ptr_rest
                .split(|c: char| c == ',' || c.is_whitespace())
                .next()
                .unwrap_or("");
            if !ptr.is_empty() {
                stores_to = Some(ptr.to_string());
            }
        }
    }
    // A pure bit op, in rendered form. Opcode-anchored at the start of the RHS
    // so `and`/`or` inside a callee name or a type cannot be mistaken for one.
    let mut transparent = rhs
        .split_once(' ')
        .map(|(op, _)| TRANSPARENT_BIN.contains(&op) || TRANSPARENT_CAST.contains(&op))
        .unwrap_or(false);
    let mut capture_get_key = None;
    // ★ `invoke` (#7305) is BOTH a call and a two-successor terminator, and it
    // has to be modelled as both. Missing the call half would classify a
    // throwing runtime helper's window as non-collecting and silently drop the
    // reloads inside a `try`; missing the successor half would hide the unwind
    // edge from the reachability walk.
    //
    // Perry emits `invoke … to label %cont unwind label %lpad` followed by an
    // INLINE continuation label in the same `LlBlock`, so one builder block can
    // hold several LLVM blocks. That is why the reload is placed at the USE and
    // not after the call: a load from the slot is valid wherever it is inserted
    // and reads whatever the collector last wrote, so it is correct on the
    // unwind edge and the normal edge alike — there is no "after the call"
    // position that has to be picked correctly.
    let is_invoke = rhs.starts_with("invoke ");
    if rhs.starts_with("br label %") {
        succs.push(rhs["br label %".len()..].trim().to_string());
    } else if rhs.starts_with("br i1 ") || rhs.starts_with("switch ") || is_invoke {
        for part in rhs.split("label %").skip(1) {
            succs.push(
                part.split(|c: char| c == ',' || c == ']' || c.is_whitespace())
                    .next()
                    .unwrap_or("")
                    .to_string(),
            );
        }
    }

    // Any call that is not a named non-collecting helper collects. `invoke` is
    // a call; it reaches this by name exactly like `call` does.
    let collecting = match rhs.find("call ").or(if is_invoke { Some(0) } else { None }) {
        None => false,
        Some(_) => match rhs.split_once('@') {
            Some((_, after)) => {
                let name: String = after
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '.' || *c == '$')
                    .collect();
                // #7725: neither capture-bits helper is in the EH nothrow allowlist (nothing
                // proves it can't reach `js_throw`), so a call inside a `try` lowers to `invoke`
                // and lands here rather than in `facts_of`'s typed `LlInst::Call` arm.
                //
                // The SET side is recorded regardless: it only feeds the invalidation
                // (`stores_to`) check, which is a fact about a real side effect and is sound
                // whether or not the call happens to be wrapped in `invoke`.
                //
                // The GET side is gated on `!is_invoke`: `materialize` re-emits a recipe step as
                // a plain mid-block instruction, and an `invoke` is a terminator — cloning one
                // in would both be invalid IR and re-target the ORIGINAL call's `%cont`/`%lpad`
                // labels from a second predecessor they don't expect. (In practice this call is
                // never seen here at all EXCEPT as an invoke, since every real emission site
                // goes through `LlBlock::call`'s typed `LlInst::Call` arm otherwise — so the
                // practical effect of this guard is "never", not "sometimes"; it is kept so a
                // future emit_raw call to this name stays correct rather than relying on that.)
                if name == CAPTURE_GET_CALLEE && !is_invoke {
                    if let Some(idx) = raw_call_literal_arg(rhs, &name, 1) {
                        transparent = true;
                        capture_get_key = Some(capture_slot_key(&idx));
                    }
                } else if is_capture_set_callee(&name) {
                    if let Some(idx) = raw_call_literal_arg(rhs, &name, 1) {
                        stores_to = Some(capture_slot_key(&idx));
                    }
                }
                is_collecting(&name)
            }
            // An indirect call, or inline asm. `asm sideeffect ""` emits
            // nothing; anything else through a function pointer is user code.
            None => !rhs.contains(" asm "),
        },
    };

    Facts {
        result,
        uses,
        load_of,
        stores_to,
        transparent,
        capture_get_key,
        collecting,
        is_phi,
        succs,
    }
}

/// [`literal_capture_idx`]'s analogue for a rendered `call`/`invoke` line: the value token of
/// `@callee(...)`'s argument at `index`, if it is a compile-time literal. `rhs` is the trimmed
/// instruction text (so `@callee(` always appears at most once — a nested call cannot appear as
/// a bare operand in this IR). Balanced-paren scanned rather than split on the first `)`, since a
/// function-pointer type operand elsewhere on the line can itself contain parens; returns `None`
/// on anything that does not confidently parse rather than guessing.
fn raw_call_literal_arg(rhs: &str, callee: &str, index: usize) -> Option<String> {
    let marker = format!("@{callee}(");
    let start = rhs.find(&marker)? + marker.len();
    let mut depth = 1usize;
    let mut end = None;
    for (i, c) in rhs[start..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(start + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let args_text = &rhs[start..end?];
    let arg = args_text.split(',').nth(index)?;
    let (_, val) = arg.trim().split_once(' ')?;
    let val = val.trim();
    (!val.is_empty() && !val.starts_with('%')).then(|| val.to_string())
}

/// The `LlvmType` alphabet is `&'static str`, so a `Raw` load can only be
/// reloaded when its type text is one of the known ones. Anything else (a
/// literal `[N x double]`, say) simply is not treated as a slot load.
fn static_llvm_type(ty: &str) -> Option<LlvmType> {
    Some(match ty {
        "double" => crate::types::DOUBLE,
        "i64" => crate::types::I64,
        "i32" => crate::types::I32,
        "i8" => crate::types::I8,
        "i1" => crate::types::I1,
        "ptr" => crate::types::PTR,
        _ => return None,
    })
}

/// Register names appearing as operands in a rendered instruction's RHS.
fn registers_in(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let start = i + 1;
            let mut j = start;
            while j < bytes.len() && is_reg_char(bytes[j]) {
                j += 1;
            }
            if j > start {
                out.push(text[start..j].to_string());
            }
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

fn is_reg_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || b == b'$'
}

/// Rewrite every operand occurrence of `%from` to `%to`, leaving the
/// instruction's own result alone.
fn rename_operand(inst: &mut LlInst, from: &str, to: &str) {
    let swap = |s: &mut String| {
        if s.len() == from.len() + 1 && s.starts_with('%') && &s[1..] == from {
            s.clear();
            s.push('%');
            s.push_str(to);
        }
    };
    match inst {
        LlInst::Raw(text) => *text = rename_in_text(text, from, to),
        LlInst::Bin { a, b, .. } => {
            swap(a);
            swap(b);
        }
        LlInst::FNeg { a, .. } => swap(a),
        LlInst::FCmp { a, b, .. } | LlInst::ICmp { a, b, .. } => {
            swap(a);
            swap(b);
        }
        LlInst::Alloca { .. } => {}
        LlInst::Load { ptr, .. } => swap(ptr),
        LlInst::Store { val, ptr, .. } => {
            swap(val);
            swap(ptr);
        }
        LlInst::Cast { v, .. } => swap(v),
        LlInst::Select { cond, a, b, .. } => {
            swap(cond);
            swap(a);
            swap(b);
        }
        LlInst::Call { args, .. } => {
            for (_, v) in args {
                swap(v);
            }
        }
        LlInst::CallIndirect { fptr, args, .. } => {
            swap(fptr);
            for (_, v) in args {
                swap(v);
            }
        }
        LlInst::AsmBarrier | LlInst::RetVoid | LlInst::Unreachable | LlInst::Br { .. } => {}
        LlInst::CondBr { cond, .. } => swap(cond),
        LlInst::Ret { val, .. } => swap(val),
        LlInst::Gep { ptr, idxs, .. } => {
            swap(ptr);
            for (_, v) in idxs {
                swap(v);
            }
        }
        LlInst::Phi { pairs, .. } => {
            for (v, _) in pairs {
                swap(v);
            }
        }
    }
}

/// Token-exact `%from` -> `%to` in a rendered line, skipping the LHS.
///
/// Boundary-checked: `%r1` must not rewrite inside `%r10`, which is the whole
/// reason this is not a `str::replace`.
fn rename_in_text(text: &str, from: &str, to: &str) -> String {
    let (prefix, rhs) = match text.split_once(" = ") {
        Some((l, r)) if l.trim_start().starts_with('%') && !l.trim().contains(' ') => {
            (format!("{} = ", l), r)
        }
        _ => (String::new(), text),
    };
    let mut out = String::with_capacity(text.len() + 8);
    out.push_str(&prefix);
    let bytes = rhs.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let start = i + 1;
            let mut j = start;
            while j < bytes.len() && is_reg_char(bytes[j]) {
                j += 1;
            }
            if &rhs[start..j] == from {
                out.push('%');
                out.push_str(to);
            } else {
                out.push_str(&rhs[i..j]);
            }
            i = j;
        } else {
            out.push(rhs.as_bytes()[i] as char);
            i += 1;
        }
    }
    out
}

/// Coverage for the reload/re-derivation rule (#7280/#7664/#7725). A
/// sibling file only because of the 2,000-line cap; `use super::*` gives it
/// the same view of this module as the block above.
#[cfg(test)]
#[path = "root_reload_tests.rs"]
mod tests;
