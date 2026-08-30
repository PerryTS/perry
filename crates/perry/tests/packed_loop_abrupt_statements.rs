//! Packed-array counted loops keep their fast path when the body contains an
//! unlabeled `break`, `continue` or `return` (#9151).
//!
//! Every program here ACTUALLY takes its exit, at varying indices and with
//! observable effects before it. That matters: a never-taken exit exercises
//! only the admission decision, not the emitted clone, and the first cut of
//! this change was wrong in a way only a taken exit could reveal — a labeled
//! `break` must unwind past the loop being analysed, which the fast clone does
//! not do, so `labeled_break_still_unwinds_the_outer_loop` below returned 4032
//! instead of 4087 and silently dropped a partial iteration.

use std::path::PathBuf;
use std::process::{Command, Output};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(source: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .env("PERRY_NO_CACHE", "1")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run: Output = Command::new(&output)
        .current_dir(dir.path())
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).trim().to_owned()
}

const PRELUDE: &str = r#"
const arr: number[] = [];
for (let i = 0; i < 64; i++) arr.push(i);
"#;

#[test]
fn break_exits_at_the_right_index() {
    let out = compile_and_run(&format!(
        "{PRELUDE}
        function breakAt(k: number): number {{
            let s = 0;
            for (let i = 0; i < arr.length; i++) {{ s += arr[i]; if (arr[i] === k) break; }}
            return s;
        }}
        console.log(breakAt(0) + \" \" + breakAt(1) + \" \" + breakAt(31) + \" \" + breakAt(63) + \" \" + breakAt(999));
        "
    ));
    assert_eq!(out, "0 1 496 2016 2016");
}

#[test]
fn continue_skips_exactly_the_guarded_iterations() {
    let out = compile_and_run(&format!(
        "{PRELUDE}
        let s = 0;
        for (let i = 0; i < arr.length; i++) {{ if (arr[i] % 2 === 1) continue; s += arr[i]; }}
        console.log(s);
        "
    ));
    assert_eq!(out, "992");
}

#[test]
fn return_leaves_the_function_at_the_right_index() {
    let out = compile_and_run(&format!(
        "{PRELUDE}
        function returnAt(k: number): number {{
            for (let i = 0; i < arr.length; i++) {{ if (arr[i] === k) return i; }}
            return -1;
        }}
        console.log(returnAt(0) + \" \" + returnAt(17) + \" \" + returnAt(63) + \" \" + returnAt(999));
        "
    ));
    assert_eq!(out, "0 17 63 -1");
}

#[test]
fn effects_before_a_break_happen_exactly_once() {
    // A clone that replayed the exiting iteration would double an entry here.
    let out = compile_and_run(&format!(
        "{PRELUDE}
        function effectsThenBreak(k: number): string {{
            let out = \"\";
            for (let i = 0; i < arr.length; i++) {{ out += arr[i] + \",\"; if (arr[i] === k) break; }}
            return out;
        }}
        console.log(effectsThenBreak(3) + \" \" + effectsThenBreak(0));
        "
    ));
    assert_eq!(out, "0,1,2,3, 0,");
}

#[test]
fn labeled_break_still_unwinds_the_outer_loop() {
    // The regression that gated this change. A labeled break targets an
    // enclosing loop, so it must unwind past the analysed one; admitting it
    // into the fast clone dropped the whole partial iteration (4032 vs 4087).
    let out = compile_and_run(&format!(
        "{PRELUDE}
        let s = 0;
        outer: for (let r = 0; r < 4; r++) {{
            for (let i = 0; i < arr.length; i++) {{ s += arr[i]; if (arr[i] === 10 && r === 2) break outer; }}
        }}
        console.log(s);
        "
    ));
    assert_eq!(out, "4087");
}

#[test]
fn break_in_a_loop_whose_array_grows() {
    let out = compile_and_run(
        r#"
        const a = [1, 2, 3];
        let s = 0;
        for (let i = 0; i < a.length; i++) { s += a[i]; if (a.length < 6) a.push(9); if (i === 4) break; }
        console.log(s + ":" + a.length);
        "#,
    );
    assert_eq!(out, "24:6");
}

#[test]
fn a_throw_that_does_not_construct_takes_the_fast_path_correctly() {
    // A `throw` whose value is already built is admitted: its block ends in
    // `unreachable`, so control never returns to the loop and nothing reads
    // what the clone cached. A throw that CONSTRUCTS its value is not, because
    // the construction is emitted in blocks preceding the terminating one.
    let out = compile_and_run(&format!(
        "{PRELUDE}
        const PRE = new Error(\"boom\");
        function throwPre(k: number): string {{
            try {{
                let s = 0;
                for (let i = 0; i < arr.length; i++) {{ s += arr[i]; if (arr[i] === k) throw PRE; }}
                return \"none\" + s;
            }} catch (e) {{ return (e as Error).message + \":\" + k; }}
        }}
        function throwValue(k: number): string {{
            try {{
                let s = 0;
                for (let i = 0; i < arr.length; i++) {{ s += arr[i]; if (arr[i] === k) throw s; }}
                return \"none\" + s;
            }} catch (e) {{ return \"v\" + String(e); }}
        }}
        console.log(throwPre(4) + \" \" + throwPre(999) + \" \" + throwValue(0) + \" \" + throwValue(7));
        "
    ));
    assert_eq!(out, "boom:4 none2016 v0 v28");
}

#[test]
fn throw_inside_the_loop_is_still_correct() {
    // A throw that CONSTRUCTS its value is not admitted (the construction is a
    // call in a block that does not end in `unreachable`), but it must keep
    // working on the generic path.
    let out = compile_and_run(&format!(
        "{PRELUDE}
        function throwAt(k: number): string {{
            try {{
                let s = 0;
                for (let i = 0; i < arr.length; i++) {{ s += arr[i]; if (arr[i] === k) throw new Error(\"hit\" + s); }}
                return \"none\" + s;
            }} catch (e) {{ return String((e as Error).message); }}
        }}
        console.log(throwAt(5) + \" \" + throwAt(999));
        "
    ));
    assert_eq!(out, "hit15 none2016");
}
