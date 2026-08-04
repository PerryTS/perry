//! Clang discovery, version preflight, compile-plan shaping, and temp-path
//! naming — the unit-test half of `linker.rs`.
//!
//! Split out for the 2,000-line file cap, not because it is a different
//! subject; `use super::*` gives it the same view of the module as before.

use super::*;

fn version_block(target_line: &str) -> String {
    format!("clang version 18.0.0\n{}\nThread model: posix", target_line)
}

#[test]
fn parses_common_clang_version_banners() {
    assert_eq!(
        parse_clang_major_version("Ubuntu clang version 14.0.0-1ubuntu1.1"),
        Some(14)
    );
    assert_eq!(
        parse_clang_major_version("Apple clang version 17.0.0 (clang-1700.0.13.5)"),
        Some(17)
    );
    assert_eq!(
        parse_clang_major_version("Debian clang version 18.1.8\nTarget: x86_64-linux-gnu"),
        Some(18)
    );
    assert_eq!(parse_clang_major_version("not a clang banner"), None);
}

#[test]
fn version_banner_on_stderr_wins_over_stdout_wrapper_noise() {
    let selected = select_clang_version_output(
        "wrapper: selecting system toolchain",
        "Ubuntu clang version 14.0.0-1ubuntu1.1",
    );
    assert_eq!(
        selected.as_deref(),
        Some("Ubuntu clang version 14.0.0-1ubuntu1.1")
    );
    assert_eq!(
        select_clang_version_output("clang version 18.1.8", "warning").as_deref(),
        Some("clang version 18.1.8")
    );
}

#[test]
fn prefers_supported_versioned_clang_over_old_path_default() {
    let candidates = vec![
        PathBuf::from("/usr/bin/clang"),
        PathBuf::from("/usr/bin/clang-18"),
        PathBuf::from("/usr/bin/clang-15"),
    ];
    let selected =
        select_clang_candidate_with(candidates, |path| match path.file_name()?.to_str()? {
            "clang" => Some(14),
            "clang-18" => Some(18),
            "clang-15" => Some(15),
            _ => None,
        });
    assert_eq!(selected, Some(PathBuf::from("/usr/bin/clang-18")));
}

#[test]
fn retains_first_candidate_to_report_an_old_only_install() {
    let candidates = vec![
        PathBuf::from("/usr/bin/clang"),
        PathBuf::from("/usr/bin/clang-14"),
    ];
    let selected = select_clang_candidate_with(candidates, |_| Some(14));
    assert_eq!(selected, Some(PathBuf::from("/usr/bin/clang")));
}

#[test]
fn old_clang_preflight_explains_the_opaque_pointer_requirement() {
    let error = ensure_supported_clang_major(Path::new("/usr/bin/clang"), Some(14))
        .expect_err("clang 14 must be rejected");
    let message = error.to_string();
    assert!(message.contains("too old (14 < 15)"));
    assert!(message.contains("opaque-pointer LLVM IR"));
    assert!(message.contains("PERRY_LLVM_CLANG"));
    assert!(ensure_supported_clang_major(Path::new("/usr/bin/clang-15"), Some(15)).is_ok());
    assert!(ensure_supported_clang_major(Path::new("/toolchain-wrapper"), None).is_ok());
}

#[test]
fn hint_for_mingw_clang_on_windows_targets_msvc() {
    // Only the host-is-windows arm fires this hint. The build matrix runs
    // these tests on every host, so we gate the assertion on cfg(windows).
    // On non-Windows hosts the function falls through to the generic
    // PERRY_LLVM_CLANG suggestion — also asserted below.
    let v = version_block("Target: x86_64-w64-windows-gnu");
    let hint = build_clang_failure_hint(
        "lld-link: error: undefined symbol: __main",
        &v,
        "x86_64-pc-windows-msvc",
    );
    if cfg!(target_os = "windows") {
        assert!(
            hint.contains("MinGW/GNU"),
            "expected MinGW hint, got: {}",
            hint
        );
        assert!(hint.contains("winget install LLVM.LLVM"));
        assert!(hint.contains("PERRY_LLVM_CLANG"));
    } else {
        assert!(hint.contains("PERRY_LLVM_CLANG"));
    }
}

