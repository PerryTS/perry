//! #10812: the prologue stack check of a compiled JS body.
//!
//! ```llvm
//!   %limit = load ptr, <agent-pointer slot AGENT_PTR_STACK_LIMIT>
//!   %sp    = call i64 @llvm.read_register.i64(metadata !{!"rsp"})
//!   %deep  = icmp ult i64 %sp, ptrtoint(%limit)
//!   br i1 %deep, label %stack_guard.overflow, label %stack_guard.ok
//! stack_guard.overflow:
//!   call void @js_stack_overflow()      ; cold; throws RangeError
//!   unreachable
//! ```
//!
//! The limit is published per agent by `perry-runtime/src/stack_guard.rs`; a
//! null slot (not published) compares as 0 and never fires. The slot is read
//! inline where the agent-pointer block is (`agent_ptr.rs`: local-exec TLS
//! in ELF executables, the pthread-TSD hot cache on Apple aarch64). Where it
//! would cost a runtime call per function entry, no check is emitted and an
//! overflow still faults as before.
//!
//! On x86-64 in an ELF executable the check is two instructions,
//! `cmp %fs:PERRY_AGENT_PTRS@TPOFF+16, %rsp; jb overflow`: the slot is a
//! local-exec thread-local, so its load folds into the compare as a
//! `%fs:`-relative operand, and the stack pointer is read with
//! `llvm.read_register`, which the compare takes as its register operand
//! directly (an inline-asm `movq %rsp` would cost a register and a move).
//! Neither forces a frame pointer, as `llvm.frameaddress` would.
//!
//! At a body's entry the overflow arm ends in `unreachable` after the call
//! (`js_stack_overflow` is `-> !` in the runtime). Nothing of the body is live
//! there, so its statepoint carries no roots: the arm spills and reloads
//! nothing and does not branch back. A throw at entry abandons the frame
//! before any value of it exists; no `try` region covers an entry, and each
//! caller keeps its own roots at its own call.
//!
//! The check a `preserve_nonecc` recursion clone makes before calling another
//! clone ([`emit_stack_guard_before_clone_call`]) keeps its arm on a path to a
//! return instead (`br` back to the call), and the declaration stays without
//! `noreturn` for it: a block that ends the function without returning stops
//! ShrinkWrap from sinking the clone's prologue past its frameless leaf path
//! (`spec_preserve_none_tests::the_clone_entry_is_shrink_wrapped_frameless`).
//! There the arm sits mid-body, after the clone's frame exists, so its
//! spills are the price of the frameless entry. An entry check has no such
//! trade: a body that carries one sets up its frame at entry either way.

use super::agent_ptr::{agent_ptr_access, AgentPtrAccess};
use super::FnCtx;
use crate::types::{I64, I8, PTR};

const AGENT_PTR_STACK_LIMIT: usize = crate::runtime_abi::AGENT_PTR_STACK_LIMIT;
const HOT_TLS_AGENT_PTRS_OFFSET: usize = crate::runtime_abi::HOT_TLS_AGENT_PTRS_OFFSET;

/// Emit the entry check of a body; lowering continues in a fresh block
/// reached when the frame is above the limit.
///
/// A `preserve_nonecc` recursion clone (#8175) is skipped: its leaf path is
/// shrink-wrapped frameless, and a check whose slow path makes a call would
/// put the frame back on every entry. Its recursion is checked at the call
/// instead ([`emit_stack_guard_before_clone_call`]), where the frame exists.
pub(crate) fn emit_stack_guard(ctx: &mut FnCtx<'_>) -> Option<GuardSite> {
    if ctx.func.is_preserve_none() {
        return None;
    }
    let block = ctx.current_block;
    let inst = ctx.func.blocks()[block].insts().len();
    let first_new = ctx.func.num_blocks();
    emit_check(ctx, Arm::EndsHere)?;
    Some(GuardSite {
        block,
        inst,
        guard_blocks: first_new..ctx.func.num_blocks(),
        ok: ctx.current_block,
    })
}

/// Where [`emit_stack_guard`] put a body's entry check: the block and
/// instruction index it starts at, the blocks it created, and the block the
/// body starts in.
pub(crate) struct GuardSite {
    block: usize,
    inst: usize,
    guard_blocks: std::ops::Range<usize>,
    ok: usize,
}

