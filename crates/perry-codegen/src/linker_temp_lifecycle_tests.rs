//! Temp-file lifecycle for the clang driver (#7144), and the path-shape
//! properties it has to hold constant (#7131, #7140, #509).
//!
//! Split out of `linker.rs` for the 2,000-line file cap, not because it is a
//! different subject: everything here is about the two temp files one
//! `.ll` → `.o` compile creates, who may see them, and when they are removed.
//!
//! The end-to-end tests drive the real `clang` into a temp root of their own
//! and assert on what is left in that root. That is the only way a leak is
//! observable — every path-shape test in this file passes just as happily
//! against a compiler that never deletes anything, which is exactly how #7144
//! shipped inside a green #7135.

use super::*;

#[test]
fn scratch_dir_is_per_call_and_per_process_but_the_ll_basename_is_not() {
    // #7144's shape, and the reason it does not undo #7131: the *directory*
    // carries every uniquifier, the *basename* carries none. clang records
    // the basename into the object and nothing else (no `-g`), so the two
    // properties do not compete — a call can own its `.ll` outright and
    // still emit the same object bytes as any other call with the same IR.
    let tmp = Path::new("/tmp");
    let ir = "define void @f() {\n  ret void\n}\n";

    let a = llvm_temp_paths_for(tmp, ir, 1111, 0, LlLayout::Scratch);
    let b = llvm_temp_paths_for(tmp, ir, 1111, 1, LlLayout::Scratch); // same process
    let c = llvm_temp_paths_for(tmp, ir, 2222, 0, LlLayout::Scratch); // other process

    let dirs = [&a, &b, &c].map(|p| {
        p.scratch_dir
            .clone()
            .expect("Scratch layout must allocate a private directory")
    });
    assert_ne!(dirs[0], dirs[1], "two calls must not share a directory");
    assert_ne!(dirs[0], dirs[2], "two processes must not share a directory");

    for p in [&a, &b, &c] {
        let dir = p.scratch_dir.as_ref().unwrap();
        assert_eq!(
            p.ll_path.parent(),
            Some(dir.as_path()),
            "the .ll must live inside the directory that gets removed"
        );
        assert_eq!(
            p.obj_path.parent(),
            Some(dir.as_path()),
            "the .o must go with it, so one remove_dir_all cleans up"
        );
        assert_eq!(
            p.ll_path.file_name(),
            a.ll_path.file_name(),
            "the recorded name — the basename — must stay content-only (#7131)"
        );
    }
}

#[test]
fn debug_symbols_keep_a_stable_absolute_ll_path() {
    // Under `-g` the object names the `.ll` by ABSOLUTE path in DWARF, so
    // the whole path has to be a function of the IR (else `-g` builds stop
    // being reproducible for a fixed TMPDIR) and the file has to outlive the
    // compile (else the debugger has nothing to open). Both follow from
    // "no scratch directory": #7144 exempts this layout on purpose.
    let tmp = Path::new("/tmp");
    let ir = "define void @f() {\n  ret void\n}\n";

    let a = llvm_temp_paths_for(tmp, ir, 1111, 0, LlLayout::DebugShared);
    let b = llvm_temp_paths_for(tmp, ir, 2222, 7, LlLayout::DebugShared);
    assert!(
        a.scratch_dir.is_none() && b.scratch_dir.is_none(),
        "a per-call directory would put pid/counter into DWARF"
    );
    assert_eq!(
        a.ll_path, b.ll_path,
        "the DWARF-referenced path must be content-addressed end to end"
    );
    assert_eq!(a.ll_path.parent(), Some(tmp));
    // The object still may not collide across processes (#7140/#509).
    assert_ne!(a.obj_path.file_name(), b.obj_path.file_name());
}

