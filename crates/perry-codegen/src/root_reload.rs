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
//! Re-reading a slot is NOT unconditionally safe, and `expr/temp_root.rs`'s
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
//! [`operand_is_reloadable`]: crate::expr::temp_root

use std::collections::{HashMap, HashSet, VecDeque};

use crate::function::LlFunction;
use crate::inst::{LlInst, LoadFlavor};
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
    "js_gc_layout_note_slot",
    "js_write_barrier",
    "js_write_barrier_root_nanbox",
    "js_write_barrier_slot",
    "js_runtime_write_barrier_slot",
    "js_gc_register_global_root",
    // pure value predicates / bit twiddling
    "js_is_truthy",
    "js_nanbox_get_pointer",
    "js_value_is_object",
    "js_value_is_string",
    "js_typeof_tag",
    // inline-cache guards: pure reads
    "js_typed_feedback_closure_direct_call_guard",
    "js_typed_feedback_shape_guard",
    "js_typed_feedback_note",
    // verified non-allocating bookkeeping stores/reads
    "js_closure_set_capture_bits",
    "js_closure_get_capture_bits",
    "js_closure_set_capture_ptr",
    "js_closure_get_capture_ptr",
    "js_box_set_bits",
    "js_box_get_bits",
    "js_i32_box_set",
    "js_bool_box_set",
    "js_tdz_suppress_begin",
    "js_tdz_suppress_end",
    "js_array_note_numeric_write",
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

fn is_collecting(callee: &str) -> bool {
    if callee.starts_with("llvm.") {
        return false;
    }
    !NON_COLLECTING.contains(&callee)
}

