//! One stack check per path through a frame.
//!
//! A compiled body tests its stack pointer against the agent's limit
//! (`expr/stack_guard.rs`): at its entry, or before the blocks that hold its
//! calls. `always-inline` then copies the callee's checks into the caller, so
//! a body that inlines nine helpers tests the same stack pointer against the
//! same limit ten times on one path. In the chess engine's `describe` that was
//! 18 instructions per call instead of 2.
//!
//! A check that another check of the same frame dominates cannot fire:
//! - the stack pointer is the frame's. Every `alloca` is in the entry block
//!   (`function/entry_allocas.rs`), so it does not move inside an activation.
//!   A shrink-wrapped prologue can put one frame between two reads; the
//!   limit's reserve (`perry-runtime/src/stack_guard.rs`) covers many.
//! - the limit is published once per thread, before the agent's first
//!   allocation, and changes after that only inside `js_stack_overflow`, which
//!   does not return.
//!
//! So the dominated check is given a constant condition and the optimizer
//! drops it with its overflow arm. The first check on every path through an
//! activation stays, and with it the check on every cycle of calls: a
//! recursion enters a new activation each time round.
//!
//! The pass runs after `always-inline` and before the statepoint rewrite, so
//! a dropped arm's call never becomes a statepoint with the body's roots.
//!
//! A check is recognized by its shape, `br (icmp ult (llvm.read_register),
//! limit), overflow, ok`. The stack check is the only reader of the stack
//! pointer register in compiled code (`stack_guard::emit_check` and the
//! checked-integer clone's copy of it).
//!
//! On Apple aarch64 the limit is read through the thread's hot cache
//! (`expr/hot_tls.rs`), and a thread that has not published one skips the
//! check: the lookup's tests branch around it into the `ok` block. Such a
//! skip is the same statement as a null limit ("no limit known"), made before
//! the thread's first allocation, so it does not weaken the check that
//! follows it. It is recognized by what it tests: a value the limit is
//! computed from, against a constant.

use inkwell::module::Module;
use llvm_sys::core::*;
use llvm_sys::prelude::*;
use llvm_sys::{LLVMIntPredicate, LLVMOpcode};
use std::collections::HashMap;

/// `PERRY_PROLOGUE_COST` (default on). `=0`/`off`/`false` keeps every check.
pub(crate) fn prologue_cost_enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        prologue_cost_enabled_from_env(std::env::var("PERRY_PROLOGUE_COST").ok().as_deref())
    })
}

pub(crate) fn prologue_cost_enabled_from_env(value: Option<&str>) -> bool {
    !matches!(value, Some("0" | "off" | "false"))
}

/// Returns the number of checks dropped.
pub(super) fn apply(module: &Module<'_>) -> usize {
    if !prologue_cost_enabled() {
        return 0;
    }
    drop_dominated(module, 2).0
}

/// Test seam: the number of stack checks in `ll_text` and how many of them
/// cover what follows (their `ok` block is entered only through the check or
/// its own skip). Ties the shapes this pass reads to the ones
/// `stack_guard::emit_check` writes, per target.
#[cfg(test)]
pub(crate) fn recognized_checks(ll_text: &str) -> (usize, usize) {
    let ctx = inkwell::context::Context::create();
    let module = super::parse_ir_text(&ctx, ll_text, "stack_checks").unwrap();
    let (_, found, covering) = drop_dominated(&module, 1);
    (found, covering)
}

/// A stack check: the block it ends, and its two arms.
struct Check {
    block: usize,
    overflow: usize,
    ok: usize,
    /// `(block, arm)`: `block` ends in `br (icmp ne v, constant), _, arm`
    /// where the limit is computed from `v`. `arm` is taken when the thread
    /// has published no limit.
    unpublished: Vec<(usize, usize)>,
}

