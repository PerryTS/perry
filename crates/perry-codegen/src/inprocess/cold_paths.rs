//! Cold arms stay cold through the statepoint rewrite.
//!
//! LLVM learns that a call is unlikely from `cold`. Block frequencies follow,
//! and with them block placement and the register allocator's split and spill
//! decisions: a value that is live across an unlikely call is saved and
//! restored next to that call instead of living in memory on the likely path.
//!
//! On a helper's declaration the fact does not survive.
//! `rewrite-statepoints-for-gc` runs before the optimizer and replaces a
//! collecting call by `gc.statepoint(..., @callee, ...)`, keeping the
//! attributes of the call SITE only. The loop poll's call and the shared cold
//! element read are collecting calls inside the hottest loops: LLVM weighed
//! them like the code around them, and values live across them were stored
//! and reloaded on the likely path.
//!
//! This pass runs before the rewrite. For the helpers that exist only as the
//! slow arm of an inline fast path (`module::is_cold_arm_helper`):
//! - every call gets the `cold` call-site attribute;
//! - a conditional branch with exactly one side that cannot avoid such a call
//!   gets the `branch_weights` an `llvm.expect` on its condition would give.
//!
//! Nothing about the call changes: same callee, same convention, same
//! statepoint, same roots. The cost is code size: the save and restore that
//! one spill used to share now sit at each cold call.

use inkwell::module::Module;
use inkwell::values::{AsValueRef, InstructionOpcode};
use llvm_sys::core::*;
use llvm_sys::prelude::*;
use llvm_sys::{LLVMAttributeFunctionIndex, LLVMOpcode};
use std::collections::HashSet;

/// LLVM's own `llvm.expect` weights (`-likely-branch-weight`,
/// `-unlikely-branch-weight`).
const LIKELY: u64 = 2000;
const UNLIKELY: u64 = 1;

pub(super) fn apply(module: &Module<'_>) -> (usize, usize) {
    if !crate::module::cold_paths_enabled() {
        return (0, 0);
    }
    mark(module)
}

