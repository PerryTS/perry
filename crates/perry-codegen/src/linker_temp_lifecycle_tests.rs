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

// ── Path shape ─────────────────────────────────────────────────────────────

#[test]
fn scratch_dir_is_per_call_and_per_process_but_the_ll_basename_is_not() {
    // #7144's shape, and the reason it does not undo #7131: the *directory*
    // carries every uniquifier, the *basename* carries none. The object records
    // the basename and nothing else, so the two properties do not compete — a
    // call can own its `.ll` outright and still emit the same object bytes as
    // any other call with the same IR.
    //
    // This is the structural guarantee the whole fix rests on: no two calls are
    // ever handed the same `.ll` path, so the unlink has nothing to race.
    let tmp = Path::new("/tmp");
    let ir = "define void @f() {\n  ret void\n}\n";

    let a = llvm_temp_paths_for(tmp, ir, 1111, 0);
    let b = llvm_temp_paths_for(tmp, ir, 1111, 1); // same process, next call
    let c = llvm_temp_paths_for(tmp, ir, 2222, 0); // other process, same counter

    assert_ne!(
        a.scratch_dir, b.scratch_dir,
        "two calls must not share a directory"
    );
    assert_ne!(
        a.scratch_dir, c.scratch_dir,
        "two processes must not share a directory — every process starts the \
         counter at 0, so only the pid can separate them (#7140)"
    );

    for p in [&a, &b, &c] {
        assert_eq!(
            p.ll_path.parent(),
            Some(p.scratch_dir.as_path()),
            "the .ll must live inside the directory that gets removed"
        );
        assert_eq!(
            p.obj_path.parent(),
            Some(p.scratch_dir.as_path()),
            "the .o must go with it, so one remove_dir_all cleans up"
        );
        assert_eq!(
            p.ll_path.file_name(),
            a.ll_path.file_name(),
            "the recorded name — the basename — must stay content-only (#7131)"
        );
    }
    // …and the objects must still not collide, directory or no directory.
    assert_ne!(a.obj_path.file_name(), c.obj_path.file_name());
}

// ── Temp-file lifecycle (#7144) ────────────────────────────────────────────
//
// These drive the real `clang`, into a temp root of their own, and assert on
// what is left in that root. A leak is only observable end-to-end: every
// path-shape test above passes just as happily against a compiler that never
// deletes anything, which is exactly how #7144 shipped.

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
fn debug_symbols_do_not_change_the_temp_file_lifetime() {
    // #7144 question (b), answered by measurement rather than by inheriting the
    // premise. `PERRY_DEBUG_SYMBOLS` was believed to make the `.ll` part of the
    // shipped object — "`-g` pulls the absolute `.ll` path plus `DW_AT_comp_dir`
    // into DWARF" — which would have required keeping the file at a stable path
    // for as long as the object lived. It does not: see
    // `debug_symbols_do_not_change_what_the_object_records` below and the note
    // on `TEMP_NONCE_COUNTER`. So `-g` cleans up like everything else, and there
    // is no second layout left sitting untested.
    let Some(root) = temp_root_if_clang_available("debug") else {
        return;
    };
    let policy = TempFilePolicy {
        keep: false,
        debug_symbols: true,
    };
    for nth in 0..2 {
        compile_ll_to_object_in(&root, &test_ir(9 + nth), None, policy)
            .unwrap_or_else(|e| panic!("-g compile {nth} failed: {e:#}"));
        assert_eq!(
            entries(&root),
            Vec::<String>::new(),
            "a -g compile must leave nothing behind either"
        );
    }
    let _ = fs::remove_dir_all(&root);
}

// ── What the object actually records about the `.ll` ───────────────────────