/// Remove a body's entry check when the body cannot recurse.
///
/// The check exists so unbounded recursion throws a catchable RangeError
/// instead of faulting. A body none of whose calls can run JS (every callee an
/// LLVM intrinsic or a runtime symbol the effects table does not class as
/// `Reenters`; user code and unknown symbols are `Reenters`) is on no cycle:
/// its frame is one more frame below a caller that checked, and any recursion
/// that reaches it goes through a body that keeps its check. Such leaves are
/// the bodies LLVM inlines, where the check's `asm sideeffect` TLS read stays
/// in the caller's hot loop and cannot be hoisted or merged: in the chess
/// engine's search it cost 7% of all instructions.
///
/// `PERRY_LEAF_STACK_GUARD=1` keeps every check (A/B control).
pub(crate) fn finish_stack_guard(ctx: &mut FnCtx<'_>, site: Option<GuardSite>) {
    let Some(site) = site else { return };
    if std::env::var("PERRY_LEAF_STACK_GUARD").as_deref() == Ok("1") {
        return;
    }
    let gc = ctx.func.gc_effects();
    let safe = |callee: &str| {
        callee.starts_with("llvm.")
            || gc.runtime_class(callee) != crate::gc_call_effects::RuntimeClass::Reenters
    };
    // The check's own blocks are skipped, but `ok` is where the body starts:
    // it is in the check's block range and must be scanned like the rest.
    //
    // The entry block's instructions before the check (the prologue: root-slot
    // binds, parameter stores) ran before it already; the check never
    // protected them, so they do not count either.
    let reenters = ctx.func.blocks().iter().enumerate().any(|(b, blk)| {
        let from = if b == site.block { site.inst } else { 0 };
        (b == site.ok || !site.guard_blocks.contains(&b))
            && blk.insts()[from.min(blk.insts().len())..]
                .iter()
                .any(|inst| inst_may_reenter(inst, &safe))
    });
    if reenters {
        if std::env::var("PERRY_STACK_GUARD_CALL_SITES").as_deref() != Ok("0") {
            move_check_to_call_sites(ctx, &site, &safe);
        }
        return;
    }
    remove_entry_check(ctx, &site);
}

/// Replace the entry check by a branch into the body.
fn remove_entry_check(ctx: &mut FnCtx<'_>, site: &GuardSite) {
    let ok_label = ctx.block_label(site.ok);
    let blocks = ctx.func.blocks_mut();
    // The check's own instructions in the entry block, then its blocks: the
    // hot-TLS lookup and the overflow arm become empty `unreachable` blocks
    // (LLVM drops them), so nothing references the removed values.
    let entry = blocks[site.block].insts_mut();
    entry.truncate(site.inst);
    entry.push(crate::inst::LlInst::Br {
        label: ok_label,
        meta: None,
    });
    for b in site.guard_blocks.clone() {
        if b == site.ok {
            continue;
        }
        let insts = blocks[b].insts_mut();
        insts.clear();
        insts.push(crate::inst::LlInst::Unreachable);
    }
}

/// The most blocks a body may re-check before their calls instead of at
/// entry.
const MAX_CALL_SITE_CHECKS: usize = 8;

