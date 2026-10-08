//! LLVM optimize-and-emit for the in-process backend.
//!
//! Split out of `inprocess.rs` to keep that file under the 2000-line size gate.

use super::*;

pub(super) fn optimize_and_emit(
    module: &inkwell::module::Module<'_>,
    effective_target: &str,
    opt: char,
    mcpu_native: bool,
    explicit_cpu: Option<&str>,
    mllvm: &[String],
    emit_asm: bool,
    native_roots: bool,
    mut stats: Option<&mut UnitCodegenStats>,
) -> Result<Vec<Vec<u8>>> {
    global_init(mllvm);
    announce();

    module
        .verify()
        .map_err(|e| anyhow!("LLVM verifier rejected module:\n{}", e.to_string()))?;

    let triple = TargetTriple::create(effective_target);
    let target = Target::from_triple(&triple)
        .map_err(|e| anyhow!("no LLVM target for `{effective_target}`: {e}"))?;
    let (cpu, features) = if effective_target.starts_with("wasm32") {
        ("generic".to_string(), "+exception-handling".to_string())
    } else if mcpu_native {
        (
            TargetMachine::get_host_cpu_name()
                .to_string_lossy()
                .into_owned(),
            TargetMachine::get_host_cpu_features()
                .to_string_lossy()
                .into_owned(),
        )
    } else if let Some(cpu) = explicit_cpu {
        (cpu.to_string(), String::new())
    } else {
        (
            default_cpu_for_triple(effective_target).to_string(),
            String::new(),
        )
    };
    let opt_level = match opt {
        '0' => OptimizationLevel::None,
        '1' => OptimizationLevel::Less,
        '2' | 's' | 'z' => OptimizationLevel::Default,
        _ => OptimizationLevel::Aggressive,
    };
    let tm = target
        .create_target_machine(
            &triple,
            &cpu,
            &features,
            opt_level,
            RelocMode::PIC,
            CodeModel::Default,
        )
        .ok_or_else(|| anyhow!("failed to create TargetMachine for `{effective_target}`"))?;
    #[cfg(feature = "target-wasi")]
    if effective_target.starts_with("wasm32") {
        extern "C" {
            fn perry_llvm_enable_wasm_eh(machine: *mut core::ffi::c_void);
        }
        unsafe {
            perry_llvm_enable_wasm_eh(tm.as_mut_ptr().cast());
        }
    }

    // Same trust order as the subprocess path: `-target` wins over whatever
    // triple the module text states, and the module optimizes under the
    // machine's real datalayout.
    module.set_triple(&triple);
    module.set_data_layout(&tm.get_target_data().get_data_layout());

    // RS4GC must run BEFORE the optimization pipeline, and — critically — in
    // this process, against this LLVM.
    //
    // The external path shells `rewrite-statepoints-for-gc` out to an `opt`
    // binary and then hands the rewritten IR to `clang -c`. When those are
    // different LLVM versions (Homebrew 22 and Apple clang 21 is the ordinary
    // macOS case) the emitted IR uses constructs the older parser rejects, and
    // the compile dies with `error: unterminated attribute group`. That is why
    // RS4GC needed `PERRY_LLVM_CLANG` pointed at a version-matched toolchain,
    // and why it did not work on a stock install at all.
    //
    // Here the same `TargetMachine` runs the pass and emits the object, so the
    // skew cannot exist. This matters beyond convenience: RS4GC is the only
    // backend that can root an `invoke`, and since #7302 every call inside a
    // `try` is one — 26% of the gap suite (128 of 479 files) contains a `try`,
    // which the explicit bridge refuses outright (#7327/#7330).
    if native_roots {
        // Remove unused pure reload chains before deciding which memory homes
        // survive. DCE does not promote an alloca or forward a live root load.
        module
            .run_passes("function(dce)", &tm, PassBuilderOptions::create())
            .map_err(|e| anyhow!("native-home dead-code cleanup failed: {e}"))?;
        native_homes::retain(module);
        // Sizes before the rewrite: the budget message below names them, and
        // the per-unit report compares them with the post-rewrite census.
        let budget = rs4gc_instruction_budget();
        let rewritten_functions = rs4gc_functions(module);
        let pre_sizes = if budget == RewriteBudget::Off && stats.is_none() {
            std::collections::HashMap::new()
        } else {
            pre_rewrite_sizes(module)
        };
        if let Some(stats) = stats.as_deref_mut() {
            stats.functions = pre_sizes.len();
            stats.pre_rewrite_instructions = pre_sizes.values().sum();
            stats.pre_rewrite_widest = pre_sizes
                .iter()
                .max_by_key(|(_, n)| **n)
                .map(|(name, n)| (name.clone(), *n));
        }
        // Rewrite the retained-memory and ordinary SSA roots together. The
        // post-rewrite census bounds unrelated optimizer expansion without
        // changing the rooting mechanism.
        let rewrite_started = std::time::Instant::now();
        module
            .run_passes(STATEPOINT_REWRITE_PASSES, &tm, PassBuilderOptions::create())
            .map_err(|e| {
                anyhow!(
                    "in-process rewrite-statepoints-for-gc failed:\n{}",
                    e.to_string()
                )
            })?;
        // Verify the rewritten module before it reaches the backend. RS4GC
        // has produced verifier-invalid IR in the wild (#8121: it wrapped an
        // inline-asm barrier into a gc.statepoint), and unlike the external
        // `opt` path — whose verifier aborts with the broken instruction —
        // the in-process pipeline would feed the broken module straight to
        // ISel, where it dies as a bare SIGBUS with no diagnostic.
        module.verify().map_err(|e| {
            anyhow!(
                "in-process rewrite-statepoints-for-gc produced a module the \
                 verifier rejects (this is a Perry codegen bug — the input \
                 shape must be exempted or fixed):\n{}",
                e.to_string()
            )
        })?;
        if let Some(stats) = stats.as_deref_mut() {
            stats.rewrite_secs = rewrite_started.elapsed().as_secs_f64();
            let (_, total, widest) = module_instruction_census(module);
            stats.post_rewrite_instructions = total;
            stats.post_rewrite_widest = widest;
        }
        // The relocation-fan-out backstop (#8583/#8679): stop before the
        // super-linear optimizer and ask codegen to retry the named functions
        // with precise shadow-frame roots. The retry keeps this same pipeline
        // and optimization level; only the GC-root representation changes.
        enforce_rs4gc_instruction_budget(module, budget, &pre_sizes, &rewritten_functions)?;
    }

    let pipeline = match opt {
        '0' => "default<O0>",
        '1' => "default<O1>",
        '2' => "default<O2>",
        's' => "default<Os>",
        'z' => "default<Oz>",
        _ => "default<O3>",
    };
    let optimize_started = std::time::Instant::now();
    module
        .run_passes(pipeline, &tm, PassBuilderOptions::create())
        .map_err(|e| anyhow!("pass pipeline `{pipeline}` failed:\n{}", e.to_string()))?;
    if let Some(stats) = stats.as_deref_mut() {
        stats.optimize_secs = optimize_started.elapsed().as_secs_f64();
    }
    if native_roots {
        native_homes::publish(module)?;
        module
            .verify()
            .map_err(|e| anyhow!("native root homes: {e}"))?;
    }
    // Include small units as well as budget offenders, so before/after size
    // audits do not lose their subject when a fix brings it below the cap.
    if std::env::var_os("PERRY_CODEGEN_UNIT_TIMINGS").is_some() {
        let (functions, total, widest) = module_instruction_census(module);
        if let Some((name, instructions)) = widest {
            eprintln!(
                "[perry] codegen: `{name}` has {instructions} instructions after IR optimization \
                 (unit total {total}, {functions} functions, opt {:.3}s)",
                optimize_started.elapsed().as_secs_f64(),
            );
        }
    }

    // Last change to the module before emission, so it sees exactly the
    // functions that become symbols (see `crate::function_order`).
    super::function_layout::apply(
        module,
        &crate::function_order::program_function_layout(),
        effective_target,
    );

    let kind = if emit_asm {
        FileType::Assembly
    } else {
        FileType::Object
    };
    let emit_started = std::time::Instant::now();
    let emit = |tm: &TargetMachine, m: &inkwell::module::Module<'_>| -> Result<Vec<u8>> {
        tm.write_to_memory_buffer(m, kind)
            .map(|obj| obj.as_slice().to_vec())
            .map_err(|e| anyhow!("{kind:?} emission failed:\n{}", e.to_string()))
    };
    let pieces = vec![emit(&tm, module)?];
    if let Some(stats) = stats {
        stats.emit_secs = emit_started.elapsed().as_secs_f64();
    }
    Ok(pieces)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn relocation_results(ir: &str) -> std::collections::HashSet<&str> {
        ir.lines()
            .filter(|line| line.contains("@llvm.experimental.gc.relocate"))
            .filter_map(|line| line.trim().split_once(" = ").map(|(result, _)| result))
            .collect()
    }

    fn returned_gc_pointers(ir: &str) -> Vec<&str> {
        ir.lines()
            .filter_map(|line| {
                line.trim()
                    .strip_prefix("ret ptr addrspace(1) ")
                    .and_then(|value| value.split_whitespace().next())
            })
            .collect()
    }

    fn asm_barrier_fixture(leaf_attr: &str) -> String {
        format!(
            "declare i64 @may_collect()\n\n\
             define i64 @f(i64 %a) gc \"statepoint-example\" {{\n\
             entry:\n\
             \x20 %slot = alloca ptr addrspace(1)\n\
             \x20 %p = inttoptr i64 %a to ptr addrspace(1)\n\
             \x20 store ptr addrspace(1) %p, ptr %slot\n\
             \x20 call void asm sideeffect \"\", \"\"(){leaf_attr}\n\
             \x20 %t = call i64 @may_collect()\n\
             \x20 %after = load ptr addrspace(1), ptr %slot\n\
             \x20 %bits = ptrtoint ptr addrspace(1) %after to i64\n\
             \x20 %r = add i64 %t, %bits\n\
             \x20 ret i64 %r\n\
             }}\n"
        )
    }

    #[test]
    fn gc_leaf_asm_barrier_survives_rs4gc_unwrapped() {
        // The shipped emitters stamp the loop-preservation barrier
        // `"gc-leaf-function"`; RS4GC must leave it as a plain inline-asm
        // call while still statepointing the real call next to it.
        let rewritten = statepoint_rewritten_ir(
            &asm_barrier_fixture(" \"gc-leaf-function\""),
            "arm64-apple-darwin",
            "asm_barrier_leaf",
        )
        .expect("attributed barrier must survive the rewrite");
        assert!(
            rewritten.contains("call void asm sideeffect"),
            "barrier must remain a plain inline-asm call:\n{rewritten}"
        );
        assert!(
            !rewritten.contains("elementtype(void ()) asm"),
            "barrier must not be statepoint-wrapped:\n{rewritten}"
        );
        assert!(
            rewritten.contains("@llvm.experimental.gc.statepoint"),
            "the genuine call must still be statepointed:\n{rewritten}"
        );
    }

    #[test]
    fn unattributed_asm_barrier_is_rejected_not_miscompiled() {
        // Sabotage arm: without the attribute RS4GC wraps the asm into a
        // gc.statepoint whose callee is inline asm — invalid IR. The
        // pipeline must fail verification loudly (#8121's SIGBUS shape),
        // proving the leaf test above can actually fail.
        let result = statepoint_rewritten_ir(
            &asm_barrier_fixture(""),
            "arm64-apple-darwin",
            "asm_barrier_broken",
        );
        assert!(
            result.is_err(),
            "an unattributed barrier must be rejected by the verifier"
        );
    }

    #[test]
    fn rewrite_budget_spellings() {
        assert_eq!(
            parse_rewrite_budget(None),
            RewriteBudget::Error(DEFAULT_RS4GC_MAX_INSTRS)
        );
        assert_eq!(parse_rewrite_budget(Some("0")), RewriteBudget::Off);
        assert_eq!(parse_rewrite_budget(Some("off")), RewriteBudget::Off);
        assert_eq!(
            parse_rewrite_budget(Some(" 250000 ")),
            RewriteBudget::Error(250_000)
        );
        assert_eq!(
            parse_rewrite_budget(Some("warn:4096")),
            RewriteBudget::Warn(4096)
        );
        assert_eq!(parse_rewrite_budget(Some("warn:0")), RewriteBudget::Off);
        // Unparsable values keep the default rather than silently disabling.
        assert_eq!(
            parse_rewrite_budget(Some("lots")),
            RewriteBudget::Error(DEFAULT_RS4GC_MAX_INSTRS)
        );
    }

    fn constant_fold_order_fixture(folded: bool) -> String {
        let mut ir = String::from(
            "declare i64 @may_collect()\n\ndefine i64 @f(i64 %d0, i64 %d1, i64 %d2, i64 %d3, i64 %d4, i64 %d5, i64 %d6, i64 %d7) gc \"statepoint-example\" {\nentry:\n",
        );
        for i in 0..8 {
            ir.push_str(&format!("  %cslot{i} = alloca ptr addrspace(1)\n"));
            if folded {
                ir.push_str(&format!(
                    "  store ptr addrspace(1) inttoptr (i64 9222246136947933185 to ptr addrspace(1)), ptr %cslot{i}\n"
                ));
            } else {
                ir.push_str(&format!(
                    "  %cb{i} = bitcast double 0x7FFC000000000001 to i64\n  %cp{i} = inttoptr i64 %cb{i} to ptr addrspace(1)\n  store ptr addrspace(1) %cp{i}, ptr %cslot{i}\n"
                ));
            }
        }
        for i in 0..8 {
            ir.push_str(&format!(
                "  %dslot{i} = alloca ptr addrspace(1)\n  %dp{i} = inttoptr i64 %d{i} to ptr addrspace(1)\n  store ptr addrspace(1) %dp{i}, ptr %dslot{i}\n"
            ));
        }
        ir.push_str("  %sp = call i64 @may_collect()\n");
        for i in 0..8 {
            ir.push_str(&format!(
                "  %after{i} = load ptr addrspace(1), ptr %dslot{i}\n  %bits{i} = ptrtoint ptr addrspace(1) %after{i} to i64\n"
            ));
        }
        for i in 0..8 {
            ir.push_str(&format!(
                "  %cafter{i} = load ptr addrspace(1), ptr %cslot{i}\n  %cbits{i} = ptrtoint ptr addrspace(1) %cafter{i} to i64\n"
            ));
        }
        ir.push_str("  %x1 = xor i64 %bits0, %bits1\n");
        for i in 2..8 {
            ir.push_str(&format!("  %x{i} = xor i64 %x{}, %bits{i}\n", i - 1));
        }
        ir.push_str("  %y0 = xor i64 %x7, %cbits0\n");
        for i in 1..8 {
            ir.push_str(&format!("  %y{i} = xor i64 %y{}, %cbits{i}\n", i - 1));
        }
        ir.push_str("  ret i64 %y7\n}\n");
        ir
    }

    #[test]
    fn rs4gc_canonicalizes_construction_time_folds_before_root_liveness() {
        let _native = crate::codegen::helpers::NativeRootsPin::native();
        let target = crate::codegen::default_target_triple();
        let text_ir = constant_fold_order_fixture(false);
        let folded_ir = constant_fold_order_fixture(true);

        for (label, ir) in [("text", &text_ir), ("folded", &folded_ir)] {
            let rewritten = statepoint_rewritten_ir(ir, &target, label)
                .unwrap_or_else(|e| panic!("{label} fixture must run RS4GC: {e:#}"));
            assert!(
                !rewritten.contains("%cb0 = bitcast"),
                "{label} fixture reached RS4GC before construction-time folds converged:\n{rewritten}"
            );
            let live_bundle = rewritten
                .lines()
                .find(|line| line.contains("\"gc-live\""))
                .unwrap_or_else(|| panic!("{label} fixture lost every dynamic root:\n{rewritten}"));
            assert!(
                live_bundle.contains("%dp0"),
                "{label} fixture lost every dynamic root:\n{rewritten}"
            );
            assert!(
                rewritten.contains("gc.relocate"),
                "{label} fixture did not relocate a dynamic root:\n{rewritten}"
            );
        }

        let emit = |ir: &str, name: &str| {
            let context = Context::create();
            let module = parse_ir_text(&context, ir, name).expect("fixture parses");
            optimize_and_emit_module(&module, &target, &["-O3".into(), "-S".into()], true)
                .map(single_piece)
                .expect("fixture emits assembly")
        };
        // Both arms must be emitted under the SAME module name. The name
        // becomes the module id, and on ELF the assembler writes it into the
        // object as a `.file` directive — so two differently-named arms differ
        // by that one line no matter how perfectly the code itself converged.
        // Mach-O records no such directive, which is why naming them apart only
        // ever failed on Linux (#8087).
        let text = emit(&text_ir, "constant_fold_order");
        let folded = emit(&folded_ir, "constant_fold_order");
        assert_eq!(
            text, folded,
            "construction-time constant folding must converge before RS4GC assigns root liveness"
        );

        const PRE_FIX_PASSES: &str = "function(mem2reg),rewrite-statepoints-for-gc";
        let pre_fix_emit = |ir: &str, name: &str| {
            let rewritten = statepoint_rewritten_ir_with_passes(
                ir,
                &target,
                &format!("{name}_rewrite"),
                PRE_FIX_PASSES,
            )
            .expect("pre-fix pipeline rewrites fixture");
            let context = Context::create();
            let module =
                parse_ir_text(&context, &rewritten, name).expect("rewritten fixture parses");
            let _shadow = crate::codegen::helpers::NativeRootsPin::shadow();
            (
                rewritten,
                optimize_and_emit_module(&module, &target, &["-O3".into(), "-S".into()], false)
                    .map(single_piece)
                    .expect("rewritten fixture emits assembly"),
            )
        };
        let (pre_fix_text_ir, pre_fix_text) = pre_fix_emit(&text_ir, "pre_fix_text");
        let (_, pre_fix_folded) = pre_fix_emit(&folded_ir, "pre_fix_native");
        assert!(
            pre_fix_text_ir
                .lines()
                .find(|line| line.contains("\"gc-live\""))
                .is_some_and(|line| line.contains("%cp0")),
            "negative control must keep a constant-derived text root live across the safepoint:\n{pre_fix_text_ir}"
        );
        assert_ne!(
            pre_fix_text, pre_fix_folded,
            "fixture must fail byte equality under the pre-#8065 pass order"
        );
    }

    #[test]
    fn rs4gc_honors_alwaysinline_before_rewriting_calls() {
        let target = crate::codegen::default_target_triple();
        let ir = r#"
declare ptr addrspace(1) @alloc()

define internal ptr addrspace(1) @leaf(ptr addrspace(1) %p) alwaysinline gc "statepoint-example" {
entry:
  %unused = call ptr addrspace(1) @alloc()
  ret ptr addrspace(1) %p
}

define ptr addrspace(1) @caller(ptr addrspace(1) %p) gc "statepoint-example" {
entry:
  %result = call ptr addrspace(1) @leaf(ptr addrspace(1) %p)
  ret ptr addrspace(1) %result
}
"#;

        const PRE_FIX_PASSES: &str = "function(mem2reg,sccp),rewrite-statepoints-for-gc";
        let before =
            statepoint_rewritten_ir_with_passes(ir, &target, "alwaysinline_before", PRE_FIX_PASSES)
                .expect("negative control rewrites the fixture");
        assert!(
            before.lines().any(|line| {
                line.contains("@llvm.experimental.gc.statepoint") && line.contains("@leaf")
            }),
            "negative control must leave the alwaysinline call as a statepoint:\n{before}"
        );

        let after = statepoint_rewritten_ir(ir, &target, "alwaysinline_after")
            .expect("shipped pipeline rewrites the inlined fixture");
        assert!(
            !after.contains("@leaf"),
            "alwaysinline callee and call must disappear before RS4GC:\n{after}"
        );
        let live_bundle = after
            .lines()
            .find(|line| line.contains("@llvm.experimental.gc.statepoint"))
            .unwrap_or_else(|| panic!("inlined allocation must remain a statepoint:\n{after}"));
        assert!(
            live_bundle.contains("\"gc-live\"") && live_bundle.contains("%p"),
            "caller root must stay live through the inlined allocation:\n{after}"
        );
        let relocation_results = relocation_results(&after);
        let returned_pointers = returned_gc_pointers(&after);
        assert_eq!(
            returned_pointers.len(),
            1,
            "fixture must retain exactly one return edge after inlining:\n{after}"
        );
        assert!(
            relocation_results.contains(returned_pointers[0]),
            "caller must return the gc.relocate result, not the pre-statepoint root:\n{after}"
        );
    }

    #[test]
    fn rs4gc_rewrites_inlined_invoke_and_preserves_exception_edge() {
        let target = crate::codegen::default_target_triple();
        let ir = r#"
declare ptr addrspace(1) @alloc()
declare i32 @perry_eh_personality(...)

define internal ptr addrspace(1) @leaf(ptr addrspace(1) %p) alwaysinline gc "statepoint-example" personality ptr @perry_eh_personality {
entry:
  %unused = invoke ptr addrspace(1) @alloc()
      to label %ok unwind label %exception
ok:
  ret ptr addrspace(1) %p
exception:
  %landing = landingpad token cleanup
  ret ptr addrspace(1) %p
}

define ptr addrspace(1) @caller(ptr addrspace(1) %p) gc "statepoint-example" personality ptr @perry_eh_personality {
entry:
  %result = call ptr addrspace(1) @leaf(ptr addrspace(1) %p)
  ret ptr addrspace(1) %result
}
"#;

        let after = statepoint_rewritten_ir(ir, &target, "alwaysinline_invoke")
            .expect("shipped pipeline rewrites an invoke in an inlined callee");
        assert!(
            !after.contains("@leaf"),
            "alwaysinline invoke callee must disappear before RS4GC:\n{after}"
        );
        assert!(
            after.lines().any(|line| {
                line.contains("invoke token") && line.contains("@llvm.experimental.gc.statepoint")
            }),
            "inlined invoke must become a statepoint while retaining its unwind edge:\n{after}"
        );
        assert!(
            after.contains("landingpad token")
                && after.lines().any(|line| line.trim() == "cleanup"),
            "statepoint invoke must retain a verifier-valid exceptional pad:\n{after}"
        );
        let relocation_results = relocation_results(&after);
        let returned_pointers = returned_gc_pointers(&after);
        assert_eq!(
            returned_pointers.len(),
            1,
            "inlined invoke fixture must retain one merged return edge:\n{after}"
        );
        assert_eq!(
            relocation_results.len(),
            2,
            "normal and exceptional continuations must each relocate the root:\n{after}"
        );
        let return_phi = after
            .lines()
            .find(|line| {
                line.trim().starts_with(returned_pointers[0])
                    && line.contains(" = phi ptr addrspace(1) ")
            })
            .unwrap_or_else(|| {
                panic!("invoke continuations must merge through the returned phi:\n{after}")
            });
        assert!(
            relocation_results
                .iter()
                .all(|relocated| return_phi.contains(*relocated)),
            "returned phi must merge both gc.relocate results, not the pre-statepoint root:\n{after}"
        );
    }

    /// Layer-2 readiness (#7174, engine-plan layer 0 -> 2): the in-process
    /// pipeline can schedule `RewriteStatepointsForGC` at the pinned LLVM —
    /// no `opt` subprocess, no version-skewed toolchain. This is the exact
    /// mechanism #7108 measured as viable-but-blocked under text-plus-clang.
    /// A statepoint lands at the may-GC call and the live GC pointer is
    /// relocated across it — the property that makes the register-held-
    /// pointer bug class unrepresentable.
    #[test]
    fn rs4gc_schedules_in_process() {
        let context = Context::create();
        let ir = r#"
declare ptr addrspace(1) @alloc()

define ptr addrspace(1) @f(ptr addrspace(1) %p) gc "statepoint-example" {
entry:
  %q = call ptr addrspace(1) @alloc()
  ret ptr addrspace(1) %p
}
"#;
        let module = parse_ir_text(&context, ir, "rs4gc_probe").expect("probe parses");
        global_init(&[]);
        let triple = TargetMachine::get_default_triple();
        let target = Target::from_triple(&triple).expect("host target");
        let tm = target
            .create_target_machine(
                &triple,
                "",
                "",
                OptimizationLevel::None,
                RelocMode::PIC,
                CodeModel::Default,
            )
            .expect("target machine");
        module
            .run_passes(
                "rewrite-statepoints-for-gc",
                &tm,
                PassBuilderOptions::create(),
            )
            .expect("RS4GC pipeline runs in-process");
        let printed = module.print_to_string().to_string();
        assert!(
            printed.contains("gc.statepoint"),
            "no statepoint emitted:\n{printed}"
        );
        assert!(
            printed.contains("gc.relocate"),
            "live GC pointer not relocated across the call:\n{printed}"
        );
        module.verify().expect("statepoint IR verifies");
    }

    /// The initialized backend set must cover every triple the compile driver
    /// can produce. `initialize_all()` cost +86.9 MB of static link for ~18
    /// unreachable backends; this pins the replacement, so narrowing it
    /// further — or adding a target without initializing its backend — fails
    /// here rather than at a user's compile.
    #[test]
    fn every_supported_triple_resolves_to_an_initialized_backend() {
        global_init(&[]);
        for triple in [
            "arm64-apple-macosx",
            "aarch64-apple-ios",
            "aarch64-apple-watchos",
            "arm64_32-apple-watchos",
            "aarch64-unknown-linux-gnu",
            "aarch64-unknown-linux-musl",
            "aarch64-linux-android",
            "x86_64-apple-darwin",
            "x86_64-unknown-linux-gnu",
            "x86_64-pc-windows-msvc",
            "i686-unknown-linux-gnu",
        ] {
            let t = TargetTriple::create(triple);
            assert!(
                Target::from_triple(&t).is_ok(),
                "{triple} has no initialized LLVM backend — the compile driver \
                 can emit this triple, so `global_init` must initialize it"
            );
        }
    }

    /// #7327 CI regression: an empty CPU string makes LLVM pick `generic`,
    /// which on aarch64 is ARMv8.0 and has no FEAT_JSCVT — so the
    /// `llvm.aarch64.fjcvtzs` that codegen emits for any Apple arm64 triple
    /// cannot be selected and the compile aborts. Clang defaults that triple to
    /// `apple-m1`, which is the assumption `set_jscvt_for_target` already makes.
    /// Reproduced with `PERRY_TARGET_CPU=generic`, which is the path CI took.
    #[test]
    fn apple_aarch64_defaults_to_a_cpu_with_feat_jscvt() {
        for triple in [
            "arm64-apple-macosx",
            "arm64-apple-darwin",
            "aarch64-apple-darwin",
            "arm64-apple-ios",
        ] {
            assert_eq!(
                default_cpu_for_triple(triple),
                "apple-m1",
                "{triple} must not fall back to LLVM's ARMv8.0 `generic`: codegen \
                 emits llvm.aarch64.fjcvtzs for Apple arm64 triples"
            );
        }
        // Everything else keeps LLVM's portable baseline, matching the clang
        // path when no tuning flag is passed.
        for triple in [
            "x86_64-apple-darwin",
            "aarch64-unknown-linux-gnu",
            "x86_64-unknown-linux-gnu",
        ] {
            assert_eq!(default_cpu_for_triple(triple), "", "{triple}");
        }
    }

    /// `-S` used to be swallowed by the catch-all that ignores `-c`, so the
    /// statepoint backends asked for assembly and were handed an object. The
    /// failure was invisible here and surfaced two steps later as
    /// `ld: unknown file type`, because #7314's compact-map rewriter rewrites
    /// `.llvm_stackmaps` in assembly *text* and had nothing to rewrite.
    #[test]
    fn dash_s_requests_assembly_and_dash_c_does_not() {
        let (_, _, _, _, emit_asm) =
            interpret_plan_args(&["-O2".into(), "-S".into()]).expect("args parse");
        assert!(emit_asm, "-S must request assembly");

        let (_, _, _, _, emit_asm) =
            interpret_plan_args(&["-O2".into(), "-c".into()]).expect("args parse");
        assert!(!emit_asm, "-c must still request an object");
    }

    /// The property the wiring depends on: the same module emitted with
    /// `FileType::Assembly` is assembler text carrying a stack-map section,
    /// not an object. If this ever silently produced an object again, the
    /// compact-map rewrite would find no `.llvm_stackmaps` to shrink and the
    /// GC would be reading an empty map — the #7332 shape, a binary that
    /// looks correct until a collection frees something live.
    #[test]
    fn assembly_emission_is_text_not_an_object() {
        let context = Context::create();
        let ir = r#"
define i32 @f(i32 %x) {
entry:
  %y = add i32 %x, 1
  ret i32 %y
}
"#;
        let module = parse_ir_text(&context, ir, "asm_probe").expect("probe parses");
        global_init(&[]);
        let triple = TargetMachine::get_default_triple();
        let target = Target::from_triple(&triple).expect("host target");
        let tm = target
            .create_target_machine(
                &triple,
                "",
                "",
                OptimizationLevel::None,
                RelocMode::PIC,
                CodeModel::Default,
            )
            .expect("target machine");

        let asm = tm
            .write_to_memory_buffer(&module, FileType::Assembly)
            .expect("assembly emission");
        let text = String::from_utf8_lossy(asm.as_slice()).to_string();
        assert!(
            text.contains(".globl") || text.contains(".global"),
            "expected assembler directives, got:\n{}",
            &text[..text.len().min(200)]
        );

        let obj = tm
            .write_to_memory_buffer(&module, FileType::Object)
            .expect("object emission");
        assert_ne!(
            asm.as_slice(),
            obj.as_slice(),
            "assembly and object emission returned identical bytes — `-S` is \
             being ignored somewhere in the emission path"
        );
    }
}