/// Returns (call sites marked, branches weighted).
fn mark(module: &Module<'_>) -> (usize, usize) {
    let (mut sites, mut branches) = (0, 0);
    unsafe {
        let ctx = LLVMGetModuleContext(module.as_mut_ptr());
        let cold_kind = LLVMGetEnumAttributeKindForName(c"cold".as_ptr(), 4);
        let prof_kind = LLVMGetMDKindIDInContext(ctx, c"prof".as_ptr(), 4);
        let i32_ty = LLVMInt32TypeInContext(ctx);
        for function in module.get_functions() {
            let blocks: Vec<LLVMBasicBlockRef> = function
                .get_basic_blocks()
                .iter()
                .map(|block| block.as_mut_ptr())
                .collect();
            let mut cold: HashSet<LLVMBasicBlockRef> = HashSet::new();
            for block in function.get_basic_blocks() {
                for instruction in block.get_instructions() {
                    if !matches!(
                        instruction.get_opcode(),
                        InstructionOpcode::Call | InstructionOpcode::Invoke
                    ) {
                        continue;
                    }
                    let site = instruction.as_value_ref();
                    let callee = LLVMGetCalledValue(site);
                    if LLVMIsAFunction(callee).is_null() {
                        continue;
                    }
                    let mut len = 0usize;
                    let name = LLVMGetValueName2(callee, &mut len);
                    let name = std::slice::from_raw_parts(name.cast::<u8>(), len);
                    if !std::str::from_utf8(name).is_ok_and(crate::module::is_cold_arm_helper) {
                        continue;
                    }
                    LLVMAddCallSiteAttribute(
                        site,
                        LLVMAttributeFunctionIndex,
                        LLVMCreateEnumAttribute(ctx, cold_kind, 0),
                    );
                    sites += 1;
                    cold.insert(block.as_mut_ptr());
                }
            }
            if cold.is_empty() {
                continue;
            }
            let branch = |block: LLVMBasicBlockRef| {
                let terminator = LLVMGetBasicBlockTerminator(block);
                (!terminator.is_null()
                    && LLVMGetInstructionOpcode(terminator) == LLVMOpcode::LLVMBr)
                    .then_some(terminator)
            };
            // A block whose every way on leads into a cold block cannot avoid
            // the call either.
            loop {
                let mut grew = false;
                for &block in &blocks {
                    if cold.contains(&block) {
                        continue;
                    }
                    let Some(br) = branch(block) else { continue };
                    let n = LLVMGetNumSuccessors(br);
                    if n > 0 && (0..n).all(|i| cold.contains(&LLVMGetSuccessor(br, i))) {
                        cold.insert(block);
                        grew = true;
                    }
                }
                if !grew {
                    break;
                }
            }
            for &block in &blocks {
                if cold.contains(&block) {
                    continue;
                }
                let Some(br) = branch(block) else { continue };
                if LLVMIsConditional(br) == 0 || !LLVMGetMetadata(br, prof_kind).is_null() {
                    continue;
                }
                let taken = cold.contains(&LLVMGetSuccessor(br, 0));
                let not_taken = cold.contains(&LLVMGetSuccessor(br, 1));
                if taken == not_taken {
                    continue;
                }
                let weight = |is_cold: bool| {
                    let w = if is_cold { UNLIKELY } else { LIKELY };
                    LLVMValueAsMetadata(LLVMConstInt(i32_ty, w, 0))
                };
                let mut operands = [
                    LLVMMDStringInContext2(ctx, c"branch_weights".as_ptr(), 14),
                    weight(taken),
                    weight(not_taken),
                ];
                let node = LLVMMDNodeInContext2(ctx, operands.as_mut_ptr(), operands.len());
                LLVMSetMetadata(br, prof_kind, LLVMMetadataAsValue(ctx, node));
                branches += 1;
            }
        }
    }
    (sites, branches)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inprocess::{parse_ir_text, statepoint_rewritten_ir_with_passes};
    use crate::linker::STATEPOINT_REWRITE_PASSES;
    use inkwell::context::Context;

    const IR: &str = r#"
declare void @js_gc_loop_safepoint() cold
declare void @js_object_get_field_slow() cold
declare void @plain()
declare i32 @personality(...)
define void @diamond(i1 %c, ptr %f) {
entry:
  br i1 %c, label %arm, label %join
arm:
  call void @js_gc_loop_safepoint()
  call void %f()
  br label %join
join:
  ret void
}
define void @cold_on_false(i1 %c) {
entry:
  br i1 %c, label %join, label %arm
arm:
  call void @js_gc_loop_safepoint()
  br label %join
join:
  ret void
}
define void @through_a_block(i1 %c, i1 %d) {
entry:
  br i1 %c, label %setup, label %join
setup:
  br i1 %d, label %arm.a, label %arm.b
arm.a:
  br label %arm
arm.b:
  br label %arm
arm:
  call void @js_gc_loop_safepoint()
  br label %join
join:
  ret void
}
define void @both_cold(i1 %c, i1 %d) {
entry:
  br i1 %d, label %pick, label %out
pick:
  br i1 %c, label %a, label %b
a:
  call void @js_gc_loop_safepoint()
  ret void
b:
  call void @js_gc_loop_safepoint()
  ret void
out:
  ret void
}
define void @not_cold(i1 %c) {
entry:
  br i1 %c, label %arm, label %join
arm:
  call void @plain()
  br label %join
join:
  ret void
}
define void @declared_cold_only(i1 %c) {
entry:
  br i1 %c, label %arm, label %join
arm:
  call void @js_object_get_field_slow()
  br label %join
join:
  ret void
}
define void @weighted_already(i1 %c) {
entry:
  br i1 %c, label %arm, label %join, !prof !0
arm:
  call void @js_gc_loop_safepoint()
  br label %join
join:
  ret void
}
define void @invoked(i1 %c) personality ptr @personality {
entry:
  br i1 %c, label %arm, label %join
arm:
  invoke void @js_gc_loop_safepoint() to label %join unwind label %pad
join:
  ret void
pad:
  %p = landingpad { ptr, i32 } cleanup
  resume { ptr, i32 } %p
}
!0 = !{!"branch_weights", i32 7, i32 9}
"#;

    fn marked() -> (String, (usize, usize)) {
        let ctx = Context::create();
        let module = parse_ir_text(&ctx, IR, "cold_paths").unwrap();
        let counts = mark(&module);
        module.verify().expect("marked module verifies");
        (module.print_to_string().to_string(), counts)
    }

    fn body<'a>(ir: &'a str, name: &str) -> &'a str {
        let start = ir
            .find(&format!("define void @{name}("))
            .unwrap_or_else(|| panic!("no @{name} in:\n{ir}"));
        let end = ir[start..].find("\n}\n").unwrap() + start;
        &ir[start..end]
    }

    /// The attribute group of the first line of `ir` that contains `needle`.
    fn site_attrs(ir: &str, needle: &str) -> String {
        let site = ir
            .lines()
            .find(|l| l.contains(needle))
            .unwrap_or_else(|| panic!("no `{needle}` in:\n{ir}"));
        let Some(group) = site.split(" #").nth(1) else {
            return String::new();
        };
        let head = format!(
            "attributes #{} = ",
            group.split_whitespace().next().unwrap()
        );
        ir.lines()
            .find(|l| l.starts_with(&head))
            .unwrap_or("")
            .to_string()
    }

    /// The node the `!prof !N` of the line of `text` containing `needle` names.
    fn weights<'a>(ir: &'a str, text: &str, needle: &str) -> Option<&'a str> {
        let line = text.lines().find(|l| l.contains(needle))?;
        let id = line.split("!prof !").nth(1)?.split_whitespace().next()?;
        let def = format!("\n!{id} = ");
        let at = ir.find(&def)? + def.len();
        ir[at..].lines().next()
    }

    #[test]
    fn a_cold_callee_marks_its_sites_and_the_branch_into_them() {
        let (ir, (sites, branches)) = marked();
        // diamond, cold_on_false, through_a_block, both_cold x2,
        // weighted_already, invoked.
        assert_eq!(sites, 7, "{ir}");
        // diamond, cold_on_false, through_a_block (entry), both_cold (entry),
        // invoked.
        assert_eq!(branches, 5, "{ir}");
        let diamond = body(&ir, "diamond");
        assert!(
            site_attrs(&ir, "call void @js_gc_loop_safepoint()").contains("cold"),
            "{ir}"
        );
        assert!(
            site_attrs(&ir, "invoke void @js_gc_loop_safepoint()").contains("cold"),
            "{ir}"
        );
        assert_eq!(
            weights(&ir, diamond, "br i1 %c"),
            Some("!{!\"branch_weights\", i32 1, i32 2000}"),
            "{ir}"
        );
        assert_eq!(
            weights(&ir, body(&ir, "cold_on_false"), "br i1 %c"),
            Some("!{!\"branch_weights\", i32 2000, i32 1}"),
            "{ir}"
        );
        assert_eq!(
            weights(&ir, body(&ir, "invoked"), "br i1 %c"),
            Some("!{!\"branch_weights\", i32 1, i32 2000}"),
            "{ir}"
        );
    }

    #[test]
    fn a_block_that_only_leads_into_a_cold_call_is_cold_too() {
        let (ir, _) = marked();
        let f = body(&ir, "through_a_block");
        assert_eq!(
            weights(&ir, f, "br i1 %c"),
            Some("!{!\"branch_weights\", i32 1, i32 2000}"),
            "{f}"
        );
        // `setup` chooses between two ways into the same call: neither side
        // is the likely one.
        assert_eq!(weights(&ir, f, "br i1 %d"), None, "{f}");
        let f = body(&ir, "both_cold");
        assert_eq!(weights(&ir, f, "br i1 %c"), None, "{f}");
        assert_eq!(
            weights(&ir, f, "br i1 %d"),
            Some("!{!\"branch_weights\", i32 1, i32 2000}"),
            "{f}"
        );
    }

    #[test]
    fn negative_controls_keep_their_calls_and_branches() {
        let (ir, _) = marked();
        let plain = body(&ir, "not_cold");
        assert!(!plain.contains("!prof"), "{ir}");
        assert_eq!(site_attrs(&ir, "call void @plain()"), "", "{ir}");
        // `cold` on a declaration alone does not make a helper a cold arm.
        assert!(!body(&ir, "declared_cold_only").contains("!prof"), "{ir}");
        assert_eq!(
            site_attrs(&ir, "call void @js_object_get_field_slow()"),
            "",
            "{ir}"
        );
        assert_eq!(site_attrs(&ir, "call void %f()"), "", "{ir}");
        assert_eq!(
            weights(&ir, body(&ir, "weighted_already"), "br i1 %c"),
            Some("!{!\"branch_weights\", i32 7, i32 9}"),
            "weights a lowering wrote are kept:\n{ir}"
        );
    }

    const GC_IR: &str = r#"
