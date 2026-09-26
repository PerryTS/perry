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
    let (cpu, features) = if mcpu_native {
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
        // Sizes before the rewrite: the budget message below names them, and
        // the per-unit report compares them with the post-rewrite census.
        let budget = rs4gc_instruction_budget();
        let preflight_cap = crate::codegen::helpers::root_spill_relocation_threshold();
        let rewritten_functions = rs4gc_functions(module);
        let pre_sizes = if budget == RewriteBudget::Off && preflight_cap == 0 && stats.is_none() {
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
        // The source-level estimate is intentionally cheap but can miss
        // codegen expansion (one expression becoming many collecting helper
        // calls). Check the actual constructed CallBase/root shape before
        // asking RS4GC to perform the potentially super-linear rewrite.
        enforce_rs4gc_preflight_budget(module, preflight_cap, &pre_sizes, &rewritten_functions)?;
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
    // TailCallElim runs inside every `default<O1+>` function-simplification
    // pipeline; bound its alloca walk on the module the pipeline will see
    // (#8883). `-O0` runs no TRE, so there is nothing to bound.
    if opt != '0' {
        let skipped = disable_tail_call_elim_over_budget(module, tre_walk_budget());
        for over in &skipped {
            eprintln!("perry: {over}");
        }
        if let Some(stats) = stats.as_deref_mut() {
            stats.tail_call_elim_skipped = skipped;
        }
    }
    let optimize_started = std::time::Instant::now();
    module
        .run_passes(pipeline, &tm, PassBuilderOptions::create())
        .map_err(|e| anyhow!("pass pipeline `{pipeline}` failed:\n{}", e.to_string()))?;
    if let Some(stats) = stats.as_deref_mut() {
        stats.optimize_secs = optimize_started.elapsed().as_secs_f64();
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

    // The IR pipeline above has already done the requested optimization. For
    // an extreme generated function, LLVM's optimized *machine* pipeline can
    // still become super-linear in instruction selection / LiveIntervals /
    // register allocation. Use an O0 target machine only for final emission
    // of such functions; the optimized IR is not rebuilt or demoted.
    //
    // A TargetMachine carries one optimization level for a whole module, and
    // `optnone` does not reach LiveIntervals or the register allocator, so the
    // offenders are split into a module of their own (#10586) and every
    // ordinary function in the unit keeps `tm`. Only a unit with nothing left
    // to free (every function over budget) is demoted whole.
    let mut fast_emit = if opt == '0' {
        Vec::new()
    } else {
        fast_emit_fallbacks(module, fast_emit_budget(effective_target))
    };
    let offenders: std::collections::HashSet<String> =
        fast_emit.iter().map(|f| f.name.clone()).collect();
    let whole_unit = if fast_emit.is_empty() {
        None
    } else {
        split_emit::whole_unit_reason(module, &offenders)
    };
    for fallback in &mut fast_emit {
        fallback.whole_unit = whole_unit;
        eprintln!("perry: {fallback}");
    }
    if let Some(stats) = stats.as_deref_mut() {
        stats.fast_emit_fallbacks = fast_emit.clone();
    }
    let fast_tm = if !fast_emit.is_empty() {
        Some(
            target
                .create_target_machine(
                    &triple,
                    &cpu,
                    &features,
                    OptimizationLevel::None,
                    RelocMode::PIC,
                    CodeModel::Default,
                )
                .ok_or_else(|| {
                    anyhow!(
                        "failed to create bounded O0 emission TargetMachine for \
                         `{effective_target}`"
                    )
                })?,
        )
    } else {
        None
    };
    let offender_module = match (&fast_tm, whole_unit) {
        (Some(_), None) => Some(split_emit::split_offenders(module, &offenders)?),
        _ => None,
    };

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
    let pieces = match (&fast_tm, &offender_module) {
        (Some(fast_tm), Some(offenders)) => {
            vec![emit(&tm, module)?, emit(fast_tm, &offenders.module)?]
        }
        (Some(fast_tm), None) => vec![emit(fast_tm, module)?],
        (None, _) => vec![emit(&tm, module)?],
    };
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

    /// The source-level estimate is only a fast first line of defence. This
    /// fixture pins the constructed-IR backstop: managed-root allocas count,
    /// ordinary calls count, explicit GC-leaf calls and LLVM intrinsics do
    /// not, and only functions which will actually enter RS4GC are governed.
    #[test]
    fn rs4gc_preflight_uses_constructed_roots_and_non_leaf_calls() {
        let fixture = r#"
declare i64 @may_collect()
declare i64 @leaf()
declare void @llvm.donothing()

define i64 @hot() gc "statepoint-example" {
entry:
  %root = alloca ptr addrspace(1)
  %plain = alloca i64
  %a = call i64 @may_collect()
  %b = call i64 @may_collect()
  %c = call i64 @leaf() "gc-leaf-function"
  call void @llvm.donothing()
  %p = load ptr addrspace(1), ptr %root
  %bits = ptrtoint ptr addrspace(1) %p to i64
  %sum = add i64 %a, %b
  %sum2 = add i64 %sum, %c
  %out = add i64 %sum2, %bits
  ret i64 %out
}

define i64 @shadow() {
entry:
  %root = alloca ptr addrspace(1)
  %a = call i64 @may_collect()
  ret i64 %a
}
"#;
        let context = Context::create();
        let module = parse_ir_text(&context, fixture, "preflight_fixture").expect("fixture parses");
        let hot = module.get_function("hot").expect("hot");
        assert_eq!(
            rs4gc_preflight_factors(hot),
            (1, 2),
            "plain allocas, leaf calls and intrinsics do not add RS4GC work"
        );

        // (one constructed root + two possible call-result roots) x two
        // safepoints = six estimated relocations. The boundary is exclusive.
        let rewritten_functions = rs4gc_functions(&module);
        assert_eq!(
            rs4gc_preflight_violations(&module, 5, &rewritten_functions),
            vec![("hot".to_string(), 1, 2, 6)]
        );
        assert!(rs4gc_preflight_violations(&module, 6, &rewritten_functions).is_empty());
        assert!(rs4gc_preflight_violations(&module, 0, &rewritten_functions).is_empty());

        let pre = pre_rewrite_sizes(&module);
        let err = enforce_rs4gc_preflight_budget(&module, 5, &pre, &rewritten_functions)
            .expect_err("the constructed shape requests a spill retry");
        let retry = rs4gc_budget_retry(&err).expect("the request stays typed");
        assert_eq!(retry.len(), 1);
        assert_eq!(retry[0].name, "hot");
        assert_eq!(retry[0].pre_instructions, pre.get("hot").copied());
        assert_eq!(
            retry[0].cause,
            Rs4gcBudgetCause::PreRewrite {
                root_allocas: 1,
                safepoints: 2,
                estimated_relocations: 6,
            }
        );
        assert_eq!(retry[0].cap, 5);
        let msg = format!("{err:#}");
        for needle in [
            "before rewrite-statepoints-for-gc",
            "`hot`",
            "1 managed-root allocas",
            "2 non-leaf call sites",
            "predicts 6 relocations",
            "budget 5",
            "PERRY_ROOT_SPILL_RELOCATIONS",
            "re-lower",
        ] {
            assert!(
                msg.contains(needle),
                "message must carry {needle:?}:\n{msg}"
            );
        }

        let no_rewritten_functions = std::collections::HashSet::new();
        enforce_rs4gc_preflight_budget(&module, 1, &pre, &no_rewritten_functions)
            .expect("a shadow-root function is outside the preflight budget");
    }

    /// Six gc values live across forty safepoints: ~60 instructions before
    /// `rewrite-statepoints-for-gc`, a few hundred after (each statepoint
    /// relocates every live value). A budget between the two is exceeded
    /// only by the post-rewrite module — which is the property the
    /// assertion exists for. Counting BEFORE the rewrite (the #8421
    /// replacement knob's mistake) would make `after` empty and fail here.
    fn relocation_fanout_fixture() -> String {
        let mut ir = String::from(
            "declare i64 @may_collect()\n\n\
             define i64 @f(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5) gc \"statepoint-example\" {\n\
             entry:\n",
        );
        for i in 0..6 {
            ir.push_str(&format!(
                "  %p{i} = inttoptr i64 %a{i} to ptr addrspace(1)\n"
            ));
        }
        for c in 0..40 {
            ir.push_str(&format!("  %c{c} = call i64 @may_collect()\n"));
        }
        for i in 0..6 {
            ir.push_str(&format!(
                "  %b{i} = ptrtoint ptr addrspace(1) %p{i} to i64\n"
            ));
        }
        ir.push_str(
            "  %s0 = add i64 %b0, %b1\n  %s1 = add i64 %s0, %b2\n  %s2 = add i64 %s1, %b3\n\
             \x20 %s3 = add i64 %s2, %b4\n  %s4 = add i64 %s3, %b5\n  %s5 = add i64 %s4, %c0\n\
             \x20 %s6 = add i64 %s5, %c39\n  ret i64 %s6\n}\n",
        );
        ir
    }

    #[test]
    fn rs4gc_budget_fires_only_on_the_rewritten_module() {
        global_init(&[]);
        let target = "arm64-apple-darwin";
        let fixture = relocation_fanout_fixture();
        let rewritten = statepoint_rewritten_ir(&fixture, target, "fanout_budget")
            .expect("fan-out fixture must run RS4GC");

        let context = Context::create();
        let before = parse_ir_text(&context, &fixture, "fanout_before").expect("fixture parses");
        let after = parse_ir_text(&context, &rewritten, "fanout_after").expect("rewritten parses");
        let pre = pre_rewrite_sizes(&before);
        let rewritten_functions = rs4gc_functions(&before);
        let pre_f = pre["f"];
        let (_, post_total, post_widest) = module_instruction_census(&after);
        let post_f = post_widest.as_ref().map(|(_, n)| *n).unwrap_or(0);
        assert!(
            post_f > 3 * pre_f,
            "fixture must grow under relocation fan-out (pre {pre_f}, post {post_f}):\n{rewritten}"
        );
        assert_eq!(post_total, post_f, "one defined function");
        let cap = pre_f + (post_f - pre_f) / 2;

        assert!(
            rs4gc_budget_violations(&before, cap, &rewritten_functions).is_empty(),
            "the pre-rewrite module is under the budget by construction"
        );
        let over = rs4gc_budget_violations(&after, cap, &rewritten_functions);
        assert_eq!(
            over.len(),
            1,
            "exactly the rewritten body is over: {over:?}"
        );
        assert_eq!(over[0].0, "f");
        assert_eq!(over[0].1, post_f);

        let err = enforce_rs4gc_instruction_budget(
            &after,
            RewriteBudget::Error(cap),
            &pre,
            &rewritten_functions,
        )
        .expect_err("the default spelling requests a spill retry");
        let retry = rs4gc_budget_retry(&err).expect("the request stays typed");
        assert_eq!(retry.len(), 1);
        assert_eq!(retry[0].name, "f");
        assert_eq!(retry[0].pre_instructions, Some(pre_f));
        assert_eq!(
            retry[0].cause,
            Rs4gcBudgetCause::PostRewrite {
                post_instructions: post_f
            }
        );
        assert_eq!(retry[0].cap, cap);
        let msg = format!("{err:#}");
        for needle in [
            "`f`",
            &format!("to {post_f} instructions"),
            &format!("it was {pre_f} before"),
            &format!("budget is {cap}"),
            "PERRY_LL_RS4GC_MAX_INSTRS",
            "re-lower",
            "#8679",
        ] {
            assert!(
                msg.contains(needle),
                "message must carry {needle:?}:\n{msg}"
            );
        }
        assert!(
            !msg.contains("optnone"),
            "the budget is an assertion, never a demotion:\n{msg}"
        );
        enforce_rs4gc_instruction_budget(
            &after,
            RewriteBudget::Warn(cap),
            &pre,
            &rewritten_functions,
        )
        .expect("warn spelling does not retry");
        enforce_rs4gc_instruction_budget(&after, RewriteBudget::Off, &pre, &rewritten_functions)
            .expect("off spelling does not retry");
        enforce_rs4gc_instruction_budget(
            &after,
            RewriteBudget::Error(post_f),
            &pre,
            &rewritten_functions,
        )
        .expect("a budget at the exact size is not exceeded");

        // The retry removes the function's GC strategy. Its ordinary shadow
        // body may itself exceed a deliberately tiny test cap, but it must not
        // request the same spill forever: only functions that entered RS4GC
        // are governed by this relocation-fan-out budget.
        let no_rewritten_functions = std::collections::HashSet::new();
        enforce_rs4gc_instruction_budget(
            &after,
            RewriteBudget::Error(cap),
            &pre,
            &no_rewritten_functions,
        )
        .expect("a shadow-spilled function is outside the RS4GC budget");
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
    #[test]
    fn tre_walk_budget_spellings() {
        assert_eq!(
            parse_tre_walk_budget(None),
            TreWalkBudget::Cap(DEFAULT_TRE_MAX_ALLOCA_WALK)
        );
        assert_eq!(
            parse_tre_walk_budget(Some("")),
            TreWalkBudget::Cap(DEFAULT_TRE_MAX_ALLOCA_WALK)
        );
        assert_eq!(parse_tre_walk_budget(Some("0")), TreWalkBudget::Off);
        assert_eq!(parse_tre_walk_budget(Some("off")), TreWalkBudget::Off);
        assert_eq!(parse_tre_walk_budget(Some("false")), TreWalkBudget::Off);
        assert_eq!(
            parse_tre_walk_budget(Some(" 250000 ")),
            TreWalkBudget::Cap(250_000)
        );
        assert_eq!(
            parse_tre_walk_budget(Some("lots")),
            TreWalkBudget::Cap(DEFAULT_TRE_MAX_ALLOCA_WALK)
        );
    }

    /// Spellings, on both an x86-64 and an arm64 target: only the default
    /// differs between them, and an explicit setting wins on either.
    #[test]
    fn fast_emit_budget_spellings() {
        for (target, default) in [
            ("x86_64-unknown-linux-gnu", 600_000),
            ("arm64-apple-darwin", 100_000),
        ] {
            assert_eq!(
                parse_fast_emit_budget(None, target),
                FastEmitBudget::Cap(default),
                "{target}"
            );
            assert_eq!(
                parse_fast_emit_budget(Some(""), target),
                FastEmitBudget::Cap(default),
                "{target}"
            );
            assert_eq!(
                parse_fast_emit_budget(Some("0"), target),
                FastEmitBudget::Off
            );
            assert_eq!(
                parse_fast_emit_budget(Some("off"), target),
                FastEmitBudget::Off
            );
            assert_eq!(
                parse_fast_emit_budget(Some("false"), target),
                FastEmitBudget::Off
            );
            assert_eq!(
                parse_fast_emit_budget(Some(" 250000 "), target),
                FastEmitBudget::Cap(250_000),
                "an explicit ceiling wins on every target"
            );
            assert_eq!(
                parse_fast_emit_budget(Some("lots"), target),
                FastEmitBudget::Cap(default),
                "{target}"
            );
        }
    }

    /// Two functions: `wide` has 4 allocas across 9 instructions (estimate
    /// 36), `narrow` has one across 3 (estimate 3), and `decl` has no body.
    fn alloca_walk_fixture() -> &'static str {
        r#"
declare void @sink(ptr)

define void @wide() {
entry:
  %a = alloca i64
  %b = alloca i64
  %c = alloca i64
  %d = alloca i64
  call void @sink(ptr %a)
  call void @sink(ptr %b)
  call void @sink(ptr %c)
  call void @sink(ptr %d)
  ret void
}

define void @narrow() {
entry:
  %a = alloca i64
  call void @sink(ptr %a)
  ret void
}
"#
    }

    fn has_disable_tail_calls(module: &inkwell::module::Module<'_>, name: &str) -> bool {
        module
            .get_function(name)
            .expect("fixture function exists")
            .get_string_attribute(
                inkwell::attributes::AttributeLoc::Function,
                DISABLE_TAIL_CALLS_ATTR,
            )
            .is_some_and(|attr| attr.get_string_value().to_bytes() == b"true")
    }

    /// The budget is `allocas × instructions`, applied per function: only
    /// the function over it is stamped, the boundary is exclusive, and
    /// `off` stamps nothing.
    #[test]
    fn tre_budget_stamps_only_the_function_over_it() {
        let context = Context::create();
        let module = parse_ir_text(&context, alloca_walk_fixture(), "tre_budget_fixture")
            .expect("fixture parses");
        let wide = module.get_function("wide").expect("wide");
        let narrow = module.get_function("narrow").expect("narrow");
        assert_eq!(alloca_walk_factors(wide), (4, 9));
        assert_eq!(alloca_walk_factors(narrow), (1, 3));

        assert!(
            disable_tail_call_elim_over_budget(&module, TreWalkBudget::Off).is_empty(),
            "a disabled budget stamps nothing"
        );
        assert!(!has_disable_tail_calls(&module, "wide"));

        let exact = disable_tail_call_elim_over_budget(&module, TreWalkBudget::Cap(36));
        assert!(exact.is_empty(), "the cap is inclusive: {exact:?}");

        let over = disable_tail_call_elim_over_budget(&module, TreWalkBudget::Cap(35));
        assert_eq!(
            over,
            vec![TreWalkOverBudget {
                name: "wide".to_string(),
                allocas: 4,
                instructions: 9,
                cap: 35,
            }]
        );
        assert!(has_disable_tail_calls(&module, "wide"));
        assert!(!has_disable_tail_calls(&module, "narrow"));
        let message = over[0].to_string();
        for needle in [
            "`wide`",
            "4 allocas",
            "9 instructions",
            "estimate 36",
            "budget 35",
            "PERRY_LL_TRE_MAX_ALLOCA_WALK",
            "#8883",
        ] {
            assert!(
                message.contains(needle),
                "{needle} missing from:\n{message}"
            );
        }
        assert!(
            !message.contains("optnone"),
            "the budget must never read as a demotion:\n{message}"
        );
    }

    /// Selection is per function, the boundary is inclusive, declarations do
    /// not count, every violator is reported (widest first) and the
    /// diagnostic names the function and the size of the collateral.
    #[test]
    fn fast_emit_budget_selects_only_above_the_boundary() {
        let context = Context::create();
        let module = parse_ir_text(&context, alloca_walk_fixture(), "fast_emit_fixture")
            .expect("fixture parses");
        assert!(fast_emit_fallbacks(&module, FastEmitBudget::Off).is_empty());
        assert!(fast_emit_fallbacks(&module, FastEmitBudget::Cap(9)).is_empty());

        let fallbacks = fast_emit_fallbacks(&module, FastEmitBudget::Cap(8));
        assert_eq!(
            fallbacks,
            vec![FastEmitFallback {
                name: "wide".to_string(),
                instructions: 9,
                cap: 8,
                // `narrow` is under the cap and `sink` is a declaration.
                unit_functions: 2,
                unit_offenders: 1,
                // Decided by the emission path, which can see the aliases.
                whole_unit: None,
            }],
            "only the function over the budget is selected"
        );

        // Every violator, widest first: one build names the whole set that
        // has to shrink instead of re-reporting a new widest each time.
        let both = fast_emit_fallbacks(&module, FastEmitBudget::Cap(2));
        let named: Vec<(&str, usize)> = both
            .iter()
            .map(|f| (f.name.as_str(), f.instructions))
            .collect();
        assert_eq!(named, [("wide", 9), ("narrow", 3)]);

        let mut fallback = fallbacks[0].clone();
        let common = [
            "`wide`",
            "9 instructions",
            "budget 8",
            "requested IR optimization",
            "O0 machine pipeline",
            "PERRY_LL_FAST_EMIT_MAX_INSTRS",
        ];
        let split = [
            "1 over-budget function(s)",
            "object of their own",
            "other 1 function(s) keep the optimized machine pipeline",
        ];
        let whole = ["all 2 of its defined functions"];
        let check = |message: &str, present: &[&str], absent: &[&str]| {
            for needle in common.iter().chain(present) {
                assert!(
                    message.contains(needle),
                    "{needle:?} missing from:\n{message}"
                );
            }
            for needle in absent {
                assert!(
                    !message.contains(needle),
                    "{needle:?} wrongly in:\n{message}"
                );
            }
        };
        check(&fallback.to_string(), &split, &whole);
        fallback.whole_unit = Some(WholeUnitReason::NoSiblings);
        check(
            &fallback.to_string(),
            &[whole[0], "every one over the budget"],
            &split,
        );
        fallback.whole_unit = Some(WholeUnitReason::AliasOrIfunc);
        check(
            &fallback.to_string(),
            &[whole[0], "global alias or ifunc"],
            &split,
        );
    }

    /// The ceiling is a calibration, not a round number, and it is only as
    /// wide as the target it was measured on. x86-64 must sit above the
    /// measured population of extreme generated functions (OpenCode corpus:
    /// 60 of the 61 functions past the old 100k ceiling are under 600k, and
    /// the largest one measured end to end is `mime` `types/other.ts`'s
    /// 522,756-instruction constructor, which emits through the optimized
    /// machine pipeline in 704 s at 2.26 GB) and below the RS4GC
    /// relocation-fan-out budget, which is the backstop that runs first and
    /// re-lowers rather than demotes. Every unmeasured target — aarch64
    /// above all, where both pathological observations were made — keeps
    /// 100k. Raising one of those is a measurement, not an edit.
    #[test]
    fn fast_emit_budget_admits_the_measured_giant_population_on_measured_targets_only() {
        assert_eq!(DEFAULT_FAST_EMIT_MAX_INSTRS_X86_64, 600_000);
        assert!(
            DEFAULT_FAST_EMIT_MAX_INSTRS_X86_64 > 522_756,
            "the largest measured-affordable function must not be demoted"
        );
        assert!(
            DEFAULT_FAST_EMIT_MAX_INSTRS_X86_64 < DEFAULT_RS4GC_MAX_INSTRS,
            "a function this wide is handled by the RS4GC budget first"
        );
        assert_eq!(DEFAULT_FAST_EMIT_MAX_INSTRS_UNMEASURED, 100_000);

        for target in [
            "x86_64-unknown-linux-gnu",
            "x86_64-apple-darwin",
            "x86_64h-apple-darwin",
        ] {
            assert_eq!(
                default_fast_emit_max_instrs(target),
                600_000,
                "{target} is measured"
            );
        }
        // The host is irrelevant: a cross-compile runs the *target's* ISel and
        // register allocator, so these keep the unmeasured ceiling even from an
        // x86-64 box.
        for target in [
            "arm64-apple-darwin",
            "aarch64-apple-darwin",
            "aarch64-unknown-linux-gnu",
            "arm64_32-apple-watchos",
            "i686-unknown-linux-gnu",
            "",
        ] {
            assert_eq!(
                default_fast_emit_max_instrs(target),
                100_000,
                "{target:?} has not been measured"
            );
        }
    }

    /// A self-recursive tail call that TailCallElim turns into a loop at
    /// the pinned LLVM: with no attribute the recursive `call` disappears,
    /// with `"disable-tail-calls"="true"` (exactly what the budget stamps)
    /// it survives the full `default<Os>` pipeline — so the lever the
    /// budget pulls is live, not merely spelled.
    fn tail_recursive_fixture(attrs: &str) -> String {
        format!(
            "define i64 @count_down(i64 %n, i64 %acc) noinline {attrs} {{\n\
             entry:\n\
             \x20 %done = icmp eq i64 %n, 0\n\
             \x20 br i1 %done, label %ret, label %rec\n\
             rec:\n\
             \x20 %n1 = sub i64 %n, 1\n\
             \x20 %acc1 = add i64 %acc, %n\n\
             \x20 %r = call i64 @count_down(i64 %n1, i64 %acc1)\n\
             \x20 ret i64 %r\n\
             ret:\n\
             \x20 ret i64 %acc\n\
             }}\n"
        )
    }

    #[test]
    fn disable_tail_calls_attribute_stops_tail_call_elim_at_the_pinned_llvm() {
        let target = crate::codegen::default_target_triple();
        let with_tre = statepoint_rewritten_ir_with_passes(
            &tail_recursive_fixture(""),
            &target,
            "tre_control",
            "default<Os>",
        )
        .expect("control optimizes");
        assert!(
            !with_tre.contains("call i64 @count_down"),
            "control: TailCallElim must turn the tail recursion into a loop, or this test \
             cannot tell the attribute apart from a no-op:\n{with_tre}"
        );

        let without_tre = statepoint_rewritten_ir_with_passes(
            &tail_recursive_fixture(&format!("\"{DISABLE_TAIL_CALLS_ATTR}\"=\"true\"")),
            &target,
            "tre_disabled",
            "default<Os>",
        )
        .expect("attributed fixture optimizes");
        assert!(
            without_tre.contains("call i64 @count_down"),
            "the attribute must keep TailCallElim off the function:\n{without_tre}"
        );
    }

    /// The budget is wired into the shipped emission path: under a cap of
    /// zero every function with an alloca is stamped before `default<O*>`
    /// runs, the per-unit stats name it, and the unit still emits.
    #[test]
    fn tre_budget_is_applied_by_the_shipped_pipeline() {
        global_init(&[]);
        let target = crate::codegen::default_target_triple();
        let context = Context::create();
        let module = parse_ir_text(&context, alloca_walk_fixture(), "tre_budget_shipped")
            .expect("fixture parses");
        let mut stats = UnitCodegenStats::default();
        let object = with_test_tre_walk_budget(0, || {
            optimize_and_emit_module_with_stats(
                &module,
                &target,
                &["-Os".into(), "-c".into()],
                false,
                Some(&mut stats),
            )
        })
        .expect("a stamped module still optimizes and emits");
        assert!(!object.is_empty());
        let mut names: Vec<&str> = stats
            .tail_call_elim_skipped
            .iter()
            .map(|over| over.name.as_str())
            .collect();
        names.sort_unstable();
        assert_eq!(names, ["narrow", "wide"]);

        // -O0 runs no TailCallElim, so nothing is stamped there.
        let module = parse_ir_text(&context, alloca_walk_fixture(), "tre_budget_o0")
            .expect("fixture parses");
        let mut stats = UnitCodegenStats::default();
        with_test_tre_walk_budget(0, || {
            optimize_and_emit_module_with_stats(
                &module,
                &target,
                &["-O0".into(), "-c".into()],
                false,
                Some(&mut stats),
            )
        })
        .expect("-O0 emits");
        assert!(stats.tail_call_elim_skipped.is_empty());
        assert!(!has_disable_tail_calls(&module, "wide"));
    }

    /// A tiny test cap proves the shipped path records and successfully uses
    /// the second, O0 target machine only after running the requested Os IR
    /// pipeline. The production threshold is pinned by the parser test and
    /// the real measurements in its constant's documentation.
    #[test]
    fn fast_emit_budget_is_applied_by_the_shipped_pipeline() {
        global_init(&[]);
        let target = crate::codegen::default_target_triple();
        let context = Context::create();
        let module = parse_ir_text(&context, alloca_walk_fixture(), "fast_emit_shipped")
            .expect("fixture parses");
        let mut stats = UnitCodegenStats::default();
        let object = with_test_fast_emit_budget(1, || {
            optimize_and_emit_module_with_stats(
                &module,
                &target,
                &["-Os".into(), "-c".into()],
                false,
                Some(&mut stats),
            )
        })
        .expect("the already-optimized module emits through the bounded target machine");
        assert!(!object.is_empty());
        let fallback = stats
            .fast_emit_fallbacks
            .first()
            .expect("the shipped path must report the selected fallback")
            .clone();
        assert_eq!(fallback.name, "wide");
        assert!(fallback.instructions > fallback.cap);
        assert_eq!(fallback.cap, 1);
        // Both of the fixture's functions are over a cap of 1: nothing is left
        // to free, so the unit is demoted whole, as one piece.
        assert_eq!(fallback.whole_unit, Some(WholeUnitReason::NoSiblings));
        assert_eq!(object.len(), 1);

        // A requested O0 compile already uses the bounded target machine; it
        // neither needs nor reports a fallback.
        let module =
            parse_ir_text(&context, alloca_walk_fixture(), "fast_emit_o0").expect("fixture parses");
        let mut stats = UnitCodegenStats::default();
        with_test_fast_emit_budget(1, || {
            optimize_and_emit_module_with_stats(
                &module,
                &target,
                &["-O0".into(), "-c".into()],
                false,
                Some(&mut stats),
            )
        })
        .expect("-O0 emits");
        assert!(stats.fast_emit_fallbacks.is_empty());
    }

    /// One function over the budget, one ordinary function that is nowhere
    /// near it, and one external callee so nothing folds away. `narrow`
    /// holds three values across three calls, which is what makes its
    /// machine code differ between the optimized and the O0 register
    /// allocators.
    fn sibling_cost_fixture(with_wide: bool) -> String {
        let wide = r#"
define i64 @wide(i64 %n) {
entry:
  %a = call i64 @src(i64 %n)
  %b = call i64 @src(i64 %a)
  %c = call i64 @src(i64 %b)
  %d = call i64 @src(i64 %c)
  %s = add i64 %a, %b
  %t = add i64 %s, %c
  %u = add i64 %t, %d
  ret i64 %u
}
"#;
        format!(
            r#"
declare i64 @src(i64)

define i64 @narrow(i64 %x, i64 %y) {{
entry:
  %a = call i64 @src(i64 %x)
  %b = call i64 @src(i64 %y)
  %c = call i64 @src(i64 %a)
  %s = add i64 %a, %b
  %t = add i64 %s, %c
  ret i64 %t
}}
{}"#,
            if with_wide { wide } else { "" }
        )
    }

    /// The assembly of one function, from its label to the end of its body.
    /// Tolerates ELF (`narrow:` / `.size`) and Mach-O (`_narrow:`) spelling.
    fn function_assembly(asm: &str, name: &str) -> String {
        let label_elf = format!("{name}:");
        let label_macho = format!("_{name}:");
        let mut body: Vec<&str> = Vec::new();
        let mut inside = false;
        for line in asm.lines() {
            let trimmed = line.trim();
            if !inside {
                inside = trimmed == label_elf || trimmed == label_macho;
                continue;
            }
            let next_symbol = trimmed.ends_with(':')
                && !trimmed.starts_with('.')
                && !trimmed.starts_with('L')
                && !trimmed.contains(' ');
            if trimmed.starts_with(".size") || trimmed == ".cfi_endproc" || next_symbol {
                break;
            }
            body.push(line);
        }
        assert!(
            body.len() > 3,
            "no body extracted for `{name}` — the assertion below would be vacuous:\n{asm}"
        );
        body.join("\n")
    }

    /// Every emitted piece's assembly (one normally, two when the budget
    /// split the offenders out), and the names the budget selected.
    fn emit_assembly(
        ir: &str,
        module_name: &str,
        budget: FastEmitBudget,
    ) -> (Vec<String>, Vec<FastEmitFallback>) {
        global_init(&[]);
        let target = crate::codegen::default_target_triple();
        let context = Context::create();
        let module = parse_ir_text(&context, ir, module_name).expect("fixture parses");
        let mut stats = UnitCodegenStats::default();
        let pieces = with_test_fast_emit_budget_value(budget, || {
            optimize_and_emit_module_with_stats(
                &module,
                &target,
                &["-Os".into(), "-S".into()],
                false,
                Some(&mut stats),
            )
        })
        .expect("the fixture emits");
        (
            pieces
                .into_iter()
                .map(|asm| String::from_utf8(asm).expect("LLVM emits UTF-8 assembly"))
                .collect(),
            stats.fast_emit_fallbacks,
        )
    }

    fn defines(asm: &str, name: &str) -> bool {
        asm.lines()
            .map(str::trim)
            .any(|line| line == format!("{name}:") || line == format!("_{name}:"))
    }

    /// #10586: the demotion used to be a whole-unit act — an ordinary
    /// function sharing a unit with an extreme one was compiled differently
    /// the moment the budget was crossed, without ever having been over any
    /// budget itself. The offenders are now emitted in an object of their
    /// own, so the ordinary function's code is exactly what an undemoted unit
    /// (and a unit without the extreme function) emits.
    #[test]
    fn the_budget_no_longer_makes_ordinary_siblings_pay() {
        let with_wide = sibling_cost_fixture(true);
        let alone = sibling_cost_fixture(false);

        let (undemoted, none) = emit_assembly(&with_wide, "sibling_cost_ok", FastEmitBudget::Off);
        assert!(none.is_empty(), "this arm must not demote: {none:?}");
        assert_eq!(undemoted.len(), 1, "an undemoted unit emits one piece");
        let (solo, _) = emit_assembly(&alone, "sibling_cost_alone", FastEmitBudget::Off);
        assert_eq!(
            function_assembly(&undemoted[0], "narrow"),
            function_assembly(&solo[0], "narrow"),
            "an undemoted unit emits an ordinary function exactly as a unit without the \
             extreme function does"
        );

        // Same module, same IR pipeline, budget crossed by `wide` only.
        let (split, over) =
            emit_assembly(&with_wide, "sibling_cost_demoted", FastEmitBudget::Cap(7));
        let names: Vec<&str> = over.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["wide"], "only `wide` is over the budget");
        assert_eq!(over[0].whole_unit, None, "a unit with a sibling is split");
        assert_eq!((over[0].unit_functions, over[0].unit_offenders), (2, 1));
        assert_eq!(split.len(), 2, "siblings and offenders are emitted apart");
        let (siblings, offenders) = (&split[0], &split[1]);
        assert!(defines(siblings, "narrow") && !defines(siblings, "wide"));
        assert!(defines(offenders, "wide") && !defines(offenders, "narrow"));
        assert_eq!(
            function_assembly(siblings, "narrow"),
            function_assembly(&undemoted[0], "narrow"),
            "the sibling keeps the optimized machine pipeline"
        );

        // Control: the offender really went through the O0 machine pipeline
        // — its code differs from what the optimized pipeline emits for it —
        // or the equality above could not tell a split from no demotion.
        assert_ne!(
            function_assembly(offenders, "wide"),
            function_assembly(&undemoted[0], "wide"),
            "the offender must still be emitted through the O0 machine pipeline"
        );
    }
}