#[test]
fn debug_symbols_layout_and_g_flag_agree() {
    // The hazard this pins: if the layout said "scratch, delete it" while
    // clang was still passed `-g`, every debug build would ship DWARF
    // pointing at a file that no longer exists — and nothing would fail
    // loudly. One `TempFilePolicy` decides both; this is that contract.
    let with_g = TempFilePolicy {
        keep: false,
        debug_symbols: true,
    };
    let without = TempFilePolicy {
        keep: false,
        debug_symbols: false,
    };
    assert_eq!(with_g.layout(), LlLayout::DebugShared);
    assert_eq!(without.layout(), LlLayout::Scratch);

    for policy in [with_g, without] {
        let plan = build_clang_compile_plan(
            PathBuf::from("clang"),
            PathBuf::from("/tmp/input.ll"),
            PathBuf::from("/tmp/output.o"),
            None,
            0,
            0,
            policy.debug_symbols,
        );
        let has_g = plan.clang_args.iter().any(|a| a == "-g");
        assert_eq!(
            has_g,
            policy.layout() == LlLayout::DebugShared,
            "`-g` is passed iff the `.ll` is retained at a stable path: {policy:?}"
        );
    }
}

// ── Temp-file lifecycle (#7144) ────────────────────────────────────────
//
// These drive the real `clang`, into a temp root of their own, and assert
// on what is left in that root. A leak is only observable end-to-end: every
// path-shape test above passes just as happily against a compiler that
// never deletes anything, which is exactly how #7144 shipped.