/// The property the whole design rests on, as a test rather than a comment.
///
/// Everything above is safe *because* an emitted object records the `.ll`'s
/// **basename** and never its **directory** — that is what lets a per-call
/// directory coexist with byte-identical emission (#7131). Until now that was
/// measured by hand, once, on a Raspberry Pi (#7140), and written down. A
/// property this load-bearing that no test can restate is one that quietly
/// stops being true.
///
/// ELF is the format that records the basename at all (Mach-O does not, which
/// is why this defect class is invisible on the machine most of this project's
/// work happens on), so this cross-compiles: the embedding is a property of the
/// ELF writer, not of the host or the arch.
#[test]
fn the_ll_directory_is_not_recorded_in_the_object_but_the_basename_is() {
    let Some(root) = temp_root_if_clang_available("elf-record") else {
        return;
    };
    let clang = find_clang().unwrap();
    let ir = test_ir(1);

    for target in ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"] {
        // Same basename, two different directories.
        let mut objs = Vec::new();
        for dir_name in ["dirA", "dirB"] {
            let dir = root.join(format!("{target}-{dir_name}"));
            fs::create_dir_all(&dir).unwrap();
            let ll = dir.join("perry_llvm_00000000cafe0001.ll");
            fs::write(&ll, &ir).unwrap();
            let Some(obj) = elf_compile(&clang, &ll, target, &dir) else {
                eprintln!("[linker tests] skipping {target}: clang cannot target it");
                return;
            };
            objs.push(obj);
        }
        assert_eq!(
            objs[0], objs[1],
            "{target}: the .ll's DIRECTORY must not reach the object — a \
             per-call scratch dir would otherwise break emission determinism"
        );

        // Control: the instrument must be able to see a difference at all. If
        // this ever stops differing, the assertion above proves nothing.
        let dir = root.join(format!("{target}-control"));
        fs::create_dir_all(&dir).unwrap();
        let other = dir.join("perry_llvm_00000000cafe0002.ll");
        fs::write(&other, &ir).unwrap();
        let control = elf_compile(&clang, &other, target, &dir).unwrap();
        assert_ne!(
            objs[0], control,
            "{target}: a different .ll BASENAME must change the object — if it \
             does not, this test cannot detect the directory leaking either"
        );
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn debug_symbols_do_not_change_what_the_object_records() {
    // The measurement that retired the `PERRY_DEBUG_SYMBOLS` exemption. Perry's
    // codegen emits no `DICompileUnit`/`DIFile`/`!dbg` metadata, and `clang -g`
    // on a `.ll` lowers debug info that is *in* the IR rather than synthesising
    // a compile unit for the input file — so `-g` adds no DWARF, and in
    // particular no record of where the `.ll` was. If Perry ever does emit debug
    // metadata this goes red, and the temp-file lifetime has to be revisited.
    let Some(root) = temp_root_if_clang_available("elf-debug") else {
        return;
    };
    let clang = find_clang().unwrap();
    let target = "x86_64-unknown-linux-gnu";

    let dir = root.join("g");
    fs::create_dir_all(&dir).unwrap();
    let ll = dir.join("perry_llvm_00000000cafe0003.ll");
    fs::write(&ll, test_ir(2)).unwrap();

    let Some(plain) = elf_compile(&clang, &ll, target, &dir) else {
        eprintln!("[linker tests] skipping: clang cannot target {target}");
        return;
    };
    let with_g = elf_compile_with(&clang, &ll, target, &dir, &["-g"]).unwrap();
    assert_eq!(
        plain, with_g,
        "-g changed the emitted object. Perry's IR has gained debug metadata, \
         or this clang synthesises a compile unit for .ll input. Either way the \
         `.ll` may now be referenced by the object and #7144's decision to \
         delete it unconditionally has to be re-taken."
    );

    // Said directly as well as comparatively: no DWARF is emitted at all, so
    // there is nowhere for a path to have been recorded. An equality assertion
    // can be defeated by an edit to itself; "this object contains no `.debug_`
    // section name" is a claim about the artifact.
    assert!(
        !contains(&with_g, b".debug_"),
        "the -g object has .debug_ sections — DWARF is being emitted now, and \
         `DW_AT_comp_dir` / `DW_AT_name` may point at the temp `.ll`"
    );

    // Control: this comparison must be capable of failing. `-O0` is a flag that
    // definitely changes the bytes; if even that compares equal, the harness is
    // not really building two objects.
    let control = elf_compile_with(&clang, &ll, target, &dir, &["-O0"]).unwrap();
    assert_ne!(
        plain, control,
        "the object comparison cannot distinguish two different compiles, so \
         the -g assertion above proves nothing"
    );
    let _ = fs::remove_dir_all(&root);
}

fn elf_compile(clang: &Path, ll: &Path, target: &str, cwd: &Path) -> Option<Vec<u8>> {
    elf_compile_with(clang, ll, target, cwd, &[])
}

/// Substring search over raw object bytes — enough to spot an ELF section name.
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// `clang -c` for an explicit target, returning the object bytes. `None` when
/// this clang has no backend for `target` (then the caller must skip, not pass).
fn elf_compile_with(
    clang: &Path,
    ll: &Path,
    target: &str,
    cwd: &Path,
    extra: &[&str],
) -> Option<Vec<u8>> {
    let obj = cwd.join(format!("out{}.o", extra.join("")));
    let run = Command::new(clang)
        .current_dir(cwd)
        .args(["-c", "-O3"])
        .args(extra)
        .arg("-target")
        .arg(target)
        .arg(ll)
        .arg("-o")
        .arg(&obj)
        .output()
        .ok()?;
    if !run.status.success() {
        return None;
    }
    fs::read(&obj).ok()
}
