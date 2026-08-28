//! #8897: `this.f.push(v)` is expanded by `field_push_local_bind` into a
//! receiver local plus an inline `ArrayPush` that writes the field back when
//! the append re-allocated the head. The write-back must (1) actually happen
//! after a growing append — JS equality sees through the growth-forwarding
//! stub, so it is decided on handle bits — (2) never clobber an assignment
//! the push ARGUMENT made to the same field, and (3) never throw on a
//! receiver that cannot take a plain store.
use std::path::PathBuf;
use std::process::{Command, Output};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(source: &str) -> Output {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.js");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, source).expect("write entry");
    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-auto-optimize")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    Command::new(&output).output().expect("run compiled binary")
}

fn stdout_of(run: &Output) -> String {
    assert!(
        run.status.success(),
        "the program must exit cleanly\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// The issue's reproducer: a class whose field arrays grow past their initial
/// capacity, read back through `this.packed.length` / `this.packed[i]` on a
/// hot path. Output is what Node prints; the regression was a 2.5x slowdown
/// from every such read walking the forwarding stub through the dynamic
/// property path, which this test cannot time — but the class-field IC that
/// the write-back keeps warm is also what keeps the results correct after
/// the many re-allocations below.
#[test]
fn growing_field_pushes_keep_the_field_pointing_at_the_live_head() {
    let run = compile_and_run(
        r#"
class SparseSet {
    packed = [];
    sparse = [];
    has(x) { return this.sparse[x] < this.packed.length && this.packed[this.sparse[x]] === x; }
    add(x) { if (!this.has(x)) { this.sparse[x] = this.packed.length; this.packed.push(x); } }
    remove(x) { if (this.has(x)) { const last = this.packed.pop(); if (x !== last) { this.sparse[last] = this.sparse[x]; this.packed[this.sparse[x]] = last; } } }
}
const rm = new SparseSet(); const other = new SparseSet(); other.packed = [];
for (let i = 0; i < 1000; i++) rm.add(i);
for (let i = 999; i >= 0; i--) rm.remove(i);
for (let i = 0; i < 1000; i++) rm.add(i);
let hits = 0;
for (let i = 0; i < 300000; i++) if (rm.has(i & 2047)) hits++;
console.log("hits", hits, other.packed.length, rm.packed.length);
"#,
    );
    assert_eq!(stdout_of(&run), "hits 146992 0 1000\n");
}

/// `this.items.push(this.reset())`: the receiver is read BEFORE the argument
/// runs, so the push lands on the old array, and the argument's own
/// assignment to `this.items` must survive the write-back — even when the
/// push re-allocated the old array (it is filled to its initial capacity
/// first so the appending push grows it).
#[test]
fn an_argument_that_reassigns_the_field_wins_over_the_write_back() {
    let run = compile_and_run(
        r#"
class Buffer {
    items = [];
    old = null;
    reset() { this.old = this.items; this.items = [9]; return 7; }
    fill(n) { for (let i = 0; i < n; i++) this.items.push(i); }
    add() { this.items.push(this.reset()); }
}
for (const n of [0, 16, 64]) {
    const b = new Buffer();
    b.fill(n);
    b.add();
    console.log(n, b.items.length, b.items[0], b.old.length, b.old[b.old.length - 1]);
}
"#,
    );
    assert_eq!(stdout_of(&run), "0 1 9 1 7\n16 1 9 17 7\n64 1 9 65 7\n");
}

/// A frozen receiver cannot take the repair store; the append itself is a
/// mutation of the (unfrozen) array and must still succeed, and later reads
/// of the field must still see the grown array through the forwarding head
/// rather than throwing on the skipped write-back.
#[test]
fn a_frozen_receiver_keeps_the_append_and_skips_the_repair_without_throwing() {
    let run = compile_and_run(
        r#"
class Log {
    lines = [];
    constructor() { Object.freeze(this); }
    add(v) { this.lines.push(v); }
}
const log = new Log();
for (let i = 0; i < 40; i++) log.add(i);
console.log(log.lines.length, log.lines[39], Object.isFrozen(log));
"#,
    );
    assert_eq!(stdout_of(&run), "40 39 true\n");
}
