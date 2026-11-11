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
    drop_dominated(module)
}

/// A stack check: the block it ends, and its two arms.
struct Check {
    block: usize,
    overflow: usize,
    ok: usize,
    branch: LLVMValueRef,
}

fn drop_dominated(module: &Module<'_>) -> usize {
    let mut dropped = 0;
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
            if branches.len() < 2 {
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
            let checks: Vec<Check> = branches
                .into_iter()
                .filter_map(|(block, branch)| {
                    Some(Check {
                        block,
                        overflow: *index.get(&LLVMGetSuccessor(branch, 0))?,
                        ok: *index.get(&LLVMGetSuccessor(branch, 1))?,
                        branch,
                    })
                })
                .collect();
            for i in dominated_checks(&succ, &checks) {
                LLVMSetCondition(checks[i].branch, never);
                dropped += 1;
            }
        }
    }
    dropped
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

/// Indices of the checks that another check dominates, over the CFG `succ`
/// (block 0 is the entry).
///
/// A check dominates what its `ok` edge dominates. The `ok` block stands for
/// that edge when nothing else enters it: every other predecessor is a block
/// the `ok` block dominates (a back edge, which came through the edge first),
/// or the check's own overflow arm, which only the check enters and which
/// does not come back from `js_stack_overflow`.
fn dominated_checks(succ: &[Vec<usize>], checks: &[Check]) -> Vec<usize> {
    const NONE: usize = usize::MAX;
    let n = succ.len();
    if n == 0 {
        return Vec::new();
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
    for check in checks {
        if rank[check.block] == NONE || check.ok == check.overflow {
            continue;
        }
        let arm_is_private = pred[check.overflow] == [check.block];
        let stands = pred[check.ok].iter().all(|&p| {
            p == check.block || (p == check.overflow && arm_is_private) || dominates(check.ok, p)
        });
        guards[check.ok] |= stands;
    }
    // Number of guarding `ok` blocks among each block's dominators (itself
    // included: a check in an `ok` block runs after the edge into it).
    let mut depth = vec![0usize; n];
    for &block in &tree_order {
        let above = if block == 0 { 0 } else { depth[idom[block]] };
        depth[block] = above + usize::from(guards[block]);
    }
    checks
        .iter()
        .enumerate()
        .filter(|(_, check)| rank[check.block] != NONE && depth[check.block] > 0)
        .map(|(i, _)| i)
        .collect()
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
        let count = drop_dominated(&module);
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

    #[test]
    fn a_single_check_stays_and_the_count_is_exact() {
        let (ir, count) = dropped();
        assert_eq!(live_and_dropped(&ir, "single"), (1, 0));
        assert_eq!(count, 4, "inlined, both_arms, rejoining_arm, loops");
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