/// A body that can recurse only through a few calls checks before the blocks
/// holding them instead of at its entry.
///
/// Recursion through this body continues only through one of its calls that
/// can run JS, so a check made on entry to the block holding such a call
/// fires at the same depth, from the same frame, as the entry check would.
/// When those calls are a body's cold arms (the generic fallback of a guarded
/// clone, a `valueOf`-capable comparison), its hot path carries no check,
/// inlined or not: a one-line `at(sq) { return this.board[sq] ?? 0 }` inlined
/// into a loop had put the entry check's hot-TLS lookup (13 instructions)
/// into every iteration.
///
/// The check is a new block in front of the calling block: its predecessors
/// branch to the check, which falls through into it. The calling block keeps
/// its name, its place and every instruction but its leading phis, which
/// move to the check's first block (its predecessors are theirs).
///
/// Declined, keeping the entry check, when a body has a source loop, more
/// than [`MAX_CALL_SITE_CHECKS`] calling blocks, a call in its entry block,
/// or any exception-handling construct (a check inside a `try` would have to
/// unwind to its handler).
fn move_check_to_call_sites(ctx: &mut FnCtx<'_>, site: &GuardSite, safe: &dyn Fn(&str) -> bool) {
    // A body with a source loop keeps its entry check: a call in the loop may
    // be inlined later (a guarded trampoline), and a check before it would
    // then run every iteration instead of once per entry. A cycle that is no
    // source loop (a generic comparison's coercion retry) does not count.
    if has_source_loop(ctx) && has_back_edge(ctx) {
        return;
    }
    let mut calling: Vec<usize> = Vec::new();
    for (b, blk) in ctx.func.blocks().iter().enumerate() {
        if b != site.ok && site.guard_blocks.contains(&b) {
            continue;
        }
        let label = blk.label.as_str();
        // Try lowering's blocks (`try.body`, `try.catch*`, `try.finally`) and
        // its landing pads (`eh.lpad*`).
        if label.starts_with("try.") || label.starts_with("eh.") {
            return;
        }
        let from = if b == site.block { site.inst } else { 0 };
        let mut calls = false;
        for inst in &blk.insts()[from.min(blk.insts().len())..] {
            if let crate::inst::LlInst::Raw(text) = inst {
                if text.contains("invoke")
                    || text.contains("landingpad")
                    || text.contains("catchswitch")
                    || text.contains("cleanuppad")
                {
                    return;
                }
            }
            calls |= inst_may_reenter(inst, safe);
        }
        if calls {
            // A call in the entry block runs where the entry check does. The
            // function's entry block (0) is not always the check's block (a
            // constructor prologue precedes it), so both count.
            if b == site.block || b == 0 {
                return;
            }
            calling.push(b);
        }
    }
    if calling.is_empty() || calling.len() > MAX_CALL_SITE_CHECKS {
        return;
    }
    remove_entry_check(ctx, site);
    let original = ctx.func.blocks().len();
    let mut groups: Vec<(usize, std::ops::Range<usize>)> = Vec::new();
    for &b in &calling {
        let target = ctx.block_label(b);
        let created_from = ctx.func.blocks().len();
        let check_idx = ctx.new_block("stack_guard.call");
        let check = ctx.block_label(check_idx);
        // Every edge into the calling block now enters the check first.
        for (idx, blk) in ctx.func.blocks_mut().iter_mut().enumerate() {
            if idx != check_idx {
                retarget_branches(blk.insts_mut(), &target, &check);
            }
        }
        // The check's first block now has exactly the calling block's
        // predecessors, so the calling block's leading phis move to it
        // unchanged and still dominate everything after.
        let phis = take_leading_phis(ctx.func.blocks_mut()[b].insts_mut());
        ctx.func.blocks_mut()[check_idx].insts_mut().extend(phis);
        let saved = ctx.current_block;
        ctx.current_block = check_idx;
        let _ = emit_check(ctx, Arm::RejoinsTheCall);
        if !ctx.block().is_terminated() {
            ctx.block().br(&target);
        }
        ctx.current_block = saved;
        groups.push((b, created_from..ctx.func.blocks().len()));
    }
    place_before_calling_blocks(ctx, original, &groups);
}

/// Remove and return the block's leading phis.
fn take_leading_phis(insts: &mut Vec<crate::inst::LlInst>) -> Vec<crate::inst::LlInst> {
    let n = insts
        .iter()
        .take_while(|inst| match inst {
            crate::inst::LlInst::Phi { .. } => true,
            crate::inst::LlInst::Raw(text) => text
                .trim_start()
                .split_once(" = ")
                .is_some_and(|(_, rhs)| rhs.starts_with("phi ")),
            _ => false,
        })
        .count();
    insts.drain(..n).collect()
}

/// Point every branch to `%from` at `%to`. A raw terminator (`switch`, a raw
/// `br`) names its targets as `label %name`; the next character decides the
/// match, so `%b.8` is not `%b.80`.
fn retarget_branches(insts: &mut [crate::inst::LlInst], from: &str, to: &str) {
    use crate::inst::LlInst;
    for inst in insts.iter_mut() {
        match inst {
            LlInst::Br { label, .. } if label == from => *label = to.to_string(),
            LlInst::CondBr { t, f, .. } => {
                if t == from {
                    *t = to.to_string();
                }
                if f == from {
                    *f = to.to_string();
                }
            }
            LlInst::Raw(text) if !text.contains(" phi ") && text.contains("label %") => {
                let needle = format!("label %{from}");
                let mut out = String::with_capacity(text.len());
                let mut rest = text.as_str();
                while let Some(pos) = rest.find(&needle) {
                    let end = pos + needle.len();
                    let boundary = !rest[end..].starts_with(is_label_char);
                    out.push_str(&rest[..pos]);
                    if boundary {
                        out.push_str("label %");
                        out.push_str(to);
                    } else {
                        out.push_str(&needle);
                    }
                    rest = &rest[end..];
                }
                out.push_str(rest);
                *text = out;
            }
            _ => {}
        }
    }
}