#[test]
fn hint_for_override_module_target_triple_warning() {
    let v = version_block("Target: x86_64-pc-linux-gnu");
    let hint = build_clang_failure_hint(
        "warning: overriding the module target triple with x86_64-pc-linux-gnu",
        &v,
        "x86_64-unknown-linux-gnu",
    );
    // On non-Windows hosts the override-warning branch should win.
    if !cfg!(target_os = "windows") {
        assert!(
            hint.contains("overriding the module target triple"),
            "expected override hint, got: {}",
            hint
        );
    }
}

#[test]
fn hint_for_missing_library_message() {
    let v = version_block("Target: aarch64-apple-darwin23.0.0");
    let hint = build_clang_failure_hint(
        "ld: library not found for -lSystem",
        &v,
        "arm64-apple-macosx15.0.0",
    );
    assert!(
        hint.contains("library") || hint.contains("PERRY_LLVM_CLANG"),
        "got: {}",
        hint
    );
}

#[test]
fn hint_falls_back_when_no_pattern_matches() {
    let v = version_block("Target: aarch64-apple-darwin23.0.0");
    let hint = build_clang_failure_hint(
        "(some unrelated clang stderr)",
        &v,
        "arm64-apple-macosx15.0.0",
    );
    assert!(
        hint.contains("PERRY_LLVM_CLANG"),
        "fallback hint should mention PERRY_LLVM_CLANG; got: {}",
        hint
    );
    assert!(hint.contains("arm64-apple-macosx15.0.0"));
}

#[test]
fn compile_plan_records_effective_target_and_native_tuning() {
    let plan = build_clang_compile_plan(
        PathBuf::from("clang"),
        PathBuf::from("/tmp/input.ll"),
        PathBuf::from("/tmp/output.o"),
        None,
        0,
        0,
        false,
    );
    assert!(plan.clang_args.contains(&"-fno-math-errno".to_string()));
    // Small module → optimized at -O3 (#4880).
    assert!(plan.clang_args.contains(&"-O3".to_string()));
    assert!(plan.clang_args.contains(&"-target".to_string()));
    assert!(plan.analysis_clang_args.contains(&"-target".to_string()));
    // Apple aarch64 pins `apple-m1` rather than `native`: the decision to emit
    // `llvm.aarch64.fjcvtzs` is made from the triple, and `native` broke that
    // pair on a virtualised CI runner where detection disagreed. Every other
    // host keeps native tuning.
    let expected = if cfg!(all(target_vendor = "apple", target_arch = "aarch64")) {
        "-mcpu=apple-m1"
    } else {
        native_tuning_arg_for_host()
    };
    assert_eq!(plan.native_tuning_arg.as_deref(), Some(expected));
    assert!(!plan.effective_target.is_empty());
}

#[test]
fn compile_plan_size_optimizes_oversized_many_function_module() {
    // An oversized unit made of many ordinary functions (a large minified
    // bundle: low bytes-per-function) size-optimizes at -Os — far less
    // __text than -O0 — rather than dropping to the speed pipeline or -O0.
    let huge = ll_o0_threshold_bytes() + 1;
    let many_funcs = huge / 1024; // ~1 KB/fn, well under the density cap
    let plan = build_clang_compile_plan(
        PathBuf::from("clang"),
        PathBuf::from("/tmp/input.ll"),
        PathBuf::from("/tmp/output.o"),
        None,
        huge,
        many_funcs,
        false,
    );
    assert!(plan.clang_args.contains(&"-Os".to_string()));
    assert!(!plan.clang_args.contains(&"-O3".to_string()));
    assert!(!plan.clang_args.contains(&"-O0".to_string()));
}