/// A fresh, empty directory to use as the temp root, or `None` when this
/// host has no usable clang and the compile step cannot run at all.
fn temp_root_if_clang_available(tag: &str) -> Option<PathBuf> {
    let Some(clang) = find_clang() else {
        eprintln!("[linker tests] skipping {tag}: no clang on this host");
        return None;
    };
    if let Err(err) = ensure_supported_clang(&clang) {
        eprintln!("[linker tests] skipping {tag}: unusable clang ({err:#})");
        return None;
    }
    let root = env::temp_dir().join(format!(
        "perry_linker_test_{tag}_{}_{:x}",
        std::process::id(),
        TEMP_NONCE_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("failed to create test temp root");
    Some(root)
}

fn entries(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(root)
        .expect("temp root vanished")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn test_ir(nth: u32) -> String {
    format!("\ndefine i32 @perry_temp_lifecycle_{nth}() {{\nentry:\n  ret i32 {nth}\n}}\n")
}

const CLEAN: TempFilePolicy = TempFilePolicy {
    keep: false,
    debug_symbols: false,
};

#[test]
fn successful_compile_leaves_nothing_behind() {
    // THE #7144 regression test. Before the fix each compile left one
    // `perry_llvm_<hash>.ll` in the temp dir forever — bounded by distinct
    // IR ever compiled, which in practice means every rebuild of the
    // compiler. 1627 files / 951.8 MB on one dev box after a day.
    let Some(root) = temp_root_if_clang_available("clean") else {
        return;
    };

    for nth in 0..3 {
        let bytes = compile_ll_to_object_in(&root, &test_ir(nth), None, CLEAN)
            .unwrap_or_else(|e| panic!("compile {nth} failed: {e:#}"));
        assert!(!bytes.is_empty(), "compile {nth} produced no object bytes");
        assert_eq!(
            entries(&root),
            Vec::<String>::new(),
            "compile {nth} left temp files behind (#7144)"
        );
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn concurrent_compiles_of_identical_ir_both_succeed_and_leave_nothing() {
    // The race that made #7135 stop deleting: two workers holding the SAME
    // IR agree on the content-addressed name, so one could unlink it in the
    // window between the other computing the path and clang opening it.
    //
    // What this test does and does not prove, stated because a race test that
    // is trusted for more than it shows is worse than none. It cannot *decide*
    // the race: sabotaged to the naive shape (one shared flat `.ll`, unlinked
    // after use) it went red in one full-suite run and green in the next three
    // — which is the definition of the window being narrow, not absent. The
    // guarantee comes from `scratch_dir_is_per_call_and_per_process_…`: no two
    // calls are ever handed the same `.ll` path, so there is nothing to race
    // over and no window to lose. This test is the end-to-end complement —
    // under real concurrency, 8 identical-IR compiles must all succeed, emit
    // the same bytes, and leave nothing — and it will occasionally catch a
    // regression the structural test somehow passed.
    use std::thread;

    let Some(root) = temp_root_if_clang_available("race") else {
        return;
    };
    let ir = test_ir(42);

    let results: Vec<Result<Vec<u8>>> = thread::scope(|s| {
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let root = root.clone();
                let ir = ir.clone();
                s.spawn(move || compile_ll_to_object_in(&root, &ir, None, CLEAN))
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });

    let mut first: Option<Vec<u8>> = None;
    for (i, r) in results.into_iter().enumerate() {
        let bytes = r.unwrap_or_else(|e| panic!("concurrent compile {i} failed: {e:#}"));
        match &first {
            // Identical IR must still give identical objects — the sharing
            // this fix removed was never what made emission deterministic
            // (#7131); the content-addressed basename is (#7140).
            Some(expected) => assert_eq!(&bytes, expected, "compile {i} emitted other bytes"),
            None => first = Some(bytes),
        }
    }
    assert_eq!(
        entries(&root),
        Vec::<String>::new(),
        "8 concurrent identical-IR compiles left temp files behind"
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn failed_compile_keeps_the_ll_for_diagnosis() {
    // Stated policy: failures retain their IR. The error message names the
    // file, and a compile that just failed is precisely when someone wants
    // to read the IR that produced it.
    let Some(root) = temp_root_if_clang_available("failure") else {
        return;
    };

    let err = compile_ll_to_object_in(&root, "this is not LLVM IR\n", None, CLEAN)
        .expect_err("clang must reject non-IR input");
    let message = format!("{err:#}");
    assert!(
        message.contains("LLVM IR left at:"),
        "the failure must say where the IR is; got: {message}"
    );

    // Asserted on the whole tree rather than on the scratch directory: the
    // claim is "the IR survives a failed compile", and it has to keep meaning
    // that if the layout is ever rearranged again.
    let surviving = ll_files_under(&root);
    assert_eq!(
        surviving.len(),
        1,
        "exactly the failed compile's .ll must survive, found: {surviving:?}"
    );
    assert!(
        message.contains(&surviving[0].display().to_string()),
        "the failure must name the file it left: {message}"
    );
    let _ = fs::remove_dir_all(&root);
}

/// Every `.ll` anywhere under `root`, so a lifetime assertion does not have to
/// know which layout produced the file.
fn ll_files_under(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(read) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "ll") {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

#[test]
fn keep_ir_retains_the_whole_scratch_dir() {
    let Some(root) = temp_root_if_clang_available("keep") else {
        return;
    };
    let policy = TempFilePolicy {
        keep: true,
        debug_symbols: false,
    };
    compile_ll_to_object_in(&root, &test_ir(7), None, policy).expect("compile failed");

    let left = entries(&root);
    assert_eq!(left.len(), 1, "expected one kept scratch dir: {left:?}");
    let kept = entries(&root.join(&left[0]));
    for want in [".ll", ".o", ".compile-plan.json"] {
        assert!(
            kept.iter().any(|n| n.ends_with(want)),
            "PERRY_LLVM_KEEP_IR must retain the {want}: {kept:?}"
        );
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn debug_symbols_retain_the_ll_at_the_path_dwarf_names() {
    // (b) of #7144, as executable policy rather than a sentence in a doc:
    // `-g` puts the `.ll`'s absolute path into DWARF, so this layout keeps
    // the file — flat in the temp root, content-addressed, at exactly the
    // path the object points at. The `.o` is still cleaned up.
    let Some(root) = temp_root_if_clang_available("debug") else {
        return;
    };
    let policy = TempFilePolicy {
        keep: false,
        debug_symbols: true,
    };
    let ir = test_ir(9);
    compile_ll_to_object_in(&root, &ir, None, policy).expect("compile failed");

    let left = entries(&root);
    let expected = llvm_temp_paths_for(&root, &ir, 0, 0, LlLayout::DebugShared);
    let expected_name = expected.ll_path.file_name().unwrap().to_string_lossy();
    assert_eq!(
        left,
        vec![expected_name.to_string()],
        "a -g build must retain exactly the .ll its DWARF references"
    );

    // …and re-running must not accumulate a second copy: the name is a
    // function of the IR, so a `-g` temp dir is bounded by distinct IR
    // compiled with `-g`, not by the number of compiles.
    compile_ll_to_object_in(&root, &ir, None, policy).expect("second compile failed");
    assert_eq!(entries(&root), left);
    let _ = fs::remove_dir_all(&root);
}
