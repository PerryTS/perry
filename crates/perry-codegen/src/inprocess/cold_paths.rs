//! Cold paths stay cold through the statepoint rewrite.
//!
//! A `cold` helper declaration tells LLVM that the blocks leading to its
//! call are unlikely: block frequencies, block placement and, above all, the
//! register allocator's split and spill decisions follow. That information
//! lives on the callee. `rewrite-statepoints-for-gc` replaces a collecting
//! call by `gc.statepoint(..., @callee, ...)` and keeps only the attributes
//! of the call SITE, so for every collecting helper the fact was lost before
//! the optimizer and the backend ran: the loop poll's slow call, the shared
//! cold element read and the store slow paths were all weighed like the hot
//! path, and values live across them were spilled on the hot path too.
//!
//! This pass runs before the rewrite and states the fact where it survives:
//! - every call to a `cold` callee gets the `cold` call-site attribute;
//! - a conditional branch with exactly one successor that cannot avoid such
//!   a call gets explicit `branch_weights`, like the worker-interrupt poll's
//!   branch (`loop_interrupt`).
//!
//! Nothing about what the calls do changes: same callee, same convention,
//! same statepoint, same roots.

use inkwell::module::Module;
use inkwell::values::{AsValueRef, InstructionOpcode};
use llvm_sys::core::*;
use llvm_sys::prelude::*;
use llvm_sys::LLVMAttributeFunctionIndex;
use std::collections::HashSet;

/// Taken : not taken, the ratio of the worker-interrupt poll's branch.
const COLD_WEIGHT: u64 = 1;
const HOT_WEIGHT: u64 = 1_048_575;

/// `PERRY_COLD_PATHS=0` leaves the module as codegen built it.
pub(crate) fn enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| enabled_from_env(std::env::var("PERRY_COLD_PATHS").ok().as_deref()))
}

pub(crate) fn enabled_from_env(value: Option<&str>) -> bool {
    !matches!(value, Some("0" | "off" | "false"))
}

pub(super) fn apply(module: &Module<'_>) -> (usize, usize) {
    if !enabled() {
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
        // EXPERIMENT knobs (removed before shipping).
        let extra: Vec<String> = std::env::var("PERRY_COLD_X")
            .unwrap_or_default()
            .split(',')
            .map(str::to_string)
            .collect();
        let no_prof = std::env::var("PERRY_COLD_PROF").as_deref() == Ok("0");
        let is_cold_callee = |callee: LLVMValueRef| {
            if LLVMIsAFunction(callee).is_null() {
                return false;
            }
            if !LLVMGetEnumAttributeAtIndex(callee, LLVMAttributeFunctionIndex, cold_kind).is_null()
            {
                return true;
            }
            let mut len = 0usize;
            let name = LLVMGetValueName2(callee, &mut len);
            let name = std::slice::from_raw_parts(name.cast::<u8>(), len);
            extra.iter().any(|x| x.as_bytes() == name)
        };
        for function in module.get_functions() {
            let mut cold: HashSet<LLVMBasicBlockRef> = HashSet::new();
            let blocks = function.get_basic_blocks();
            for block in &blocks {
                for instruction in block.get_instructions() {
                    if instruction.get_opcode() != InstructionOpcode::Call {
                        continue;
                    }
                    let call = instruction.as_value_ref();
                    if !is_cold_callee(LLVMGetCalledValue(call)) {
                        continue;
                    }
                    LLVMAddCallSiteAttribute(
                        call,
                        LLVMAttributeFunctionIndex,
                        LLVMCreateEnumAttribute(ctx, cold_kind, 0),
                    );
                    sites += 1;
                    cold.insert(block.as_mut_ptr());
                }
            }
            if cold.is_empty() || no_prof {
                continue;
            }
            // A block whose every successor is cold cannot avoid a cold call.
            loop {
                let mut grew = false;
                for block in &blocks {
                    let block = block.as_mut_ptr();
                    if cold.contains(&block) {
                        continue;
                    }
                    let terminator = LLVMGetBasicBlockTerminator(block);
                    if terminator.is_null()
                        || LLVMGetInstructionOpcode(terminator) != llvm_sys::LLVMOpcode::LLVMBr
                    {
                        continue;
                    }
                    let n = LLVMGetNumSuccessors(terminator);
                    if n > 0 && (0..n).all(|i| cold.contains(&LLVMGetSuccessor(terminator, i))) {
                        cold.insert(block);
                        grew = true;
                    }
                }
                if !grew {
                    break;
                }
            }
            for block in &blocks {
                let block = block.as_mut_ptr();
                if cold.contains(&block) {
                    continue;
                }
                let terminator = LLVMGetBasicBlockTerminator(block);
                if terminator.is_null()
                    || LLVMGetInstructionOpcode(terminator) != llvm_sys::LLVMOpcode::LLVMBr
                    || LLVMIsConditional(terminator) == 0
                    || LLVMHasMetadata(terminator) != 0
                        && !LLVMGetMetadata(terminator, prof_kind).is_null()
                {
                    continue;
                }
                let taken = cold.contains(&LLVMGetSuccessor(terminator, 0));
                let not_taken = cold.contains(&LLVMGetSuccessor(terminator, 1));
                if taken == not_taken {
                    continue;
                }
                let i32_ty = LLVMInt32TypeInContext(ctx);
                let weight = |is_cold: bool| {
                    LLVMValueAsMetadata(LLVMConstInt(
                        i32_ty,
                        if is_cold { COLD_WEIGHT } else { HOT_WEIGHT },
                        0,
                    ))
                };
                let mut operands = [
                    LLVMMDStringInContext2(ctx, c"branch_weights".as_ptr(), 14),
                    weight(taken),
                    weight(not_taken),
                ];
                let node = LLVMMDNodeInContext2(ctx, operands.as_mut_ptr(), operands.len());
                LLVMSetMetadata(terminator, prof_kind, LLVMMetadataAsValue(ctx, node));
                branches += 1;
            }
        }
    }
    (sites, branches)
}
