//! Regression tests for the 2026-07-02 audit's GC side-table P0s (ported
//! from the stranded be73b4f8d + the static-field-global rooting fix):
//!
//! - `ACCESSOR_DESCRIPTORS` holds the ONLY reference to
//!   `Object.defineProperty` getter/setter closures; unscanned, a minor GC
//!   swept or moved them and the next property read invoked freed memory.
//! - `PROXIES` holds target/handler for proxies commonly reachable only
//!   through the registry; unscanned, every trap deref'd freed/stale memory.
//! - `@perry_static_*` class static-field globals were never registered as
//!   GC roots (the in-code ROOT comment was aspirational), so an evacuation
//!   left the global's copy pointing at the old address.
//!
//! The program churns 20k allocations, runs an explicit gc(), and is
//! executed with PERRY_GC_FORCE_EVACUATE=1 + PERRY_GC_VERIFY_EVACUATION=1 so
//! every marked non-pinned nursery object is stress-copied and any stale
//! slot panics loudly instead of reading garbage.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run_forced_evacuation(dir: &std::path::Path, source: &str) -> String {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir)
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
        .current_dir(dir)
        .env("PERRY_GC_FORCE_EVACUATE", "1")
        .env("PERRY_GC_VERIFY_EVACUATION", "1")
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed under forced evacuation (exit {:?})\nstdout:\n{}\nstderr:\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// defineProperty accessor, proxy target/handler, and a static-field object
/// must all survive allocation churn + explicit gc() under forced
/// evacuation with the stale-slot verifier armed.
#[test]
fn descriptor_proxy_static_survive_forced_evacuation() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run_forced_evacuation(
        dir.path(),
        r#"
class Counter {
  static total = { count: 0 };
}
const obj: any = {};
const hidden = { v: 41 };
Object.defineProperty(obj, "x", {
  get() {
    return hidden.v + 1;
  },
  configurable: true,
});
const target: any = { name: "t" };
const proxy = new Proxy(target, {
  get(t: any, k: any) {
    return k === "who" ? "proxied-" + t.name : t[k];
  },
});
let sink = 0;
for (let i = 0; i < 20000; i++) {
  const tmp = { i, s: "pad" + i };
  sink += tmp.s.length > 0 ? 1 : 0;
}
(globalThis as any).gc?.();
Counter.total.count += 1;
console.log("acc:", obj.x);
console.log("proxy:", proxy.who);
console.log("static:", Counter.total.count);
console.log("churn:", sink);
"#,
    );
    assert_eq!(
        stdout,
        "acc: 42\nproxy: proxied-t\nstatic: 1\nchurn: 20000\n"
    );
}