#[test]
fn compile_plan_keeps_o0_for_oversized_giant_function_monolith() {
    // #4880: an oversized unit dominated by a few giant generated functions
    // (a multi-thousand-element data literal: megabytes-per-function) keeps
    // -O0, the only opt level whose pipeline finishes in practical time.
    let huge = ll_o0_threshold_bytes() + 1;
    let plan = build_clang_compile_plan(
        PathBuf::from("clang"),
        PathBuf::from("/tmp/input.ll"),
        PathBuf::from("/tmp/output.o"),
        None,
        huge,
        2, // ~3 MB/fn — far above the density cap
        false,
    );
    assert!(plan.clang_args.contains(&"-O0".to_string()));
    assert!(!plan.clang_args.contains(&"-O3".to_string()));
    assert!(!plan.clang_args.contains(&"-Os".to_string()));
}

#[test]
fn compile_plan_skips_native_tuning_for_explicit_target() {
    let plan = build_clang_compile_plan(
        PathBuf::from("clang"),
        PathBuf::from("/tmp/input.ll"),
        PathBuf::from("/tmp/output.o"),
        Some("x86_64-unknown-linux-gnu"),
        0,
        0,
        false,
    );
    assert_eq!(plan.effective_target, "x86_64-unknown-linux-gnu");
    assert_eq!(plan.native_tuning_arg, None);
    assert!(!plan
        .clang_args
        .iter()
        .any(|arg| arg == "-march=native" || arg == "-mcpu=native"));
}

#[test]
fn cpu_tuning_unset_keeps_historical_defaults() {
    // Host build (no triple) → native tuning; explicit triple → none.
    assert_eq!(
        cpu_tuning_arg_for(None, None, "x86_64-apple-darwin").as_deref(),
        Some(native_tuning_arg_for_host())
    );
    assert_eq!(
        cpu_tuning_arg_for(
            None,
            Some("x86_64-unknown-linux-gnu"),
            "x86_64-unknown-linux-gnu"
        ),
        None
    );
}

#[test]
fn cpu_tuning_explicit_cpu_spells_march_or_mcpu_by_target_arch() {
    // #6125: an explicit baseline applies to host AND cross builds, and
    // the flag spelling follows the effective target's architecture.
    assert_eq!(
        cpu_tuning_arg_for(Some("x86-64-v2"), None, "x86_64-unknown-linux-gnu").as_deref(),
        Some("-march=x86-64-v2")
    );
    assert_eq!(
        cpu_tuning_arg_for(
            Some("x86-64-v3"),
            Some("x86_64-unknown-linux-musl"),
            "x86_64-unknown-linux-musl"
        )
        .as_deref(),
        Some("-march=x86-64-v3")
    );
    assert_eq!(
        cpu_tuning_arg_for(Some("apple-m1"), None, "arm64-apple-macosx15.0.0").as_deref(),
        Some("-mcpu=apple-m1")
    );
}

#[test]
fn cpu_tuning_generic_disables_native_tuning_on_host_builds() {
    for off in ["generic", "off", "none", "0", "false"] {
        assert_eq!(
            cpu_tuning_arg_for(Some(off), None, "x86_64-apple-darwin"),
            None,
            "'{off}' should disable tuning"
        );
    }
    // Whitespace / empty values fall back to the default.
    assert_eq!(
        cpu_tuning_arg_for(Some("  "), None, "x86_64-apple-darwin").as_deref(),
        Some(native_tuning_arg_for_host())
    );
}

#[test]
fn cpu_tuning_native_can_be_forced_for_explicit_triples() {
    assert_eq!(
        cpu_tuning_arg_for(
            Some("native"),
            Some("x86_64-unknown-linux-gnu"),
            "x86_64-unknown-linux-gnu"
        )
        .as_deref(),
        Some("-march=native")
    );
}