fn is_label_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '.' | '$' | '-')
}

/// Lay each check's blocks out right before the block it guards, so the IR
/// still reads top to bottom in execution order (`new_block` appends).
fn place_before_calling_blocks(
    ctx: &mut FnCtx<'_>,
    original: usize,
    groups: &[(usize, std::ops::Range<usize>)],
) {
    let total = ctx.func.blocks().len();
    let mut order: Vec<usize> = Vec::with_capacity(total);
    let mut placed = vec![false; total];
    for b in 0..original {
        for (_, range) in groups.iter().filter(|(owner, _)| *owner == b) {
            for idx in range.clone() {
                order.push(idx);
                placed[idx] = true;
            }
        }
        order.push(b);
        placed[b] = true;
    }
    order.extend((original..total).filter(|idx| !placed[*idx]));
    let mut new_index = vec![0; total];
    for (pos, &idx) in order.iter().enumerate() {
        new_index[idx] = pos;
    }
    let blocks = ctx.func.blocks_mut();
    let mut taken: Vec<Option<crate::block::LlBlock>> =
        std::mem::take(blocks).into_iter().map(Some).collect();
    blocks.extend(
        order
            .iter()
            .map(|&idx| taken[idx].take().expect("each block placed once")),
    );
    ctx.current_block = new_index[ctx.current_block];
}

/// Does the body lower a source loop (`for`, `while`, `do`, a loop region,
/// an inlined array-method loop)? Judged from the loop lowering's block names.
fn has_source_loop(ctx: &FnCtx<'_>) -> bool {
    ctx.func.blocks().iter().any(|b| {
        let l = b.label.as_str();
        l.starts_with("for.")
            || l.starts_with("while.")
            || l.starts_with("dowhile.")
            || l.starts_with("do.")
            || l.contains("loop")
    })
}

/// Does the body contain a loop? A depth-first walk from the entry block
/// that meets an edge into a block still on its stack has found one.
fn has_back_edge(ctx: &FnCtx<'_>) -> bool {
    let blocks = ctx.func.blocks();
    let index: std::collections::HashMap<&str, usize> = blocks
        .iter()
        .enumerate()
        .map(|(i, b)| (b.label.as_str(), i))
        .collect();
    let successors = |b: usize| -> Vec<usize> {
        let mut out = Vec::new();
        let mut add = |label: &str| {
            if let Some(&to) = index.get(label) {
                out.push(to);
            }
        };
        for inst in blocks[b].insts() {
            match inst {
                crate::inst::LlInst::Br { label, .. } => add(label),
                crate::inst::LlInst::CondBr { t, f, .. } => {
                    add(t);
                    add(f);
                }
                crate::inst::LlInst::Raw(text) if !text.contains(" phi ") => {
                    for rest in text.split("label %").skip(1) {
                        let l: String = rest
                            .chars()
                            .take_while(|c| {
                                c.is_alphanumeric() || matches!(c, '_' | '.' | '$' | '-')
                            })
                            .collect();
                        add(&l);
                    }
                }
                _ => {}
            }
        }
        out
    };
    // 0 = unvisited, 1 = on the stack, 2 = done.
    let mut state = vec![0u8; blocks.len()];
    let mut stack: Vec<(usize, Vec<usize>, usize)> = Vec::new();
    if blocks.is_empty() {
        return false;
    }
    state[0] = 1;
    stack.push((0, successors(0), 0));
    while let Some((b, succ, next)) = stack.last_mut() {
        if *next < succ.len() {
            let to = succ[*next];
            *next += 1;
            match state[to] {
                1 => return true,
                0 => {
                    state[to] = 1;
                    let s = successors(to);
                    stack.push((to, s, 0));
                }
                _ => {}
            }
        } else {
            state[*b] = 2;
            stack.pop();
        }
    }
    false
}