declare void @js_gc_loop_safepoint() cold
define ptr addrspace(1) @rooted(ptr addrspace(1) %p, i1 %c) gc "statepoint-example" {
entry:
  br i1 %c, label %arm, label %join
arm:
  call void @js_gc_loop_safepoint()
  br label %join
join:
  ret ptr addrspace(1) %p
}
"#;

    /// What this pass exists for: the rewrite drops the callee's `cold`
    /// (negative control) and keeps the call site's. The roots are the same
    /// either way.
    #[test]
    fn the_statepoint_rewrite_keeps_only_the_call_site_attribute() {
        let target = "x86_64-unknown-linux-gnu";
        let statepoint = "@llvm.experimental.gc.statepoint";
        let unmarked = statepoint_rewritten_ir_with_passes(
            GC_IR,
            target,
            "unmarked",
            STATEPOINT_REWRITE_PASSES,
        )
        .unwrap();
        assert!(
            !site_attrs(&unmarked, statepoint).contains("cold"),
            "without the pass the statepoint does not say cold:\n{unmarked}"
        );
        assert!(!unmarked.contains("!prof"), "{unmarked}");

        let ctx = Context::create();
        let module = parse_ir_text(&ctx, GC_IR, "marked").unwrap();
        assert_eq!(mark(&module), (1, 1));
        let marked = statepoint_rewritten_ir_with_passes(
            &module.print_to_string().to_string(),
            target,
            "marked",
            STATEPOINT_REWRITE_PASSES,
        )
        .unwrap();
        assert!(
            site_attrs(&marked, statepoint).contains("cold"),
            "the statepoint carries `cold`:\n{marked}"
        );
        assert!(
            marked.contains("!\"branch_weights\", i32 1, i32 2000"),
            "{marked}"
        );
        let relocations = |ir: &str| ir.matches("@llvm.experimental.gc.relocate").count();
        assert!(relocations(&marked) > 0, "{marked}");
        assert_eq!(relocations(&marked), relocations(&unmarked));
        assert_eq!(
            marked.matches("\"gc-live\"").count(),
            unmarked.matches("\"gc-live\"").count()
        );
    }
}
