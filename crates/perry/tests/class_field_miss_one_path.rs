//! A class-field read is a shape-site read, for every receiver (#12483).
//!
//! `this.x` inside a class or object-literal method used to compile to its own
//! tower: an inline pre-check against the class's BIRTH ShapeId, then a slot
//! load, with every miss sent out of line. A receiver that left the birth
//! shape missed it on every read: a plain JavaScript class (its keys are
//! constructor stores, so no instance is ever born on the class's keys-global
//! shape) paid ~150 instructions per `this.a` against ~13 for the same class
//! written in `.ts` with declared fields, and the loop regions' static supplier
//! made the same bet and lost it the same way.
//!
//! Asserted from the receiver-route census (`PERRY_RECV_ROUTE_COUNT=1` at
//! compile time; one `PERRY_RECV_ROUTES` line at exit), with the output checked
//! against the values node prints.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// Compile `source` (as `main.<ext>`) as a census build and run it; return
/// stdout and a reader of the route counters.
fn run_census(source: &str, ext: &str) -> (String, impl Fn(&str) -> u64) {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join(format!("main.{ext}"));
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

/// The #12483 witness: a plain JavaScript class whose constructor defines its
/// keys. Its loop region is entered through the LEARNED word (the shape the
/// instance actually has) and runs its fast copy on (nearly) every iteration.
/// The `.ts` twin with declared fields keeps the static supplier, whose id its
/// instances do carry.
///
/// Sabotage: drop the `class_instances_keep_birth_shape` filter from the
/// region's `static_class_name` -> the `.js` region names the class's static
/// id, no instance carries it, and `rloop_f` stays 0 (every iteration takes
/// the generic copy).
#[test]
fn a_plain_js_class_loop_region_follows_the_shape_its_instances_have() {
    const JS: &str = r#"const N = process.argv.length > 99 ? 1 : 3000;
// Arms the census: runtime-counted routes count only after the first emitted
// route note, so a read nothing can fold runs first.
const probes = [{ x: 5 }, { y: 0, x: 5 }];
const px = probes[process.argv.length & 1].x;
class P {
    constructor() { this.a = 1; this.b = 2; this.c = 3; this.d = 4; this.e = 5; }
    sum(n) { let s = 0; for (let i = 0; i < n; i++) { this.d = i; s += this.a; } return s; }
}
console.log(px, new P().sum(N));
"#;
    let (stdout, count) = run_census(JS, "js");
    assert_eq!(stdout, "5 3000");
    assert!(
        count("rloop_f") >= 3000 - 8,
        "only {} of 3000 iterations ran the region's fast copy: the guard named \
         a ShapeId the plain-JS instance never carries",
        count("rloop_f")
    );
    assert_eq!(
        count("rloop_static"),
        0,
        "a constructor-store class must not take the static supplier"
    );
    assert_eq!(count("class_read"), 0, "no class read guard may run");

    const TS: &str = r#"const N = process.argv.length > 99 ? 1 : 3000;
const probes: any[] = [{ x: 5 }, { y: 0, x: 5 }];
const px = probes[process.argv.length & 1].x;
class P {
    a: number; b: number; c: number; d: number; e: number;
    constructor() { this.a = 1; this.b = 2; this.c = 3; this.d = 4; this.e = 5; }
    sum(n: number): number { let s = 0; for (let i = 0; i < n; i++) { this.d = i; s += this.a; } return s; }
}
console.log(px, new P().sum(N));
"#;
    let (stdout, count) = run_census(TS, "ts");
    assert_eq!(stdout, "5 3000");
    assert!(
        count("rloop_static") >= 1 && count("rloop_f") >= 3000 - 8,
        "the declared-field twin must keep its static supplier and its fast copy \
         (rloop_static={}, rloop_f={})",
        count("rloop_static"),
        count("rloop_f")
    );
}

/// `this.a` in an object literal's method, after a by-name overwrite of the
/// receiver, is served by the read site's own word: no class guard runs, and
/// the site's MRU word answers (nearly) every one of the 3000 reads.
#[test]
fn a_this_read_in_a_literal_method_is_served_by_the_site_word() {
    let (stdout, count) = run_census(
        r#"// The receiver reaches `put` as a PARAMETER, so `o.d = k` is the generic
// store, whose first execution is the runtime's by-name overwrite; `o.m()`
// runs the literal's method, whose `this.a` is a class-field read. The store
// sits in a callee, not in the loop: a store written in the loop body is
// served by the loop's receiver region (step 4b).
const N = process.argv.length > 99 ? 1 : 3000;
const probes: any[] = [{ x: 5 }, { y: 0, x: 5 }];
const px = probes[process.argv.length & 1].x;
function put(o: any, k: number): void { o.d = k; }
function run(n: number, o: any): number {
    let h = 0;
    for (let k = 0; k < n; k++) {
        put(o, k);
        h += o.m();
    }
    return h + o.d;
}
console.log(px, run(N, { a: 1, b: 2, c: 4, e: 8, d: 16, m() { return this.a; } }));
"#,
        "ts",
    );
    assert_eq!(stdout, "5 5999");
    assert_eq!(count("class_read"), 0, "no class read guard may run");
    assert!(
        count("generic_mru_hit") >= 3000 - 16,
        "only {} reads were served by a site's word; `this.a` must be",
        count("generic_mru_hit")
    );
}

/// `this.pa` (inherited) and `this.own` (own) in a literal's methods, called
/// on an `Object.create` child of it: both are ordinary shape-site reads of
/// the child's shape, in the same order and with the same answers as any
/// other receiver, including after the inherited value changes and after an
/// own key shadows it.
#[test]
fn a_this_read_of_another_shape_is_an_ordinary_site_read() {
    let (stdout, count) = run_census(
        r#"// Each read sits in its own method, called from the loop: a read written
// in the loop body is served by the loop's receiver region (step 4b).
const N = process.argv.length > 99 ? 1 : 3000;
const probes: any[] = [{ x: 5 }, { y: 0, x: 5 }];
const px = probes[process.argv.length & 1].x;
const PROTO: any = {
    pa: 32,
    own: 0,
    getPa(): number { return this.pa; },
    getOwn(): number { return this.own; },
    sum(n: number): number { let h = 0; for (let k = 0; k < n; k++) { h += this.getPa(); } return h; },
    mine(n: number): number { let h = 0; for (let k = 0; k < n; k++) { h += this.getOwn(); } return h; },
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
        "ts",
    );
    assert_eq!(stdout, "5 96000 9000 80 2 40");
    assert_eq!(count("class_read"), 0, "no class read guard may run");
    assert!(
        count("generic") >= 2 * 3000,
        "only {} reads passed a site's receiver test; the ~6000 `this` reads \
         must all be site reads",
        count("generic")
    );
}
