//! A class-field read the inline guard cannot prove takes the One Path read,
//! and a by-name overwrite keeps the typed layout that guard reads.
//!
//! `this.x` inside a class or object-literal method compiles to a class-field
//! read: an inline pre-check of the receiver against the class's birth shape
//! and its `GC_OBJ_TYPED_LAYOUT_INTACT` bit, then a slot load. Two things sent
//! ordinary receivers past it, on every read, into a site-less by-name walk
//! (~1,300 instructions where the same read elsewhere costs ~100-300):
//!
//! * a by-name overwrite of an existing key (`o.d = k` at a site that meets the
//!   object through a parameter or a module `const`) declared the object's
//!   whole layout unknown, which cleared the intact bit, so the pre-check of
//!   every later `this.a` missed;
//! * a receiver of another shape reading the method's key (an `Object.create`
//!   child of the literal, reading through `this`) is never the pre-check's,
//!   and the miss arm walked by name instead of asking the receiver's shape.
//!
//! Both are asserted from the receiver-route census (`PERRY_RECV_ROUTE_COUNT=1`
//! at compile time; one `PERRY_RECV_ROUTES` line at exit), with the output
//! checked against the values node prints.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// Compile `source` as a census build and run it; return stdout and a reader
/// of the route counters.
fn run_census(source: &str) -> (String, impl Fn(&str) -> u64) {
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
        .env("PERRY_RECV_ROUTE_COUNT", "1")
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
    let stderr = String::from_utf8_lossy(&run.stderr).into_owned();
    assert!(
        run.status.success(),
        "binary failed ({:?})\nstderr:\n{stderr}",
        run.status
    );
    let line = stderr
        .lines()
        .find(|l| l.starts_with("PERRY_RECV_ROUTES"))
        .unwrap_or_else(|| panic!("no PERRY_RECV_ROUTES line: the census did not run\n{stderr}"))
        .to_owned();
    let count = move |name: &str| -> u64 {
        line.split_whitespace()
            .find_map(|w| w.strip_prefix(name)?.strip_prefix('=')?.parse().ok())
            .unwrap_or_else(|| panic!("route {name} missing from `{line}`"))
    };
    (
        String::from_utf8_lossy(&run.stdout).trim().to_owned(),
        count,
    )
}

/// Sabotage: route `js_class_field_get_ic`'s feedback-off arm back to
/// `class_field_get_after_guard_fail` (the by-name walk) -> the shape answers
/// nothing (`rt_class_miss_shape` == 0).
#[test]
fn a_this_read_the_guard_cannot_prove_is_answered_from_the_receivers_shape() {
    let (stdout, count) = run_census(
        r#"// `this.pa` and `this.own` are class-field reads of PROTO's literal class; the
// receiver is an Object.create child of PROTO, which that pre-check never
// admits. `pa` is inherited, `own` is the child's own key.
const N = process.argv.length > 99 ? 1 : 3000;
// Arms the census: runtime-counted routes count only after the first emitted
// route note, so a read nothing can fold runs first.
const probes: any[] = [{ x: 5 }, { y: 0, x: 5 }];
const px = probes[process.argv.length & 1].x;
const PROTO: any = {
    pa: 32,
    own: 0,
    sum(n: number): number { let h = 0; for (let k = 0; k < n; k++) { h += this.pa; } return h; },
    mine(n: number): number { let h = 0; for (let k = 0; k < n; k++) { h += this.own; } return h; },
};
const child: any = Object.create(PROTO);
child.own = 3;
const a = child.sum(N);
const b = child.mine(N);
PROTO.pa = 40; // the inherited value changes under the cache
const c = child.sum(2);
child.pa = 1; // and is then shadowed by an own key
const d = child.sum(2);
console.log(px, a, b, c, d, PROTO.sum(1));
"#,
    );
    assert_eq!(stdout, "5 96000 9000 80 2 40");
    let shape = count("rt_class_miss_shape");
    assert!(
        shape >= 2 * 3000 - 8,
        "only {shape} of ~6000 unproven `this` reads were answered from the \
         receiver's shape (the site's word or the inherited cache)"
    );
}