/// One instruction, in the vocabulary this pass needs.
struct Facts {
    /// Register this instruction defines, without the `%`. Kept even though
    /// only `load_of` reads it today: `Facts` is the pass's whole vocabulary,
    /// and a def map is the first thing a follow-up (a dead-load sweep, say)
    /// needs.
    #[allow(dead_code)]
    result: Option<String>,
    /// Registers it reads, without the `%`. Only operands — never `result`.
    uses: Vec<String>,
    /// `Some((dst, ty, slot))` for `dst = load ty, ptr %slot`.
    load_of: Option<(String, LlvmType, String)>,
    /// Alloca this instruction stores into, without the `%`.
    stores_to: Option<String>,
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

pub(crate) fn apply_to_function(func: &mut LlFunction) -> usize {
    let slots: HashSet<String> = {
        let bound = func.reg_counter().shadow_slot_allocas();
        if bound.is_empty() {
            return 0;
        }
        bound
            .iter()
            .map(|s| s.trim_start_matches('%').to_string())
            .collect()
    };

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

    // Slot loads, and where they are.
    let mut loads: Vec<(usize, usize)> = Vec::new();
    for (bi, fb) in facts.iter().enumerate() {
        for (ii, f) in fb.iter().enumerate() {
            if f.load_of.is_some() {
                loads.push((bi, ii));
            }
        }
    }
    if loads.is_empty() {
        return 0;
    }
    if blocks.len().saturating_mul(loads.len()) > MAX_BLOCK_LOAD_PRODUCT {
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

    // Where a use must be rewritten: (block, insn, old register, new register,
    // slot, type). Collected first so the instruction vectors are not mutated
    // while `facts` still indexes them.
    struct Rewrite {
        blk: usize,
        insn: usize,
        from: String,
        slot: String,
        ty: LlvmType,
    }
    let mut rewrites: Vec<Rewrite> = Vec::new();

    for (lb, li) in loads {
        let (dst, ty, slot) = facts[lb][li].load_of.clone().expect("load site");
        // Forward reachability from the load, never re-entering the load's own
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
                if f.uses.iter().any(|u| *u == dst) && c && !s && !f.is_phi {
                    rewrites.push(Rewrite {
                        blk: ub,
                        insn: ui,
                        from: dst.clone(),
                        slot: slot.clone(),
                        ty,
                    });
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
    let boundary = func.entry_init_boundary();
    let entry_inserts = match boundary {
        None => 0,
        Some(b) => rewrites
            .iter()
            .filter(|r| r.blk == 0 && r.insn <= b)
            .count(),
    };
    {
        let blocks = func.blocks_mut();
        for r in rewrites {
            let fresh = format!("%r{}", counter.next());
            let reload = LlInst::Load {
                dst: fresh.clone(),
                ty: r.ty,
                ptr: format!("%{}", r.slot),
                flavor: LoadFlavor::Plain,
            };
            let insts = blocks[r.blk].insts_mut();
            rename_operand(&mut insts[r.insn], &r.from, fresh.trim_start_matches('%'));
            insts.insert(r.insn, reload);
        }
    }
    func.note_entry_block_insertions(entry_inserts);
    n
}

fn facts_of(inst: &LlInst, slots: &HashSet<String>) -> Facts {
    let mut uses = Vec::new();
    let mut result = None;
    let mut load_of = None;
    let mut stores_to = None;
    let mut collecting = false;
    let mut is_phi = false;
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
        LlInst::Bin { dst, a, b, .. } => {
            result = reg(dst);
            use_op(&mut uses, a);
            use_op(&mut uses, b);
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
            if let (Some(d), Some(p)) = (reg(dst), reg(ptr)) {
                if slots.contains(&p) {
                    load_of = Some((d, *ty, p));
                }
            }
        }
        LlInst::Store { val, ptr, .. } => {
            use_op(&mut uses, val);
            use_op(&mut uses, ptr);
            stores_to = reg(ptr);
        }
        LlInst::Cast { dst, v, .. } => {
            result = reg(dst);
            use_op(&mut uses, v);
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
            dst, callee, args, ..
        } => {
            result = dst.as_deref().and_then(reg);
            for (_, v) in args {
                use_op(&mut uses, v);
            }
            collecting = is_collecting(callee);
        }
        LlInst::CallIndirect {
            dst, fptr, args, ..
        } => {
            result = reg(dst);
            use_op(&mut uses, fptr);
            for (_, v) in args {
                use_op(&mut uses, v);
            }
            collecting = true;
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
        // `load <ty>, ptr %slot[, …]`
        if let Some((ty, rest)) = rhs["load ".len()..].split_once(", ptr ") {
            let ty = ty.trim();
            let ptr = rest
                .split(|c: char| c == ',' || c.is_whitespace())
                .next()
                .unwrap_or("");
            if let (Some(d), Some(p)) =
                (result.clone(), ptr.strip_prefix('%').map(|s| s.to_string()))
            {
                if slots.contains(&p) {
                    if let Some(t) = static_llvm_type(ty) {
                        load_of = Some((d, t, p));
                    }
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
            stores_to = ptr.strip_prefix('%').map(|s| s.to_string());
        }
    }
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
        collecting,
        is_phi,
        succs,
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::function::LlFunction;
    use crate::types::{DOUBLE, I64, PTR};

    /// Render a function the way `to_ir` does, minus the header, so a test can
    /// assert on instruction text.
    fn body(func: &LlFunction) -> String {
        let mut out = String::new();
        func.for_each_final_line::<std::convert::Infallible>(&mut |line| {
            out.push_str(line);
            out.push('\n');
            Ok(())
        })
        .unwrap_or_else(|e| match e {});
        out
    }

    /// `%slot` is a bound shadow slot holding a parameter; `mid` runs between
    /// the load and the use.
    ///
    /// This is #7280's zod object-spread frame, reduced: load the root, call
    /// something, hand the LOADED REGISTER to a consumer that dereferences it.
    fn one_block(mid: &str) -> LlFunction {
        let mut f = LlFunction::new("t", DOUBLE, vec![(DOUBLE, "%arg".into())]);
        let b = f.create_block("entry");
        let slot = b.alloca(DOUBLE);
        b.store(DOUBLE, "%arg", &slot);
        b.call_void(
            "js_shadow_slot_bind",
            &[(crate::types::I32, "0"), (PTR, &slot)],
        );
        let v = b.load(DOUBLE, &slot);
        b.call(DOUBLE, mid, &[]);
        let r = b.call(
            DOUBLE,
            "js_object_assign_one",
            &[(DOUBLE, &v), (DOUBLE, "0.0")],
        );
        b.ret(DOUBLE, &r);
        f
    }

    #[test]
    fn the_planted_hazard_is_rewritten_to_a_reload() {
        let mut f = one_block("js_object_alloc");
        assert_eq!(apply_to_function(&mut f), 1, "one stale operand to rewrite");
        let ir = body(&f);
        // The consumer must no longer read the pre-call register, and the
        // register it does read must be defined by a load of the same slot
        // immediately above it.
        let lines: Vec<&str> = ir.lines().map(str::trim).collect();
        let use_idx = lines
            .iter()
            .position(|l| l.contains("@js_object_assign_one"))
            .expect("the consumer survived the pass");
        let reload = lines[use_idx - 1];
        assert!(
            reload.starts_with("%r") && reload.contains("= load double, ptr %r1"),
            "the instruction above the consumer must re-read the slot, got {reload:?}\n{ir}"
        );
        let fresh = reload.split_whitespace().next().unwrap();
        assert!(
            lines[use_idx].contains(&format!("double {fresh},")),
            "the consumer must read the reloaded register, got {:?}",
            lines[use_idx]
        );
        assert!(
            !lines[use_idx].contains("double %r2,"),
            "the consumer must NOT still read the pre-call load"
        );
    }

    #[test]
    fn a_non_collecting_window_is_left_byte_for_byte_alone() {
        // The ONLY difference from the planted case is which helper sits in the
        // window. `js_write_barrier` cannot allocate, so nothing can move and
        // the register is still valid — a pass that fires here is firing on the
        // code shape rather than on the hazard, and would put a reload between
        // every pair of instructions in the compiler's output.
        let before = body(&one_block("js_write_barrier"));
        let mut f = one_block("js_write_barrier");
        assert_eq!(apply_to_function(&mut f), 0);
        assert_eq!(body(&f), before);
    }

    #[test]
    fn a_slot_the_program_reassigns_in_the_window_is_not_reloaded() {
        // ★ The soundness half. `f(x, (x = other, 1))` must pass the ORIGINAL
        // `x`; re-reading the slot below the assignment would hand the consumer
        // the new value, which is a miscompile and not a rooting fix. See
        // `expr::temp_root::operand_is_reloadable`.
        let mut f = LlFunction::new("t", DOUBLE, vec![(DOUBLE, "%arg".into())]);
        let b = f.create_block("entry");
        let slot = b.alloca(DOUBLE);
        b.store(DOUBLE, "%arg", &slot);
        b.call_void(
            "js_shadow_slot_bind",
            &[(crate::types::I32, "0"), (PTR, &slot)],
        );
        let v = b.load(DOUBLE, &slot);
        let other = b.call(DOUBLE, "js_object_alloc", &[]);
        b.store(DOUBLE, &other, &slot); // the reassignment
        let r = b.call(
            DOUBLE,
            "js_object_assign_one",
            &[(DOUBLE, &v), (DOUBLE, "0.0")],
        );
        b.ret(DOUBLE, &r);
        assert_eq!(
            apply_to_function(&mut f),
            0,
            "a slot the program itself stores to in the window must be left alone"
        );
    }

    #[test]
    fn the_window_is_a_cfg_path_not_a_line_range() {
        // Load in the entry block, collection point in one arm, use in the
        // merge. A line-order scan sees the same thing a path-based one does
        // here; the point of the test is that the merge block IS considered at
        // all, since the dominant population in both corpora is cross-block.
        let mut f = LlFunction::new("t", DOUBLE, vec![(DOUBLE, "%arg".into())]);
        let entry = f.create_block("entry").label.clone();
        let then = f.create_block("then").label.clone();
        let merge = f.create_block("merge").label.clone();
        let _ = entry;
        let slot;
        let v;
        {
            let b = f.block_mut(0).unwrap();
            slot = b.alloca(DOUBLE);
            b.store(DOUBLE, "%arg", &slot);
            b.call_void(
                "js_shadow_slot_bind",
                &[(crate::types::I32, "0"), (PTR, &slot)],
            );
            v = b.load(DOUBLE, &slot);
            b.cond_br("%c", &then, &merge);
        }
        {
            let b = f.block_mut(1).unwrap();
            b.call(DOUBLE, "js_object_alloc", &[]);
            b.br(&merge);
        }
        {
            let b = f.block_mut(2).unwrap();
            let r = b.call(
                DOUBLE,
                "js_object_assign_one",
                &[(DOUBLE, &v), (DOUBLE, "0.0")],
            );
            b.ret(DOUBLE, &r);
        }
        assert_eq!(apply_to_function(&mut f), 1);
        let ir = body(&f);
        assert!(
            ir.matches("= load double, ptr %r1").count() == 2,
            "the merge block must re-read the slot:\n{ir}"
        );
    }

    #[test]
    fn a_back_edge_round_trip_is_not_an_intra_iteration_path() {
        // The load is INSIDE the loop, so every iteration re-executes it and the
        // register is fresh when the use runs. Counting the back edge would make
        // the pass reload on every loop body in the program.
        let mut f = LlFunction::new("t", DOUBLE, vec![(DOUBLE, "%arg".into())]);
        f.create_block("entry");
        let looplbl = f.create_block("loop").label.clone();
        let done = f.create_block("done").label.clone();
        let slot;
        {
            let b = f.block_mut(0).unwrap();
            slot = b.alloca(DOUBLE);
            b.store(DOUBLE, "%arg", &slot);
            b.call_void(
                "js_shadow_slot_bind",
                &[(crate::types::I32, "0"), (PTR, &slot)],
            );
            b.br(&looplbl);
        }
        {
            let b = f.block_mut(1).unwrap();
            let v = b.load(DOUBLE, &slot);
            b.call(
                DOUBLE,
                "js_object_assign_one",
                &[(DOUBLE, &v), (DOUBLE, "0.0")],
            );
            b.call(DOUBLE, "js_object_alloc", &[]); // collects, but AFTER the use
            b.cond_br("%c", &looplbl, &done);
        }
        {
            let b = f.block_mut(2).unwrap();
            b.ret(DOUBLE, "0.0");
        }
        assert_eq!(apply_to_function(&mut f), 0);
    }

    /// ★ #7305 turned every throwing call into an `invoke` with TWO successors,
    /// and perry emits the continuation label INLINE in the same builder block.
    /// Both halves have to be modelled:
    ///
    /// * the call half — an `invoke` of a collecting helper opens a window, so
    ///   a use below it must reload. Missing this drops every reload inside a
    ///   `try`, silently.
    /// * the terminator half — the unwind edge is a real path, so a use in the
    ///   landing pad is reached from the load and must reload too.
    ///
    /// The unwind edge is also why the rule is "reload at the USE" rather than
    /// "reload after the call": a load from the slot reads whatever the
    /// collector last wrote and is valid wherever it sits, so it is correct on
    /// the unwind edge and the normal edge alike. There is no "after the call"
    /// position that would have to be chosen correctly for two successors.
    #[test]
    fn an_invoke_opens_a_window_on_both_of_its_edges() {
        let mut f = LlFunction::new("t", DOUBLE, vec![(DOUBLE, "%arg".into())]);
        f.create_block("entry");
        let cont = f.create_block("cont").label.clone();
        let lpad = f.create_block("lpad").label.clone();
        let slot;
        let v;
        {
            let b = f.block_mut(0).unwrap();
            slot = b.alloca(DOUBLE);
            b.store(DOUBLE, "%arg", &slot);
            b.call_void(
                "js_shadow_slot_bind",
                &[(crate::types::I32, "0"), (PTR, &slot)],
            );
            v = b.load(DOUBLE, &slot);
            b.emit_raw(format!(
                "invoke double @js_object_alloc(i32 4) to label %{cont} unwind label %{lpad}"
            ));
        }
        {
            let b = f.block_mut(1).unwrap();
            b.call(
                DOUBLE,
                "js_object_assign_one",
                &[(DOUBLE, &v), (DOUBLE, "0.0")],
            );
            b.ret(DOUBLE, "0.0");
        }
        {
            let b = f.block_mut(2).unwrap();
            b.call(
                DOUBLE,
                "js_object_assign_one",
                &[(DOUBLE, &v), (DOUBLE, "1.0")],
            );
            b.ret(DOUBLE, "0.0");
        }
        assert_eq!(
            apply_to_function(&mut f),
            2,
            "both the normal and the unwind successor use the stale register"
        );
        let ir = body(&f);
        assert_eq!(
            ir.matches("= load double, ptr %r1").count(),
            3,
            "the original load plus one reload on each edge:\n{ir}"
        );
    }

    #[test]
    fn a_function_with_no_bound_slot_is_untouched() {
        let mut f = LlFunction::new("t", DOUBLE, vec![(DOUBLE, "%arg".into())]);
        let b = f.create_block("entry");
        let slot = b.alloca(DOUBLE);
        b.store(DOUBLE, "%arg", &slot);
        let v = b.load(DOUBLE, &slot);
        b.call(DOUBLE, "js_object_alloc", &[]);
        let r = b.call(
            DOUBLE,
            "js_object_assign_one",
            &[(DOUBLE, &v), (DOUBLE, "0.0")],
        );
        b.ret(DOUBLE, &r);
        assert_eq!(apply_to_function(&mut f), 0);
    }

    #[test]
    fn the_bind_is_recorded_through_the_inline_form_too() {
        // #7088's inline slot store emits the SAME `js_shadow_slot_bind` call on
        // its slow arm, which is why the recording hook lives in `call_void`
        // rather than at the thirteen sites that build a bind. If a future bind
        // form stops going through `call_void`, this is the assertion that has
        // to be updated with it.
        let mut f = LlFunction::new("t", DOUBLE, vec![]);
        let b = f.create_block("entry");
        let slot = b.alloca(DOUBLE);
        b.call_void(
            "js_shadow_slot_bind",
            &[(crate::types::I32, "3"), (PTR, &slot)],
        );
        b.ret(DOUBLE, "0.0");
        assert!(f
            .reg_counter()
            .shadow_slot_allocas()
            .contains(slot.as_str()));
    }

    #[test]
    fn a_raw_operand_is_renamed_by_token_not_by_substring() {
        // `%r1` must not rewrite inside `%r10`. This is the reason the Raw arm
        // does not use `str::replace`.
        let line = "  %r99 = fadd double %r1, %r10";
        assert_eq!(
            rename_in_text(line, "r1", "r42"),
            "  %r99 = fadd double %r42, %r10"
        );
        // And the LHS is never a use.
        assert_eq!(
            rename_in_text("  %r1 = fadd double %r2, %r3", "r1", "r42"),
            "  %r1 = fadd double %r2, %r3"
        );
    }

    /// ★ The regression that cost the acceptance arm 30/30 -> 0/30.
    ///
    /// `entry_post_init_setup` is spliced into block 0 at `entry_init_boundary`,
    /// and for a function built by `enable_post_init_shadow_frame` that region
    /// contains the `js_shadow_frame_enter` call itself. Bumping the boundary by
    /// EVERY insertion into block 0 — rather than only the ones at or above it —
    /// pushes the index past the block, `to_ir` clamps it with
    /// `.min(instruction_count())`, and the frame push lands after every
    /// `js_shadow_slot_bind` in the body. Nothing is rooted, and the symptom is
    /// `TypeError: value is not a function` — the bug this pass fixes, wearing
    /// its own fix as a disguise.
    #[test]
    fn an_insertion_below_the_post_init_splice_does_not_move_it() {
        let mut f = LlFunction::new("t", DOUBLE, vec![(DOUBLE, "%arg".into())]);
        {
            let b = f.create_block("entry");
            // The "init prelude": everything before mark_entry_init_boundary.
            b.call_void("js_gc_init", &[]);
        }
        f.mark_entry_init_boundary();
        let boundary_before = f.entry_init_boundary();
        assert_eq!(boundary_before, Some(1));
        {
            let b = f.block_mut(0).unwrap();
            let slot = b.alloca(DOUBLE);
            b.store(DOUBLE, "%arg", &slot);
            b.call_void(
                "js_shadow_slot_bind",
                &[(crate::types::I32, "0"), (PTR, &slot)],
            );
            let v = b.load(DOUBLE, &slot);
            b.call(DOUBLE, "js_object_alloc", &[]);
            let r = b.call(
                DOUBLE,
                "js_object_assign_one",
                &[(DOUBLE, &v), (DOUBLE, "0.0")],
            );
            b.ret(DOUBLE, &r);
        }
        let n_insts = f.blocks()[0].instruction_count();
        assert_eq!(apply_to_function(&mut f), 1);
        assert_eq!(
            f.entry_init_boundary(),
            boundary_before,
            "the reload went in BELOW the splice, so the splice must not move"
        );
        assert!(
            f.entry_init_boundary().unwrap() <= f.blocks()[0].instruction_count(),
            "a boundary past the block gets clamped to the END by to_ir, which \
             relocates the whole post-init region including the frame push"
        );
        assert_eq!(f.blocks()[0].instruction_count(), n_insts + 1);
    }

    #[test]
    fn an_i64_slot_reloads_at_its_own_width() {
        let mut f = LlFunction::new("t", DOUBLE, vec![]);
        let b = f.create_block("entry");
        let slot = b.alloca(I64);
        b.call_void(
            "js_shadow_slot_bind",
            &[(crate::types::I32, "0"), (PTR, &slot)],
        );
        let v = b.load(I64, &slot);
        b.call(DOUBLE, "js_object_alloc", &[]);
        b.call(
            DOUBLE,
            "js_object_assign_one",
            &[(I64, &v), (DOUBLE, "0.0")],
        );
        b.ret(DOUBLE, "0.0");
        assert_eq!(apply_to_function(&mut f), 1);
        assert!(body(&f).contains("= load i64, ptr %r1"), "{}", body(&f));
    }
}
