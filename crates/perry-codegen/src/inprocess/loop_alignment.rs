//! Give AArch64 functions containing final worker-interrupt or GC back-edge
//! polls a modest entry alignment. Reuse those existing polls, including
//! statepoint-wrapped calls, instead of constructing another loop analysis.
//! Functions without a recognized poll retain the target's default alignment.

use inkwell::module::Module;
use inkwell::targets::Target;
use inkwell::values::{AsValueRef, InstructionOpcode};

// A small fixed alignment reduces sensitivity of hot loop bodies to unrelated
// code-layout changes without imposing larger padding on every function.
const LOOP_FUNCTION_ALIGNMENT: u32 = 64;

pub(super) fn apply(module: &Module<'_>, target: &Target) {
    align_polled_functions(module, target.get_name().to_bytes().starts_with(b"aarch64"));
}

fn align_polled_functions(module: &Module<'_>, aarch64: bool) -> usize {
    if !aarch64 {
        return 0;
    }
    let interrupt = module.get_function("js_loop_interrupt");
    let gc_poll = module.get_function("js_gc_loop_safepoint");
    if interrupt.is_none() && gc_poll.is_none() {
        return 0;
    }
    let is_poll = |callee| {
        interrupt.is_some_and(|f| f.as_value_ref() == callee)
            || gc_poll.is_some_and(|f| f.as_value_ref() == callee)
    };
    let statepoint = c"llvm.experimental.gc.statepoint";
    let statepoint_id = unsafe {
        llvm_sys::core::LLVMLookupIntrinsicID(statepoint.as_ptr(), statepoint.to_bytes().len())
    };
    let mut changed = 0;
    for function in module.get_functions() {
        let has_poll = function.get_basic_blocks().iter().any(|block| {
            block.get_instructions().any(|instruction| {
                matches!(
                    instruction.get_opcode(),
                    InstructionOpcode::Call | InstructionOpcode::Invoke
                ) && unsafe {
                    let instruction = instruction.as_value_ref();
                    let callee = llvm_sys::core::LLVMGetCalledValue(instruction);
                    is_poll(callee)
                        || (statepoint_id != 0
                            && !llvm_sys::core::LLVMIsAFunction(callee).is_null()
                            && llvm_sys::core::LLVMGetIntrinsicID(callee) == statepoint_id
                            && llvm_sys::core::LLVMGetNumArgOperands(instruction) > 2
                            // LLVM 22 Statepoint.h: CalledFunctionPos = 2.
                            // Call/invoke arguments precede their other operands.
                            && is_poll(llvm_sys::core::LLVMGetOperand(instruction, 2)))
                }
            })
        });
        if has_poll && function.as_global_value().get_alignment() < LOOP_FUNCTION_ALIGNMENT {
            function
                .as_global_value()
                .set_alignment(LOOP_FUNCTION_ALIGNMENT);
            changed += 1;
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inprocess::parse_ir_text;
    use inkwell::context::Context;

    const IR: &str = r#"
declare void @js_loop_interrupt()
declare void @js_gc_loop_safepoint()
declare void @consume(i64, i64, ptr)
declare void @unrelated()
declare i32 @personality(...)
declare token @llvm.experimental.gc.statepoint.p0(i64 immarg, i32 immarg, ptr, i32 immarg, i32 immarg, ...)
define void @looping(i1 %again) {
entry:
  br label %loop
loop:
  call void @js_loop_interrupt()
  br i1 %again, label %loop, label %exit
exit:
  ret void
}
define void @straight() { ret void }
define void @gc_direct(i1 %again) {
entry:
  br label %loop
loop:
  call void @js_gc_loop_safepoint()
  br i1 %again, label %loop, label %exit
exit:
  ret void
}
define void @gc_wrapped(i1 %again) gc "statepoint-example" {
entry:
  br label %loop
loop:
  %s = call token (i64, i32, ptr, i32, i32, ...) @llvm.experimental.gc.statepoint.p0(i64 0, i32 0, ptr elementtype(void ()) @js_gc_loop_safepoint, i32 0, i32 0, i32 0, i32 0)
  br i1 %again, label %loop, label %exit
exit:
  ret void
}
define void @worker_wrapped(i1 %again) gc "statepoint-example" {
entry:
  br label %loop
loop:
  %s = call token (i64, i32, ptr, i32, i32, ...) @llvm.experimental.gc.statepoint.p0(i64 0, i32 0, ptr elementtype(void ()) @js_loop_interrupt, i32 0, i32 0, i32 0, i32 0)
  br i1 %again, label %loop, label %exit
exit:
  ret void
}
define void @worker_invoke(i1 %again) personality ptr @personality {
entry:
  br label %loop
loop:
  invoke void @js_loop_interrupt() to label %polled unwind label %fail
polled:
  br i1 %again, label %loop, label %exit
exit:
  ret void
fail:
  %pad = landingpad { ptr, i32 } cleanup
  resume { ptr, i32 } %pad
}
define void @gc_wrapped_invoke(i1 %again) gc "statepoint-example" personality ptr @personality {
entry:
  br label %loop
loop:
  %s = invoke token (i64, i32, ptr, i32, i32, ...) @llvm.experimental.gc.statepoint.p0(i64 0, i32 0, ptr elementtype(void ()) @js_gc_loop_safepoint, i32 0, i32 0, i32 0, i32 0) to label %polled unwind label %fail
polled:
  br i1 %again, label %loop, label %exit
exit:
  ret void
fail:
  %pad = landingpad token cleanup
  ret void
}
define void @passes_poll_as_argument() {
  call void @consume(i64 0, i64 0, ptr @js_gc_loop_safepoint)
  ret void
}
define void @unrecognized_loop(i1 %again) {
entry:
  br label %loop
loop:
  br i1 %again, label %loop, label %exit
exit:
  ret void
}
define void @unknown_callee(ptr %callee) {
  call void %callee()
  ret void
}
define void @unrelated_wrapped() gc "statepoint-example" {
  %s = call token (i64, i32, ptr, i32, i32, ...) @llvm.experimental.gc.statepoint.p0(i64 0, i32 0, ptr elementtype(void ()) @unrelated, i32 0, i32 0, i32 0, i32 0)
  ret void
}
define void @stronger() align 256 {
entry:
  call void @js_loop_interrupt()
  ret void
}
@looping$pshape = alias void (i1), ptr @looping
"#;

    #[test]
    fn only_polled_bodies_gain_alignment_without_changing_instructions_or_aliases() {
        let context = Context::create();
        let module = parse_ir_text(&context, IR, "alignment_test").unwrap();
        module.verify().unwrap();
        let before = module.print_to_string().to_string();
        assert_eq!(align_polled_functions(&module, true), 6);
        for name in [
            "looping",
            "gc_direct",
            "gc_wrapped",
            "worker_wrapped",
            "worker_invoke",
            "gc_wrapped_invoke",
        ] {
            assert_eq!(
                module
                    .get_function(name)
                    .unwrap()
                    .as_global_value()
                    .get_alignment(),
                LOOP_FUNCTION_ALIGNMENT
            );
        }
        for name in [
            "straight",
            "passes_poll_as_argument",
            "unrecognized_loop",
            "unknown_callee",
            "unrelated_wrapped",
        ] {
            assert_eq!(
                module
                    .get_function(name)
                    .unwrap()
                    .as_global_value()
                    .get_alignment(),
                0,
                "{name}"
            );
        }
        assert_eq!(
            module
                .get_function("stronger")
                .unwrap()
                .as_global_value()
                .get_alignment(),
            256
        );
        module.verify().unwrap();
        let after = module.print_to_string().to_string();
        assert_eq!(before, after.replace(" align 64", ""));
    }

    #[test]
    fn other_architectures_and_modules_without_polls_are_unchanged() {
        let context = Context::create();
        for (ir, aarch64) in [(IR, false), ("define void @plain() { ret void }", true)] {
            let module = parse_ir_text(&context, ir, "alignment_unchanged").unwrap();
            let before = module.print_to_string().to_string();
            assert_eq!(align_polled_functions(&module, aarch64), 0);
            assert_eq!(module.print_to_string().to_string(), before);
        }
    }

    #[test]
    fn existing_equal_or_stronger_alignment_is_preserved_and_policy_is_idempotent() {
        let context = Context::create();
        let module = parse_ir_text(&context, IR, "alignment_existing").unwrap();
        module
            .get_function("gc_direct")
            .unwrap()
            .as_global_value()
            .set_alignment(64);
        module
            .get_function("gc_wrapped")
            .unwrap()
            .as_global_value()
            .set_alignment(128);
        assert_eq!(align_polled_functions(&module, true), 4);
        assert_eq!(
            module
                .get_function("gc_wrapped")
                .unwrap()
                .as_global_value()
                .get_alignment(),
            128
        );
        let once = module.print_to_string().to_string();
        assert_eq!(align_polled_functions(&module, true), 0);
        assert_eq!(module.print_to_string().to_string(), once);
        module.verify().unwrap();
    }

    #[test]
    fn llvm_target_identity_handles_aarch64_and_arm64_triple_spellings() {
        use inkwell::targets::TargetTriple;
        crate::inprocess::global_init(&[]);
        for triple in [
            "aarch64-apple-darwin",
            "arm64-apple-darwin",
            "aarch64-unknown-linux-gnu",
        ] {
            let target = Target::from_triple(&TargetTriple::create(triple)).unwrap();
            assert!(
                target.get_name().to_bytes().starts_with(b"aarch64"),
                "{triple}"
            );
        }
        let target =
            Target::from_triple(&TargetTriple::create("x86_64-unknown-linux-gnu")).unwrap();
        assert!(!target.get_name().to_bytes().starts_with(b"aarch64"));
    }
}
