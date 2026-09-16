//! Regression coverage for the allocation-free "is `next` still the builtin?"
//! proof in `object/iterator_prototypes.rs`.
//!
//! The probe that lets a user replacement of `%ArrayIteratorPrototype%.next`
//! (and the Map / Set / String family prototypes) drive `for…of`, spread,
//! `Array.from` and manual `.next()` used to end in a by-name prototype lookup
//! that minted a fresh `"next"` key string on EVERY built-in iterator step.
//! The proof that removed it reads the prototype's own `next` slot and the
//! per-key accessor Bloom bit instead — so every way of *defeating* that proof
//! has to keep working, and every way of *restoring* it has to hand iteration
//! back to the builtin.
//!
//! The expected output below is `node v26.5.1` running the same source
//! (`test-files/test_gap_iterator_prototype_next_patch.ts`), captured
//! 2026-09-06. The discriminating lines are:
//!
//! * `A-spread 8,10` / `A-from-array 22,24` / `A-call-spread 26,28` /
//!   `A-multi-spread 30,32` / `D-set s1,s2` / `D-map-spread [["z",105]]` /
//!   `E-string A,B` — every one of these is a runtime element-COPY fast arm
//!   that materializes the result without ever calling `.next()`, so the
//!   per-call proof above cannot see the patch from inside them. They decline
//!   on the #10086 escape signal instead (the family prototype object reached
//!   user code through `Object.getPrototypeOf`), which is the only event that
//!   can precede a patch. Without that the fixture printed the raw elements —
//!   `A-spread 4,5`, `D-set 1,2`, `E-string a,b`.
//! * `F-bound-copy 100,200` — `orig.bind(other)` has the SAME native entry as
//!   the builtin thunk but a different `this`. A proof that compared by native
//!   entry alone, without first reading the prototype's own slot, would call
//!   the builtin and print `1,2`.
//! * `G-accessor … true` — an accessor `next` installed by `defineProperty`
//!   leaves the old closure in the data slot, so the own-slot read alone still
//!   sees the canonical closure. Only the accessor Bloom bit makes the proof
//!   decline; without it the getter is silently bypassed and `gets` stays 0.
//! * `H true` — a deleted `next` must throw a TypeError, not fall through to
//!   the builtin advance.
//! * `I <type> true` x5 — a non-callable prototype `next` (a number, a string,
//!   `undefined`, `null`, a plain object) must throw a TypeError. The proof
//!   reads the own slot as a RAW value, so each of these has to defeat it: the
//!   number and the string never reach `is_pointer`/`get_valid_func_ptr` as a
//!   closure, and the plain object passes `is_pointer` but fails the
//!   CLOSURE_MAGIC probe inside `get_valid_func_ptr`.

use std::path::PathBuf;
use std::process::Command;

const SOURCE: &str = include_str!("../../../test-files/test_gap_iterator_prototype_next_patch.ts");

const EXPECTED: &str = "A-forof 2,4,6\n\
A-spread 8,10\n\
A-from 12\n\
A-from-array 22,24\n\
A-call-spread 26,28\n\
A-multi-spread 30,32\n\
A-manual 14 16 true\n\
B-forof 1,2,3\n\
B-spread 4,5\n\
B-manual 7 8 true\n\
C-forof-empty 0\n\
C-restored 9\n\
D-map a=101,b=102\n\
D-map-spread [[\"z\",105]]\n\
D-map-from [[\"y\",106]]\n\
D-map-restored a,1\n\
D-set s1,s2\n\
D-set-from s3,s4\n\
D-set-restored 3\n\
E-string A,B\n\
E-string-from C,D\n\
E-string-restored c,d\n\
F-same-object 1,2\n\
F-bound-copy 100,200\n\
F-restored 3\n\
G-accessor 1,2 true\n\
G-restored 4\n\
H true\n\
H-restored 5,6\n\
I number true\n\
I string true\n\
I undefined true\n\
I object true\n\
I object true\n\
I-restored 7,8\n";

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn patched_iterator_prototype_next_drives_every_iteration_form() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("iterator_next_patch.ts");
    let output = dir.path().join("iterator_next_patch_bin");
    std::fs::write(&entry, SOURCE).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output)
        .current_dir(dir.path())
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        EXPECTED,
        "output must match node v26.5.1\nstderr:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
}