#[test]
fn compile_plan_metadata_json_contains_object_source() {
    let temp = env::temp_dir().join(format!(
        "perry_compile_plan_test_{}_{}.json",
        std::process::id(),
        TEMP_NONCE_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let plan = build_clang_compile_plan(
        PathBuf::from("clang"),
        PathBuf::from("/tmp/input.ll"),
        PathBuf::from("/tmp/output.o"),
        Some("x86_64-unknown-linux-gnu"),
        0,
        0,
        false,
    );
    write_compile_plan_metadata(&plan, &temp).unwrap();
    let text = fs::read_to_string(&temp).unwrap();
    let _ = fs::remove_file(&temp);
    assert!(text.contains("\"clang_path\": \"clang\""));
    assert!(text.contains("\"effective_target\": \"x86_64-unknown-linux-gnu\""));
    assert!(text.contains("\"object_path\": \"/tmp/output.o\""));
    assert!(text.contains("\"stderr_remarks_path\": \"/tmp/output.o.clang-stderr\""));
}

#[test]
fn temp_nonce_counter_is_unique_across_concurrent_calls() {
    // Regression test for #509: two rayon workers calling
    // `compile_ll_to_object` concurrently must NOT generate the same
    // **output** temp-file path. The counter is mixed into the `.o`
    // basename (the `.ll` is content-addressed — see #7131).
    use std::collections::HashSet;
    use std::thread;

    let mut handles = Vec::new();
    for _ in 0..16 {
        handles.push(thread::spawn(|| {
            let mut local: Vec<u64> = Vec::with_capacity(16);
            for _ in 0..16 {
                local.push(TEMP_NONCE_COUNTER.fetch_add(1, Ordering::Relaxed));
            }
            local
        }));
    }
    let mut all: Vec<u64> = Vec::with_capacity(256);
    for h in handles {
        all.extend(h.join().unwrap());
    }
    let unique: HashSet<u64> = all.iter().copied().collect();
    assert_eq!(
        unique.len(),
        all.len(),
        "TEMP_NONCE_COUNTER produced duplicate values: total={}, unique={}",
        all.len(),
        unique.len(),
    );
}

#[test]
fn ll_temp_basename_is_content_addressed_not_clocked() {
    // #7131: two calls with identical IR must produce the same `.ll`
    // basename (so clang embeds a deterministic source path). The `.o`
    // basename still differs via the counter.
    let tmp = env::temp_dir();
    let ir = "define void @f() {\n  ret void\n}\n";
    let (a, _, _) = llvm_temp_paths(&tmp, ir);
    let (b, _, _) = llvm_temp_paths(&tmp, ir);
    let (ll_a, obj_a) = (&a.ll_path, &a.obj_path);
    let (ll_b, obj_b) = (&b.ll_path, &b.obj_path);
    assert_eq!(
        ll_a.file_name(),
        ll_b.file_name(),
        "same IR must share the content-addressed .ll basename"
    );
    assert_ne!(
        obj_a.file_name(),
        obj_b.file_name(),
        ".o basenames must stay unique across calls (#509)"
    );
    // Different IR → different .ll basename.
    let (c, _, _) = llvm_temp_paths(&tmp, "define void @g() {\n  ret void\n}\n");
    assert_ne!(ll_a.file_name(), c.ll_path.file_name());
    // No pid / wall-clock digits of variable width — only hex hash.
    let name = ll_a.file_name().unwrap().to_string_lossy();
    assert!(
        name.starts_with("perry_llvm_") && name.ends_with(".ll"),
        "unexpected .ll name: {name}"
    );
    let hex = name
        .trim_start_matches("perry_llvm_")
        .trim_end_matches(".ll");
    assert_eq!(hex.len(), 16, "hash must be 16 lowercase hex digits: {hex}");
    assert!(
        hex.chars().all(|c| c.is_ascii_hexdigit()),
        "hash must be hex: {hex}"
    );
}

#[test]
fn object_temp_name_is_unique_across_processes_but_ll_is_not() {
    // The regression this test exists for: #7135 content-addressed BOTH
    // temp names, so the `.o` lost the pid it used to carry. Two `perry`
    // processes compiling identical IR then agreed on the object path —
    // and `compile_ll_to_object` deletes the object after reading it, so
    // they deleted each other's. Measured on macOS before the fix: 8 of 12
    // concurrent same-source compiles failed with
    //   Failed to read clang output at …/perry_llvm_<hash>_0.o
    // Both processes start TEMP_NONCE_COUNTER at 0, so the counter cannot
    // separate them; only the pid can.
    let tmp = env::temp_dir();
    let ir = "define void @f() {\n  ret void\n}\n";

    // Same IR, same counter, DIFFERENT process.
    let p1 = llvm_temp_paths_for(&tmp, ir, 1111, 0);
    let p2 = llvm_temp_paths_for(&tmp, ir, 2222, 0);
    let (ll_p1, obj_p1) = (&p1.ll_path, &p1.obj_path);
    let (ll_p2, obj_p2) = (&p2.ll_path, &p2.obj_path);
    assert_eq!(
        ll_p1.file_name(),
        ll_p2.file_name(),
        "the .ll is what clang records into the object; it must stay a pure \
         function of the IR across processes (#7131)"
    );
    assert_ne!(
        obj_p1.file_name(),
        obj_p2.file_name(),
        "two processes with identical IR must NOT share an object path — \
         they delete it out from under each other (#509 across processes)"
    );

    // Same process, different call: the counter still has to separate
    // in-process rayon workers.
    let c0 = llvm_temp_paths_for(&tmp, ir, 1111, 0);
    let c1 = llvm_temp_paths_for(&tmp, ir, 1111, 1);
    assert_ne!(c0.obj_path.file_name(), c1.obj_path.file_name());

    // The atomic-write staging name needs the same separation: both
    // processes reach it with the same hash and the same counter, and
    // `File::create` truncates.
    assert_ne!(
        ll_staging_path(ll_p1, 1111, 0).file_name(),
        ll_staging_path(ll_p1, 2222, 0).file_name(),
        "staging .tmp name must be per-process"
    );
    assert_ne!(
        ll_staging_path(ll_p1, 1111, 0).file_name(),
        ll_staging_path(ll_p1, 1111, 1).file_name(),
        "staging .tmp name must be per-call"
    );

    // …and the staging file must never be mistaken for the real `.ll`.
    assert_ne!(
        ll_staging_path(ll_p1, 1111, 0).file_name(),
        ll_p1.file_name()
    );
}

#[test]
fn ll_content_hash_is_stable_for_fixed_input() {
    // Pin the FNV-1a value so a future hash swap is intentional.
    assert_eq!(ll_content_hash(""), 0xcbf2_9ce4_8422_2325);
    assert_eq!(ll_content_hash("a"), 0xaf63_dc4c_8601_ec8c);
}

#[test]
fn rs4gc_refuses_wineh_funclet_modules_before_the_pass_runs() {
    // #7354: rewrite-statepoints-for-gc crashes (0xC0000005) on WinEH funclet
    // pads — reproduced from an eight-line module with one `invoke` unwinding
    // to a `catchswitch`. The refusal must fire on the funclet instructions...
    let funclets = "\
        pad:\n  %cs = catchswitch within none [label %catch] unwind to caller\n\
        catch:\n  %cp = catchpad within %cs [ptr @filter]\n";
    let refusal = rs4gc_funclet_refusal(funclets).expect("funclet module must be refused");
    assert!(refusal.contains("funclet"), "{refusal}");
    assert!(refusal.contains("#7354"), "{refusal}");
    assert!(
        rs4gc_funclet_refusal("  %cp = cleanuppad within none []\n").is_some(),
        "cleanup funclets take the same crash path"
    );

    // ...and must NOT fire on the Itanium EH shape RS4GC supports, nor on a
    // user string literal that merely names the opcode.
    assert!(rs4gc_funclet_refusal("  %lp = landingpad { ptr, i32 } cleanup\n").is_none());
    assert!(
        rs4gc_funclet_refusal("@str = constant [10 x i8] c\"catchpad!\00\"\n").is_none(),
        "a string literal naming the opcode is not a funclet"
    );
}
