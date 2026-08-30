//! Coverage for the loop-invariant property-array hoist.
//!
//! `for (let i = 0; i < holder.arr.length; i++) … holder.arr[i] …` reads the
//! property once into a local instead of on every iteration. The rewrite is
//! only sound where the property is provably a data field, the receiver cannot
//! be rebound, and nothing in the loop can write the property — so most of the
//! cases below exist to pin the *refusals*, which are the part a regression
//! would silently break. `PERRY_LOOP_PROPERTY_HOIST=0` restores the old
//! lowering, and every program here must produce identical output either way.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile(dir: &Path, source: &str, hoist: bool) -> PathBuf {
    let entry = dir.join("main.ts");
    let output = dir.join(if hoist { "main_on" } else { "main_off" });
    std::fs::write(&entry, source).expect("write entry");

    let mut cmd = Command::new(perry_bin());
    cmd.current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .env("PERRY_NO_CACHE", "1");
    if !hoist {
        cmd.env("PERRY_LOOP_PROPERTY_HOIST", "0");
    }
    let compile = cmd.output().expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed (hoist={hoist})\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    output
}

fn run(bin: &Path, dir: &Path) -> Output {
    Command::new(bin)
        .current_dir(dir)
        // The hoisted value is an ordinary local, so it is GC-tracked like any
        // other binding; forcing evacuation proves that rather than assuming it.
        .env("PERRY_GC_FORCE_EVACUATE", "1")
        .env("PERRY_GC_VERIFY_EVACUATION", "1")
        .output()
        .expect("run compiled binary")
}