/// Can this instruction run JS (and so recurse)? `safe` answers for a direct
/// callee name.
fn inst_may_reenter(inst: &crate::inst::LlInst, safe: &dyn Fn(&str) -> bool) -> bool {
    use crate::inst::LlInst;
    match inst {
        LlInst::Call { callee, .. } => !safe(callee),
        LlInst::CallIndirect { .. } => true,
        LlInst::Raw(s) => {
            let t = s.trim_start();
            if !(t.contains("call ") || t.contains("invoke ")) || t.contains(" asm ") {
                return false;
            }
            match t.find('@') {
                Some(at) => {
                    let name: String = t[at + 1..]
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '$'))
                        .collect();
                    !safe(&name)
                }
                None => true,
            }
        }
        _ => false,
    }
}

/// The check before a direct call from one `preserve_nonecc` recursion clone
/// to another (`callee`), standing in for the entry check the callee skips.
pub(crate) fn emit_stack_guard_before_clone_call(ctx: &mut FnCtx<'_>, callee: &str) {
    if ctx.func.is_preserve_none() && ctx.func.reg_counter().callee_preserve_none(callee) {
        let _ = emit_check(ctx, Arm::RejoinsTheCall);
    }
}

/// How the overflow arm ends (module docs).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Arm {
    /// `unreachable`: nothing live across the call (a body's entry).
    EndsHere,
    /// `br` back to the checked call, keeping the clone's leaf path
    /// shrink-wrappable.
    RejoinsTheCall,
}

fn emit_check(ctx: &mut FnCtx<'_>, arm: Arm) -> Option<()> {
    if ctx.block().is_terminated() {
        return None;
    }
    let sp_register = if ctx.target_triple.starts_with("x86_64") {
        "rsp"
    } else if ctx.target_triple.starts_with("aarch64") || ctx.target_triple.starts_with("arm64") {
        "sp"
    } else {
        return None;
    };
    let slot_off = (AGENT_PTR_STACK_LIMIT * 8).to_string();
    let ok_idx;
    let limit = match agent_ptr_access(ctx) {
        AgentPtrAccess::Call => return None,
        // The runtime publishes no stack limit on Windows (`stack_bounds` is
        // `None` there), so the slot stays 0 and a check could never fire:
        // emit none, as before the block was reachable inline on Windows.
        AgentPtrAccess::WindowsTeb => return None,
        access @ AgentPtrAccess::ExecutableTls => {
            ok_idx = ctx.new_block("stack_guard.ok");
            let at = super::agent_ptr::emit_slot_addr(ctx, access, &slot_off);
            ctx.block().load(PTR, &at)
        }
        AgentPtrAccess::AppleTsd => {
            let lookup = super::hot_tls::emit_hot_tls_lookup(ctx, "stack_guard");
            ok_idx = ctx.new_block("stack_guard.ok");
            // Hot cache not ready yet: skip the check.
            let ok_label = ctx.block_label(ok_idx);
            let fast_idx = ctx.current_block;
            ctx.current_block = lookup.slow_idx;
            ctx.block().br(&ok_label);
            ctx.current_block = fast_idx;
            let field = super::hot_tls::hot_tls_field(
                ctx,
                &lookup.hot,
                &HOT_TLS_AGENT_PTRS_OFFSET.to_string(),
            );
            let blk = ctx.block();
            let block_ptr = blk.load(PTR, &field);
            let at = blk.gep(I8, &block_ptr, &[(I64, &slot_off)]);
            blk.load(PTR, &at)
        }
    };
    let overflow_idx = ctx.new_block("stack_guard.overflow");
    let ok_label = ctx.block_label(ok_idx);
    let overflow_label = ctx.block_label(overflow_idx);
    let blk = ctx.block();
    let sp = blk.next_reg();
    blk.emit_raw(format!(
        "{sp} = call i64 @llvm.read_register.i64(metadata !{{!\"{sp_register}\"}})"
    ));
    let limit = blk.ptrtoint(&limit, I64);
    let deep = blk.icmp_ult(I64, &sp, &limit);
    blk.cond_br(&deep, &overflow_label, &ok_label);
    ctx.current_block = overflow_idx;
    ctx.block().call_void("js_stack_overflow", &[]);
    match arm {
        Arm::EndsHere => ctx.block().unreachable(),
        Arm::RejoinsTheCall => ctx.block().br(&ok_label),
    }
    ctx.current_block = ok_idx;
    Some(())
}
