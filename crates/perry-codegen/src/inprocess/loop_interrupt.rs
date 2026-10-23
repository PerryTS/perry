//! Loop interrupt polls (#12633): every loop back-edge of the final optimized
//! module polls the runtime's interrupt word, so `worker.terminate()` stops a
//! worker that never leaves a loop — including one that makes no calls.
//!
//! The poll is LLVM's own `place-safepoints` pass with the inline poll body
//! below (`gc.safepoint_poll`): a relaxed load of `@PERRY_LOOP_INTERRUPT_ARMED`
//! and a branch to a cold `call @js_loop_interrupt()` (see
//! `perry-runtime/src/loop_interrupt.rs` for the runtime half).
//!
//! It runs AFTER the optimization pipeline, on purpose. A poll present while
//! the loop optimizers run is a volatile load and a call in the loop body:
//! the vectorizer refuses the loop and the load serializes everything around
//! it (the packed-f64 clone measured this for the GC poll: 45 -> 38 ms once
//! the poll left the loop). Placed after them it sees the FINAL loops, so a
//! vectorized or unrolled loop pays one load and one not-taken branch per
//! machine iteration and no loop optimization ever sees it.
//!
//! The call needs no statepoint: `js_loop_interrupt` never collects. It
//! either returns or unwinds straight to the worker's base trap, and nothing
//! below the poll runs again in either case with a stale pointer. That is
//! also why placing it after `rewrite-statepoints-for-gc` is sound.
//!
//! Options (`-spp-*`, set once in `global_init`): no entry polls (only loops
//! can run unbounded without a call, and a call that runs forever polls in its
//! own loops), every back-edge (no counted-loop or "contains a call"
//! exemption: a runtime callee never polls, so a loop whose only calls are
//! into the runtime still needs its own), no call safepoints. Complete
//! executables proven to have no Worker agents omit this pass entirely.

use anyhow::{anyhow, Result};
use inkwell::memory_buffer::MemoryBuffer;
use inkwell::module::Module;
use inkwell::passes::PassBuilderOptions;
use inkwell::targets::TargetMachine;
use inkwell::values::AsValueRef;

/// The `-mllvm` options the pass runs under; applied by `global_init`.
pub(crate) const PLACE_SAFEPOINTS_OPTIONS: [&str; 3] =
    ["-spp-no-entry", "-spp-all-backedges", "-spp-no-call"];

/// The poll body `place-safepoints` inlines at each back-edge. Defined with
/// external linkage only so the module link keeps it (an unreferenced local
/// definition is dropped by the linker); [`place`] deletes it once every use
/// is inlined, so no unit ever exports it.
// Inkwell 0.9 on Windows requires the input terminator to be present.
// A C string keeps that precondition in the type, including after IR edits.
const POLL_IR: &std::ffi::CStr = cr#"
@PERRY_LOOP_INTERRUPT_ARMED = external global i32
declare void @js_loop_interrupt() #0
define void @gc.safepoint_poll() {
entry:
  %armed = load volatile i32, ptr @PERRY_LOOP_INTERRUPT_ARMED, align 4
  %due = icmp ne i32 %armed, 0
  br i1 %due, label %interrupt, label %done, !prof !0
interrupt:
  call void @js_loop_interrupt() #1
  br label %done
done:
  ret void
}
attributes #0 = { cold }
attributes #1 = { cold "gc-leaf-function" }
!0 = !{!"branch_weights", i32 1, i32 1048575}
"#;

const POLL_FN: &str = "gc.safepoint_poll";
const STRATEGY: &std::ffi::CStr = c"statepoint-example";

/// Whether `effective_target` gets loop interrupt polls. WebAssembly has no
/// worker threads to terminate.
pub(crate) fn polls_enabled_for(effective_target: &str) -> bool {
    !effective_target.starts_with("wasm32")
}