/// Returns (checks dropped, checks found, checks that cover what follows),
/// looking only at bodies with at least `fewest` checks.
fn drop_dominated(module: &Module<'_>, fewest: usize) -> (usize, usize, usize) {
    let (mut dropped, mut found, mut covering) = (0, 0, 0);
    unsafe {
        let ctx = LLVMGetModuleContext(module.as_mut_ptr());
        let never = LLVMConstInt(LLVMInt1TypeInContext(ctx), 0, 0);
        for function in module.get_functions() {
            let blocks: Vec<LLVMBasicBlockRef> = function
                .get_basic_blocks()
                .iter()
                .map(|block| block.as_mut_ptr())
                .collect();
            // Cheap first pass: most bodies hold at most one check.
            let branches: Vec<(usize, LLVMValueRef)> = blocks
                .iter()
                .enumerate()
                .filter_map(|(i, &block)| stack_check_branch(block).map(|br| (i, br)))
                .collect();
            if branches.len() < fewest {
                continue;
            }
            let index: HashMap<LLVMBasicBlockRef, usize> =
                blocks.iter().enumerate().map(|(i, &b)| (b, i)).collect();
            let succ: Vec<Vec<usize>> = blocks
                .iter()
                .map(|&block| {
                    let terminator = LLVMGetBasicBlockTerminator(block);
                    if terminator.is_null() {
                        return Vec::new();
                    }
                    (0..LLVMGetNumSuccessors(terminator))
                        .filter_map(|i| index.get(&LLVMGetSuccessor(terminator, i)).copied())
                        .collect()
                })
                .collect();
            // Blocks that only branch on.
            let forwards: Vec<bool> = blocks
                .iter()
                .map(|&block| {
                    let first = LLVMGetFirstInstruction(block);
                    !first.is_null()
                        && first == LLVMGetBasicBlockTerminator(block)
                        && LLVMGetInstructionOpcode(first) == LLVMOpcode::LLVMBr
                        && LLVMIsConditional(first) == 0
                })
                .collect();
            let (checks, branches): (Vec<Check>, Vec<LLVMValueRef>) = branches
                .into_iter()
                .filter_map(|(block, branch)| {
                    let check = Check {
                        block,
                        overflow: *index.get(&LLVMGetSuccessor(branch, 0))?,
                        ok: *index.get(&LLVMGetSuccessor(branch, 1))?,
                        unpublished: unpublished_tests(branch, &index),
                    };
                    Some((check, branch))
                })
                .unzip();
            let (dominated, covers) = dominated_checks(&succ, &forwards, &checks);
            found += checks.len();
            covering += covers;
            for i in dominated {
                LLVMSetCondition(branches[i], never);
                dropped += 1;
            }
        }
    }
    (dropped, found, covering)
}

/// The conditional branch ending `block`, if it is a stack check.
unsafe fn stack_check_branch(block: LLVMBasicBlockRef) -> Option<LLVMValueRef> {
    unsafe {
        let branch = LLVMGetBasicBlockTerminator(block);
        if branch.is_null()
            || LLVMGetInstructionOpcode(branch) != LLVMOpcode::LLVMBr
            || LLVMIsConditional(branch) == 0
        {
            return None;
        }
        let deep = LLVMGetCondition(branch);
        if LLVMIsAICmpInst(deep).is_null()
            || LLVMGetICmpPredicate(deep) != LLVMIntPredicate::LLVMIntULT
        {
            return None;
        }
        let sp = LLVMGetOperand(deep, 0);
        if LLVMIsACallInst(sp).is_null() {
            return None;
        }
        let callee = LLVMGetCalledValue(sp);
        if LLVMIsAFunction(callee).is_null() {
            return None;
        }
        let mut len = 0usize;
        let name = LLVMGetValueName2(callee, &mut len);
        std::slice::from_raw_parts(name.cast::<u8>(), len)
            .starts_with(b"llvm.read_register.")
            .then_some(branch)
    }
}

/// The most instructions followed back from a check's limit.
const LIMIT_CHAIN: usize = 32;

