//! Regression test: writing many fresh own properties to a CLASS INSTANCE
//! (`class C {}; const o = new C(); for (i) o["k"+i] = i`) must be O(1) per
//! insert — the same as a plain `{}` — while STILL honoring an inherited
//! setter that intercepts the write.
//!
//! Two layered O(n²) bugs previously made the class-instance wide build scale
//! quadratically (a 20k build took tens of seconds vs ~25ms for a plain
//! object):
//!   * The dynamic-write sidecar key index was registered under the keys-array
//!     pointer instead of the (stable) object pointer on the inline-slot append
//!     path, so the obj-keyed lookup never hit and rebuilt the full index every
//!     insert.
//!   * `Object.getPrototypeOf` on a declared-class instance resolved its
//!     `[[Prototype]]` via a `constructor`-field probe, which does a LINEAR scan
//!     over the instance's own keys before missing — re-run on every set by the
//!     `[[Set]]` interception check, the scan grew by one each iteration.
//!
//! This test asserts BOTH: the wide build completes (and reads back), and an
//! inherited `set` accessor still intercepts (no own data property created).

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(src: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, src).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).to_string()
}

#[test]
fn class_instance_wide_set_is_fast_and_intercepts() {
    // An `Object.prototype` accessor is present (the worst case that forces the
    // interception check on every write). The wide build of 20_000 fresh keys
    // must still complete and read back correctly. The base-class `set baz`
    // must intercept (no own `baz` data property created on the instance).
    let src = r#"
Object.defineProperty(Object.prototype, "__x__", { get(){ return 1; }, configurable: true });

class Base { _b: any; set baz(v: any) { this._b = "base:" + v; } }
class C extends Base { [k: string]: any; }

const o: any = new C();
const N = 20000;
for (let i = 0; i < N; i++) o["k" + i] = i;

// Inherited setter still intercepts: stored via the setter, NOT as an own prop.
o.baz = 5;

const own = (obj: any, k: string) => Object.prototype.hasOwnProperty.call(obj, k);

// Wide build completed: every key read back, count is exact.
console.log("count", Object.keys(o).length);
console.log("first", o["k0"], "mid", o["k12345"], "last", o["k19999"]);
console.log("setter", o._b, own(o, "baz"));
console.log("fresh-own", own(o, "k7"));
console.log("DONE");
"#;

    let out = compile_and_run(src);
    // Object.keys includes the 20_000 fresh keys (the inherited `baz` setter
    // created `_b`, an own data field on the instance, but `baz` itself is not
    // an own key). The exact count guards against dropped/duplicated keys.
    assert!(
        out.contains("count 20001"),
        "wide build must keep every fresh key (+ the setter-created `_b`)\n{out}"
    );
    assert!(
        out.contains("first 0 mid 12345 last 19999"),
        "values must read back at the correct keys\n{out}"
    );
    // `o.baz = 5` ran the inherited setter (`_b == "base:5"`) and created NO
    // own `baz` property — interception preserved despite the fast insert path.
    assert!(
        out.contains("setter base:5 false"),
        "inherited setter must intercept (no own `baz` prop)\n{out}"
    );
    assert!(
        out.contains("fresh-own true"),
        "a fresh key is a real own data property\n{out}"
    );
    assert!(
        out.contains("DONE"),
        "program must run to completion\n{out}"
    );
}