/// Insert the back-edge polls into every function `module` defines.
pub(super) fn place(module: &Module<'_>, tm: &TargetMachine) -> Result<()> {
    // The compiler's whole-program Worker analysis also selects per-agent
    // globals. Its explicit no-worker proof licenses omitting these polls:
    // no thread executing this image can acquire a termination handle.
    // Absence of the proof stays conservative, including reusable libraries.
    if module.get_global_metadata_size("perry.no_workers") != 0 {
        return Ok(());
    }
    let ctx = module.get_context();
    let buffer =
        MemoryBuffer::create_from_memory_range_copy(POLL_IR.to_bytes_with_nul(), "perry_loop_poll");
    let poll = ctx
        .create_module_from_ir(buffer)
        .map_err(|e| anyhow!("loop interrupt poll IR: {e}"))?;
    poll.set_triple(&module.get_triple());
    poll.set_data_layout(&module.get_data_layout());
    module
        .link_in_module(poll)
        .map_err(|e| anyhow!("linking the loop interrupt poll: {e}"))?;
    // `place-safepoints` dereferences the poll without checking for it.
    if module.get_function(POLL_FN).is_none() {
        return Err(anyhow!(
            "the loop interrupt poll did not survive the module link"
        ));
    }

    // `place-safepoints` visits only functions with a statepoint GC strategy.
    // Every defined function gets polls, so lend the strategy to those
    // without one for the duration of the pass and take it back after.
    let mut lent = Vec::new();
    for function in module.get_functions() {
        if function.count_basic_blocks() == 0 {
            continue;
        }
        if function.get_name().to_bytes() == POLL_FN.as_bytes() {
            continue;
        }
        let has_gc = unsafe { !llvm_sys::core::LLVMGetGC(function.as_value_ref()).is_null() };
        if !has_gc {
            unsafe { llvm_sys::core::LLVMSetGC(function.as_value_ref(), STRATEGY.as_ptr()) };
            lent.push(function);
        }
    }
    let placed = module.run_passes(
        "function(place-safepoints)",
        tm,
        PassBuilderOptions::create(),
    );
    for function in lent {
        unsafe { llvm_sys::core::LLVMSetGC(function.as_value_ref(), std::ptr::null()) };
    }
    placed.map_err(|e| anyhow!("place-safepoints failed: {e}"))?;
    if let Some(function) = module.get_function(POLL_FN) {
        // Every use was inlined; the definition itself is dead.
        unsafe { function.delete() };
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::{global_init, parse_ir_text};
    use super::*;
    use inkwell::context::Context;
    use inkwell::targets::{CodeModel, RelocMode, Target, TargetTriple};
    use inkwell::OptimizationLevel;

    /// A call-free loop in a function with no GC strategy, after O3: the
    /// shape #12633 could not stop.
    const SPIN: &str = r#"
define i32 @spin(i32 %n) {
entry:
  br label %loop
loop:
  %i = phi i32 [ 0, %entry ], [ %next, %loop ]
  %x = phi i32 [ 0, %entry ], [ %y, %loop ]
  %m = mul i32 %x, 31
  %y = add i32 %m, %i
  %next = add i32 %i, 1
  %more = icmp ne i32 %next, %n
  br i1 %more, label %loop, label %exit
exit:
  ret i32 %y
}

define i32 @straight(i32 %a) {
entry:
  %b = add i32 %a, 1
  ret i32 %b
}
"#;

    fn polled(ir: &str) -> String {
        let context = Context::create();
        let module = parse_ir_text(&context, ir, "loop_interrupt_test").unwrap();
        global_init(&[]);
        let triple = TargetTriple::create("x86_64-unknown-linux-gnu");
        let tm = Target::from_triple(&triple)
            .unwrap()
            .create_target_machine(
                &triple,
                "",
                "",
                OptimizationLevel::Default,
                RelocMode::PIC,
                CodeModel::Default,
            )
            .unwrap();
        module
            .run_passes("default<O3>", &tm, PassBuilderOptions::create())
            .unwrap();
        place(&module, &tm).unwrap();
        module.verify().expect("polled module verifies");
        module.print_to_string().to_string()
    }

    fn body<'a>(ir: &'a str, name: &str) -> &'a str {
        let start = ir.find(&format!("@{name}(")).unwrap();
        let end = start + ir[start..].find("\n}\n").unwrap();
        &ir[start..end]
    }

    #[test]
    fn every_back_edge_polls_and_nothing_else_does() {
        let ir = polled(SPIN);
        let spin = body(&ir, "spin");
        assert_eq!(
            spin.matches("load volatile i32, ptr @PERRY_LOOP_INTERRUPT_ARMED")
                .count(),
            1,
            "the loop's one back-edge polls:\n{ir}"
        );
        assert!(spin.contains("call void @js_loop_interrupt()"), "{ir}");
        assert!(
            !body(&ir, "straight").contains("PERRY_LOOP_INTERRUPT_ARMED"),
            "no entry polls:\n{ir}"
        );
    }

    #[test]
    fn the_pass_leaves_no_trace_but_the_polls() {
        let ir = polled(SPIN);
        assert!(!ir.contains("define void @gc.safepoint_poll"), "{ir}");
        assert!(
            !ir.contains("gc \"statepoint-example\""),
            "a lent strategy is returned:\n{ir}"
        );
    }

    #[test]
    fn whole_program_no_worker_proof_omits_interrupt_polls() {
        let ir = polled(&format!(
            "{SPIN}\n!perry.no_workers = !{{!0}}\n!0 = !{{}}\n"
        ));
        assert!(!ir.contains("PERRY_LOOP_INTERRUPT_ARMED"), "{ir}");
        assert!(!ir.contains("js_loop_interrupt"), "{ir}");
        assert!(!ir.contains("gc.safepoint_poll"), "{ir}");
    }

    #[test]
    fn only_complete_executables_without_workers_publish_the_proof() {
        for (output_type, workers, expected) in [
            ("executable", false, true),
            ("executable", true, false),
            ("library", false, false),
        ] {
            let module = perry_hir::Module::new("worker_poll_proof");
            let ir = crate::compile_module(
                &module,
                crate::CompileOptions {
                    output_type: output_type.to_string(),
                    program_has_worker: workers,
                    emit_ir_only: true,
                    is_entry_module: true,
                    ..Default::default()
                },
            )
            .unwrap();
            let ir = String::from_utf8(ir).unwrap();
            assert_eq!(
                ir.contains("!perry.no_workers ="),
                expected,
                "output={output_type}, workers={workers}: {ir}"
            );
        }
    }
}