/// The tests of "this thread has published no limit" in front of a check:
/// conditional branches on `icmp ne v, constant` where the check's limit is
/// computed from `v`, each with the arm it takes when they are equal.
unsafe fn unpublished_tests(
    branch: LLVMValueRef,
    index: &HashMap<LLVMBasicBlockRef, usize>,
) -> Vec<(usize, usize)> {
    unsafe {
        let mut tests = Vec::new();
        // The instructions the limit is computed from, nearest first. A phi
        // is not followed: its value is not one computation.
        let mut chain = vec![LLVMGetOperand(LLVMGetCondition(branch), 1)];
        let mut next = 0;
        while next < chain.len() && chain.len() < LIMIT_CHAIN {
            let value = chain[next];
            next += 1;
            if LLVMIsAInstruction(value).is_null() || !LLVMIsAPHINode(value).is_null() {
                continue;
            }
            for i in 0..LLVMGetNumOperands(value) {
                let operand = LLVMGetOperand(value, i as u32);
                if !LLVMIsAInstruction(operand).is_null() && !chain.contains(&operand) {
                    chain.push(operand);
                }
            }
        }
        for value in chain {
            if LLVMIsAInstruction(value).is_null() {
                continue;
            }
            let mut use_ = LLVMGetFirstUse(value);
            while !use_.is_null() {
                let test = LLVMGetUser(use_);
                use_ = LLVMGetNextUse(use_);
                if LLVMIsAICmpInst(test).is_null()
                    || LLVMGetICmpPredicate(test) != LLVMIntPredicate::LLVMIntNE
                {
                    continue;
                }
                let other = if LLVMGetOperand(test, 0) == value {
                    LLVMGetOperand(test, 1)
                } else {
                    LLVMGetOperand(test, 0)
                };
                if LLVMIsAConstant(other).is_null() {
                    continue;
                }
                let mut user = LLVMGetFirstUse(test);
                while !user.is_null() {
                    let br = LLVMGetUser(user);
                    user = LLVMGetNextUse(user);
                    if LLVMIsABranchInst(br).is_null()
                        || LLVMIsConditional(br) == 0
                        || LLVMGetCondition(br) != test
                    {
                        continue;
                    }
                    let block = index.get(&LLVMGetInstructionParent(br));
                    let arm = index.get(&LLVMGetSuccessor(br, 1));
                    if let (Some(&block), Some(&arm)) = (block, arm) {
                        tests.push((block, arm));
                    }
                }
            }
        }
        tests
    }
}

