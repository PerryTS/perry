//! #11495: a compile must leave nothing in `$TMPDIR`.
//!
//! `--trace llvm` used to set `PERRY_LLVM_KEEP_IR` on the user's behalf. That
//! parked a `.perry-keep`-marked `perry_llvm_scratch_*` directory (which the
//! stale reaper is forbidden to touch) and a `perry_native_reps_*.json` in the
//! temp root for every module, forever. The gc-root-dominance corpus traces
//! hundreds of compiles per run, so a shared build host filled its disk.
//!
//! The test points `TMPDIR` at an empty directory of its own and asserts it is
//! still empty afterwards — for a plain compile and for a traced one — while
//! the trace directory did receive the `.ll` it promises, so a green run cannot
//! mean "codegen never ran".

use std::path::{Path, PathBuf};
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn leftovers(tmp: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(tmp)
        .expect("read private TMPDIR")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn compile(dir: &Path, tmp: &Path, trace: bool) {
    let mut command = Command::new(perry_bin());
    command
        .current_dir(dir)
        .arg("compile")
        .arg("main.ts")
        .arg("-o")
        .arg(dir.join("main.o"))
        .arg("--no-link")
        .arg("--no-cache")
        .env("TMPDIR", tmp)
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env_remove("PERRY_LLVM_KEEP_IR")
        .env_remove("PERRY_NATIVE_REPS")
        .env_remove("PERRY_NATIVE_REPS_DIR")
        .env_remove("PERRY_SAVE_LL");
    if trace {
        command.arg("--trace").arg("llvm");
    }
    let output = command.output().expect("run perry compile");
    assert!(
        output.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn compiles_leave_nothing_in_tmpdir() {
    let work = tempfile::tempdir().expect("tempdir");
    let tmp = tempfile::tempdir().expect("private TMPDIR");
    std::fs::write(
        work.path().join("main.ts"),
        "class Box { constructor(public v: number) {} get(): number { return this.v; } }\n\
         const xs: number[] = [];\n\
         for (let i = 0; i < 8; i++) xs.push(new Box(i).get());\n\
         console.log(xs.join(','));\n",
    )
    .expect("write main.ts");

    compile(work.path(), tmp.path(), false);
    assert_eq!(
        leftovers(tmp.path()),
        Vec::<String>::new(),
        "a plain compile left files in TMPDIR"
    );

    compile(work.path(), tmp.path(), true);
    let traced = work.path().join(".perry-trace/llvm");
    let ll_count = std::fs::read_dir(&traced)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|e| e.path().extension().is_some_and(|x| x == "ll"))
                .count()
        })
        .unwrap_or(0);
    assert!(
        ll_count > 0,
        "--trace llvm wrote no .ll into {}; the TMPDIR check below would be vacuous",
        traced.display()
    );
    assert_eq!(
        leftovers(tmp.path()),
        Vec::<String>::new(),
        "--trace llvm left files in TMPDIR"
    );
}