fn stdout_of(bin: &Path, dir: &Path) -> String {
    let out = run(bin, dir);
    assert!(
        out.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Compiles `source` twice and asserts both builds print `expected`. Running
/// the kill-switch build against the same expectation is what makes this a
/// test of equivalence and not merely of the hoisted path.
fn assert_same_with_and_without_hoist(name: &str, source: &str, expected: &str) {
    let dir = tempfile::tempdir().expect("tempdir");
    let on = compile(dir.path(), source, true);
    let off = compile(dir.path(), source, false);
    let got_on = stdout_of(&on, dir.path());
    let got_off = stdout_of(&off, dir.path());
    assert_eq!(got_on.trim(), expected.trim(), "{name}: hoisted output");
    assert_eq!(
        got_off.trim(),
        expected.trim(),
        "{name}: kill-switch output"
    );
}

#[test]
fn hoists_a_loop_invariant_property_array() {
    assert_same_with_and_without_hoist(
        "basic",
        r#"
        const holder = { arr: [1, 2, 3, 4], n: 4 };
        let s = 0;
        for (let i = 0; i < holder.arr.length; i++) s += holder.arr[i];
        console.log(s);
        "#,
        "10",
    );
}

#[test]
fn generated_hoist_name_does_not_shadow_a_source_binding() {
    assert_same_with_and_without_hoist(
        "generated name collision",
        r#"
        const __perry_hoist_arr = 99;
        const holder = { arr: [1, 2, 3] };
        let sum = 0;
        for (let i = 0; i < holder.arr.length; i++) sum += holder.arr[i];
        console.log(__perry_hoist_arr + " " + sum);
        "#,
        "99 6",
    );
}

#[test]
fn refuses_when_the_receiver_is_rebound_in_the_loop() {
    // Both objects share a shape, so no runtime shape check could catch this:
    // the rewrite has to refuse it syntactically. A stale array would keep
    // reading a1 and print 60 instead of 510.
    assert_same_with_and_without_hoist(
        "receiver rebound",
        r#"
        const a1 = { arr: [10, 20, 30], n: 3 };
        const a2 = { arr: [100, 200, 300], n: 3 };
        let h = a1;
        let t = 0;
        for (let i = 0; i < h.arr.length; i++) { t += h.arr[i]; h = a2; }
        console.log(t);
        "#,
        "510",
    );
}

#[test]
fn refuses_when_a_call_can_overwrite_the_property() {
    assert_same_with_and_without_hoist(
        "call overwrite",
        r#"
        const o: any = { arr: [1, 2, 3] };
        function swap(): number { o.arr = [9, 9, 9]; return 0; }
        let u = 0;
        for (let i = 0; i < o.arr.length; i++) { u += o.arr[i] + swap(); }
        console.log(u);
        "#,
        "19",
    );
}

#[test]
fn refuses_a_getter_receiver_and_preserves_invocation_count() {
    // A getter runs user code, so collapsing the reads is observable. The
    // count matters as much as the sum.
    assert_same_with_and_without_hoist(
        "getter",
        r#"
        let reads = 0;
        const g = { get arr() { reads++; return [5, 6]; } };
        let v = 0;
        for (let i = 0; i < g.arr.length; i++) v += g.arr[i];
        console.log(v + " " + reads);
        "#,
        "11 5",
    );
}

#[test]
fn refuses_when_the_property_is_written_directly_in_the_loop() {
    assert_same_with_and_without_hoist(
        "direct write",
        r#"
        const o: any = { arr: [1, 2, 3, 4] };
        let s = 0;
        for (let i = 0; i < o.arr.length; i++) {
            s += o.arr[i];
            if (i === 1) o.arr = [7, 7];
        }
        console.log(s);
        "#,
        "3",
    );
}

#[test]
fn tracks_an_array_grown_during_iteration() {
    // `length` is re-read every iteration by the condition; only the property
    // lookup is hoisted, and the array object itself is shared, so pushes must
    // still be observed.
    assert_same_with_and_without_hoist(
        "grown mid-loop",
        r#"
        const o = { arr: [1, 2, 3] };
        let s = 0;
        for (let i = 0; i < o.arr.length; i++) {
            s += o.arr[i];
            if (o.arr.length < 6) o.arr.push(10);
        }
        console.log(s + " " + o.arr.length);
        "#,
        "36 6",
    );
}

#[test]
fn handles_nested_loops_and_string_elements() {
    assert_same_with_and_without_hoist(
        "nested and strings",
        r#"
        const m = { rows: [[1, 2], [3, 4]] };
        let s = 0;
        for (let i = 0; i < m.rows.length; i++) {
            const row = m.rows[i];
            for (let j = 0; j < row.length; j++) s += row[j];
        }
        const w = { parts: ["a", "b", "c"] };
        let out = "";
        for (let i = 0; i < w.parts.length; i++) out += w.parts[i];
        const empty = { arr: [] as number[] };
        let e = 0;
        for (let i = 0; i < empty.arr.length; i++) e += empty.arr[i];
        console.log(s + " " + out + " " + e);
        "#,
        "10 abc 0",
    );
}

#[test]
fn hoists_through_a_const_alias_and_a_capture() {
    // `const h = holder` cannot be rebound and neither can `holder`, so the
    // alias inherits the data-field proof. The closure capture keeps the same
    // LocalId, so the same rewrite reaches a receiver read inside an arrow.
    assert_same_with_and_without_hoist(
        "alias and capture",
        r#"
        const holder = { arr: [1, 2, 3, 4], n: 4 };
        function aliased(): number {
            const h = holder;
            let s = 0;
            for (let i = 0; i < h.arr.length; i++) s += h.arr[i];
            return s;
        }
        function captured(): number {
            const h = holder;
            const f = (): number => {
                let s = 0;
                for (let i = 0; i < h.arr.length; i++) s += h.arr[i];
                return s;
            };
            return f();
        }
        console.log(aliased() + " " + captured());
        "#,
        "10 10",
    );
}

#[test]
fn refuses_an_alias_of_a_reassignable_binding() {
    // `let base` is not in the registry, so the alias inherits nothing and the
    // loop keeps its per-iteration lookup. Pinned because the alias rule would
    // be unsound if it ever followed a mutable source.
    assert_same_with_and_without_hoist(
        "mutable source",
        r#"
        let base: any = { get arr() { return [1, 2]; } };
        const h = base;
        let s = 0;
        for (let i = 0; i < h.arr.length; i++) s += h.arr[i];
        console.log(s);
        "#,
        "3",
    );
}

#[test]
fn hoists_an_outer_loop_containing_a_nested_loop() {
    assert_same_with_and_without_hoist(
        "nested loop hoisted",
        r#"
        const g = { cells: [1, 2, 3, 4], n: 4 };
        let s = 0;
        for (let r = 0; r < 3; r++) {
            for (let i = 0; i < g.cells.length; i++) {
                for (let k = 0; k < 2; k++) s += g.cells[i];
            }
        }
        console.log(s);
        "#,
        "60",
    );
}

#[test]
fn hoists_a_loop_that_returns_early() {
    assert_same_with_and_without_hoist(
        "early return",
        r#"
        const h = { arr: [3, 7, 11, 15], n: 4 };
        function firstOver(limit: number): number {
            for (let i = 0; i < h.arr.length; i++) {
                if (h.arr[i] > limit) return h.arr[i];
            }
            return -1;
        }
        console.log(firstOver(8) + " " + firstOver(100));
        "#,
        "11 -1",
    );
}