/// Indices of the checks that another check dominates, over the CFG `succ`
/// (block 0 is the entry). `forwards[b]`: block `b` only branches on.
///
/// A check dominates what its `ok` edge dominates. The `ok` block stands for
/// that edge when nothing else enters it. Every other predecessor is one of:
/// - a block the `ok` block dominates (a back edge, which came through the
///   edge first);
/// - the check's own overflow arm, which only the check enters and which
///   does not come back from `js_stack_overflow`;
/// - the check's skip for a thread without a limit: a block that only
///   branches on, entered only from the check's own tests of that
///   ([`Check::unpublished`]), each of which dominates the check.
///
/// Also returns how many checks have such an `ok` block.
fn dominated_checks(
    succ: &[Vec<usize>],
    forwards: &[bool],
    checks: &[Check],
) -> (Vec<usize>, usize) {
    const NONE: usize = usize::MAX;
    let n = succ.len();
    if n == 0 {
        return (Vec::new(), 0);
    }
    // Reverse postorder from the entry.
    let mut order = Vec::with_capacity(n);
    let mut seen = vec![false; n];
    let mut stack = vec![(0usize, 0usize)];
    seen[0] = true;
    while let Some((block, next)) = stack.last_mut() {
        if let Some(&to) = succ[*block].get(*next) {
            *next += 1;
            if !seen[to] {
                seen[to] = true;
                stack.push((to, 0));
            }
        } else {
            order.push(*block);
            stack.pop();
        }
    }
    order.reverse();
    let mut rank = vec![NONE; n];
    for (i, &block) in order.iter().enumerate() {
        rank[block] = i;
    }
    let mut pred: Vec<Vec<usize>> = vec![Vec::new(); n];
    for &block in &order {
        for &to in &succ[block] {
            pred[to].push(block);
        }
    }
    // Immediate dominators (Cooper, Harvey, Kennedy).
    let mut idom = vec![NONE; n];
    idom[0] = 0;
    loop {
        let mut changed = false;
        for &block in &order[1..] {
            let mut new = NONE;
            for &p in &pred[block] {
                if idom[p] == NONE {
                    continue;
                }
                if new == NONE {
                    new = p;
                    continue;
                }
                let (mut a, mut b) = (p, new);
                while a != b {
                    while rank[a] > rank[b] {
                        a = idom[a];
                    }
                    while rank[b] > rank[a] {
                        b = idom[b];
                    }
                }
                new = a;
            }
            if idom[block] != new {
                idom[block] = new;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    // Dominator tree intervals: `a` dominates `b` iff a's interval holds b's.
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); n];
    for &block in &order[1..] {
        children[idom[block]].push(block);
    }
    let mut enter = vec![0usize; n];
    let mut leave = vec![0usize; n];
    let mut clock = 0usize;
    let mut stack = vec![(0usize, 0usize)];
    let mut tree_order = Vec::with_capacity(order.len());
    while let Some((block, next)) = stack.last_mut() {
        if *next == 0 {
            enter[*block] = clock;
            clock += 1;
            tree_order.push(*block);
        }
        if let Some(&child) = children[*block].get(*next) {
            *next += 1;
            stack.push((child, 0));
        } else {
            leave[*block] = clock;
            clock += 1;
            stack.pop();
        }
    }
    let dominates = |a: usize, b: usize| {
        rank[a] != NONE && rank[b] != NONE && enter[a] <= enter[b] && leave[b] <= leave[a]
    };
    // `ok` blocks that stand for their check's edge.
    let mut guards = vec![false; n];
    let mut covering = 0;
    for check in checks {
        if rank[check.block] == NONE || check.ok == check.overflow {
            continue;
        }
        let arm_is_private = pred[check.overflow] == [check.block];
        let skips_unpublished = |p: usize| {
            forwards[p]
                && !pred[p].is_empty()
                && pred[p].iter().all(|&test| {
                    check.unpublished.contains(&(test, p)) && dominates(test, check.block)
                })
        };
        let stands = pred[check.ok].iter().all(|&p| {
            p == check.block
                || (p == check.overflow && arm_is_private)
                || dominates(check.ok, p)
                || skips_unpublished(p)
        });
        guards[check.ok] |= stands;
        covering += usize::from(stands);
    }
    // Number of guarding `ok` blocks among each block's dominators (itself
    // included: a check in an `ok` block runs after the edge into it).
    let mut depth = vec![0usize; n];
    for &block in &tree_order {
        let above = if block == 0 { 0 } else { depth[idom[block]] };
        depth[block] = above + usize::from(guards[block]);
    }
    let dominated = checks
        .iter()
        .enumerate()
        .filter(|(_, check)| rank[check.block] != NONE && depth[check.block] > 0)
        .map(|(i, _)| i)
        .collect();
    (dominated, covering)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inprocess::parse_ir_text;
    use inkwell::context::Context;

    const IR: &str = r#"
declare i64 @llvm.read_register.i64(metadata)
declare void @js_stack_overflow()
declare void @work()
define void @inlined(i64 %limit) {
entry:
  %sp = call i64 @llvm.read_register.i64(metadata !0)
  %deep = icmp ult i64 %sp, %limit
  br i1 %deep, label %over, label %ok
over:
  call void @js_stack_overflow()
  unreachable
ok:
  call void @work()
  %sp2 = call i64 @llvm.read_register.i64(metadata !0)
  %deep2 = icmp ult i64 %sp2, %limit
  br i1 %deep2, label %over2, label %ok2
over2:
  call void @js_stack_overflow()
  br label %ok2
ok2:
  call void @work()
  ret void
}
define void @one_arm_only(i64 %limit, i1 %c) {
entry:
  br i1 %c, label %arm, label %join
arm:
  %sp = call i64 @llvm.read_register.i64(metadata !0)
  %deep = icmp ult i64 %sp, %limit
  br i1 %deep, label %over, label %ok
over:
  call void @js_stack_overflow()
  unreachable
ok:
  call void @work()
  br label %join
join:
  %sp2 = call i64 @llvm.read_register.i64(metadata !0)
  %deep2 = icmp ult i64 %sp2, %limit
  br i1 %deep2, label %over2, label %ok2
over2:
  call void @js_stack_overflow()
  unreachable
ok2:
  call void @work()
  ret void
}
define void @both_arms(i64 %limit, i1 %c) {
entry:
  br i1 %c, label %a, label %b
a:
  %spa = call i64 @llvm.read_register.i64(metadata !0)
  %deepa = icmp ult i64 %spa, %limit
  br i1 %deepa, label %overa, label %oka
overa:
  call void @js_stack_overflow()
  unreachable
oka:
  %spa2 = call i64 @llvm.read_register.i64(metadata !0)
  %deepa2 = icmp ult i64 %spa2, %limit
  br i1 %deepa2, label %overa2, label %join
overa2:
  call void @js_stack_overflow()
  unreachable
b:
  %spb = call i64 @llvm.read_register.i64(metadata !0)
  %deepb = icmp ult i64 %spb, %limit
  br i1 %deepb, label %overb, label %join
overb:
  call void @js_stack_overflow()
  unreachable
join:
  %sp2 = call i64 @llvm.read_register.i64(metadata !0)
  %deep2 = icmp ult i64 %sp2, %limit
  br i1 %deep2, label %over2, label %ok2
over2:
  call void @js_stack_overflow()
  unreachable
ok2:
  ret void
}
define void @rejoining_arm(i64 %limit) {
entry:
  %sp = call i64 @llvm.read_register.i64(metadata !0)
  %deep = icmp ult i64 %sp, %limit
  br i1 %deep, label %over, label %ok
over:
  call void @js_stack_overflow()
  br label %ok
ok:
  %sp2 = call i64 @llvm.read_register.i64(metadata !0)
  %deep2 = icmp ult i64 %sp2, %limit
  br i1 %deep2, label %over2, label %ok2
over2:
  call void @js_stack_overflow()
  unreachable
ok2:
  ret void
}
define void @ok_entered_from_elsewhere(i64 %limit, i1 %c) {
entry:
  br i1 %c, label %checked, label %ok
checked:
  %sp = call i64 @llvm.read_register.i64(metadata !0)
  %deep = icmp ult i64 %sp, %limit
  br i1 %deep, label %over, label %ok
over:
  call void @js_stack_overflow()
  unreachable
ok:
  %sp2 = call i64 @llvm.read_register.i64(metadata !0)
  %deep2 = icmp ult i64 %sp2, %limit
  br i1 %deep2, label %over2, label %ok2
over2:
  call void @js_stack_overflow()
  unreachable
ok2:
  ret void
}
define void @shared_arm(i64 %limit, i1 %c) {
entry:
  br i1 %c, label %checked, label %over
checked:
  %sp = call i64 @llvm.read_register.i64(metadata !0)
  %deep = icmp ult i64 %sp, %limit
  br i1 %deep, label %over, label %ok
over:
  call void @work()
  br label %ok
ok:
  %sp2 = call i64 @llvm.read_register.i64(metadata !0)
  %deep2 = icmp ult i64 %sp2, %limit
  br i1 %deep2, label %over2, label %ok2
over2:
  call void @js_stack_overflow()
  unreachable
ok2:
  ret void
}
define void @loops(i64 %limit, i1 %c) {
entry:
  br label %head
head:
  %sp = call i64 @llvm.read_register.i64(metadata !0)
  %deep = icmp ult i64 %sp, %limit
  br i1 %deep, label %over, label %body
over:
  call void @js_stack_overflow()
  unreachable
body:
  %sp2 = call i64 @llvm.read_register.i64(metadata !0)
  %deep2 = icmp ult i64 %sp2, %limit
  br i1 %deep2, label %over2, label %latch
over2:
  call void @js_stack_overflow()
  unreachable
latch:
  br i1 %c, label %head, label %out
out:
  ret void
}
define void @other_compares(i64 %limit, i64 %x) {
entry:
  %sp = call i64 @llvm.read_register.i64(metadata !0)
  %deep = icmp ult i64 %sp, %limit
  br i1 %deep, label %over, label %ok
over:
  call void @js_stack_overflow()
  unreachable
ok:
  %small = icmp ult i64 %x, %limit
  br i1 %small, label %a, label %b
a:
  %sp2 = call i64 @llvm.read_register.i64(metadata !0)
  %above = icmp ugt i64 %sp2, %limit
  br i1 %above, label %b, label %c
b:
  ret void
c:
  ret void
}
define void @hot_tls(ptr %keyp) {
entry:
  %key = load i64, ptr %keyp
  %has = icmp ne i64 %key, -1
  br i1 %has, label %tsd, label %slow
tsd:
  %addr = shl i64 %key, 3
  %slot = inttoptr i64 %addr to ptr
  %hot = load ptr, ptr %slot
  %published = icmp ne ptr %hot, null
  br i1 %published, label %fast, label %slow
slow:
  br label %ok
fast:
  %field = getelementptr i8, ptr %hot, i64 16
  %block = load ptr, ptr %field
  %at = getelementptr i8, ptr %block, i64 16
  %lim = load ptr, ptr %at
  %sp = call i64 @llvm.read_register.i64(metadata !0)
  %limit = ptrtoint ptr %lim to i64
  %deep = icmp ult i64 %sp, %limit
  br i1 %deep, label %over, label %ok
over:
  call void @js_stack_overflow()
  unreachable
ok:
  call void @work()
  %key2 = load i64, ptr %keyp
  %has2 = icmp ne i64 %key2, -1
  br i1 %has2, label %tsd2, label %slow2
tsd2:
  %addr2 = shl i64 %key2, 3
  %slot2 = inttoptr i64 %addr2 to ptr
  %hot2 = load ptr, ptr %slot2
  %published2 = icmp ne ptr %hot2, null
  br i1 %published2, label %fast2, label %slow2
slow2:
  br label %ok2
fast2:
  %field2 = getelementptr i8, ptr %hot2, i64 16
  %block2 = load ptr, ptr %field2
  %at2 = getelementptr i8, ptr %block2, i64 16
  %lim2 = load ptr, ptr %at2
  %sp2 = call i64 @llvm.read_register.i64(metadata !0)
  %limit2 = ptrtoint ptr %lim2 to i64
  %deep2 = icmp ult i64 %sp2, %limit2
  br i1 %deep2, label %over2, label %ok2
over2:
  call void @js_stack_overflow()
  unreachable
ok2:
  ret void
}
define void @skipped_for_another_reason(ptr %limitp, i64 %x) {
entry:
  %lim = load i64, ptr %limitp
  %wanted = icmp ne i64 %x, 0
  br i1 %wanted, label %checked, label %skip
skip:
  br label %ok
checked:
  %sp = call i64 @llvm.read_register.i64(metadata !0)
  %deep = icmp ult i64 %sp, %lim
  br i1 %deep, label %over, label %ok
over:
  call void @js_stack_overflow()
  unreachable
ok:
  %sp2 = call i64 @llvm.read_register.i64(metadata !0)
  %deep2 = icmp ult i64 %sp2, %lim
  br i1 %deep2, label %over2, label %ok2
over2:
  call void @js_stack_overflow()
  unreachable
ok2:
  ret void
}
define void @skip_block_does_work(ptr %limitp) {
entry:
  %lim = load i64, ptr %limitp
  %known = icmp ne i64 %lim, 0
  br i1 %known, label %checked, label %skip
skip:
  call void @work()
  br label %ok
checked:
  %sp = call i64 @llvm.read_register.i64(metadata !0)
  %deep = icmp ult i64 %sp, %lim
  br i1 %deep, label %over, label %ok
over:
  call void @js_stack_overflow()
  unreachable
ok:
  %sp2 = call i64 @llvm.read_register.i64(metadata !0)
  %deep2 = icmp ult i64 %sp2, %lim
  br i1 %deep2, label %over2, label %ok2
over2:
  call void @js_stack_overflow()
  unreachable
ok2:
  ret void
}
define void @single(i64 %limit) {
entry:
  %sp = call i64 @llvm.read_register.i64(metadata !0)
  %deep = icmp ult i64 %sp, %limit
  br i1 %deep, label %over, label %ok
over:
  call void @js_stack_overflow()
  unreachable
ok:
  ret void
}
!0 = !{!"rsp"}
"#;

    fn dropped() -> (String, usize) {
        let ctx = Context::create();
        let module = parse_ir_text(&ctx, IR, "stack_checks").unwrap();
        let (count, _, _) = drop_dominated(&module, 2);
        module.verify().expect("module verifies");
        (module.print_to_string().to_string(), count)
    }

    fn body<'a>(ir: &'a str, name: &str) -> &'a str {
        let start = ir
            .find(&format!("define void @{name}("))
            .unwrap_or_else(|| panic!("no @{name} in:\n{ir}"));
        let end = ir[start..].find("\n}\n").unwrap() + start;
        &ir[start..end]
    }

    /// The branches of `name` that still test a register read, and those
    /// given a constant condition.
    fn live_and_dropped(ir: &str, name: &str) -> (usize, usize) {
        let body = body(ir, name);
        (
            body.matches("br i1 %deep").count(),
            body.matches("br i1 false").count(),
        )
    }

    #[test]
    fn an_inlined_check_under_the_entry_check_is_dropped() {
        let (ir, _) = dropped();
        assert_eq!(live_and_dropped(&ir, "inlined"), (1, 1));
        assert!(
            body(&ir, "inlined").contains("br i1 %deep, label %over, label %ok\n"),
            "the entry check stays:\n{ir}"
        );
        assert!(body(&ir, "inlined").contains("br i1 false, label %over2, label %ok2"));
    }

    /// Negative control: a check on one arm does not cover the join.
    #[test]
    fn a_check_on_one_arm_does_not_cover_the_join() {
        let (ir, _) = dropped();
        assert_eq!(live_and_dropped(&ir, "one_arm_only"), (2, 0));
    }

    /// Negative control: each arm checks, but no single check dominates the
    /// join, so the join keeps its own. The second check on arm `a` goes.
    #[test]
    fn checks_on_both_arms_do_not_cover_the_join() {
        let (ir, _) = dropped();
        assert_eq!(live_and_dropped(&ir, "both_arms"), (3, 1));
        assert!(body(&ir, "both_arms").contains("br i1 false, label %overa2, label %join"));
    }

    #[test]
    fn an_arm_that_rejoins_still_stands_for_its_check() {
        let (ir, _) = dropped();
        assert_eq!(live_and_dropped(&ir, "rejoining_arm"), (1, 1));
    }

    /// Negative controls: the `ok` block has a way in that passed no check.
    #[test]
    fn an_ok_block_entered_without_the_check_covers_nothing() {
        let (ir, _) = dropped();
        assert_eq!(live_and_dropped(&ir, "ok_entered_from_elsewhere"), (2, 0));
        assert_eq!(live_and_dropped(&ir, "shared_arm"), (2, 0));
    }

    /// The loop's own check runs first on every path and stays; the one it
    /// dominates in the body goes. The back edge into the head is not a way
    /// around the check.
    #[test]
    fn a_check_in_a_loop_covers_the_body() {
        let (ir, _) = dropped();
        assert_eq!(live_and_dropped(&ir, "loops"), (1, 1));
        assert!(body(&ir, "loops").contains("br i1 %deep, label %over, label %body"));
    }

    /// Negative control: only `icmp ult` of a register read is a check.
    #[test]
    fn other_compares_are_left_alone() {
        let (ir, _) = dropped();
        let body = body(&ir, "other_compares");
        assert!(body.contains("br i1 %small, label %a, label %b"));
        assert!(body.contains("br i1 %above, label %b, label %c"));
        assert_eq!(body.matches("br i1 false").count(), 0);
    }

    /// The Apple aarch64 shape: the lookup's two tests skip the check for a
    /// thread without a limit. The first check still covers the second, whose
    /// own lookup stays for the optimizer to fold.
    #[test]
    fn a_skip_for_an_unpublished_limit_does_not_uncover() {
        let (ir, _) = dropped();
        assert_eq!(live_and_dropped(&ir, "hot_tls"), (1, 1));
        let body = body(&ir, "hot_tls");
        assert!(body.contains("br i1 %deep, label %over, label %ok\n"));
        assert!(body.contains("br i1 false, label %over2, label %ok2"));
    }

    /// Negative controls: a way round the check that tests something the
    /// limit is not computed from, and a skip block that does more than
    /// branch on.
    #[test]
    fn any_other_way_round_the_check_uncovers() {
        let (ir, _) = dropped();
        assert_eq!(live_and_dropped(&ir, "skipped_for_another_reason"), (2, 0));
        assert_eq!(live_and_dropped(&ir, "skip_block_does_work"), (2, 0));
    }

    #[test]
    fn a_single_check_stays_and_the_count_is_exact() {
        let (ir, count) = dropped();
        assert_eq!(live_and_dropped(&ir, "single"), (1, 0));
        assert_eq!(
            count, 5,
            "inlined, both_arms, rejoining_arm, loops, hot_tls"
        );
    }

    /// The two-part run is the one pass string, cut after the inlining.
    #[test]
    fn the_split_pipeline_is_the_statepoint_pipeline() {
        use crate::linker::*;
        assert_eq!(
            format!("{STATEPOINT_INLINE_PASS},{STATEPOINT_REWRITE_AFTER_INLINE}"),
            STATEPOINT_REWRITE_PASSES
        );
    }

    #[test]
    fn the_switch_turns_it_off() {
        assert!(prologue_cost_enabled_from_env(None));
        assert!(prologue_cost_enabled_from_env(Some("1")));
        for off in ["0", "off", "false"] {
            assert!(!prologue_cost_enabled_from_env(Some(off)));
        }
    }
}
