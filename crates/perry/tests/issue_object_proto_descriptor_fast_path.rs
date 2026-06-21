//! A descriptor on `Object.prototype` must not force every dynamic write onto
//! the slow `[[Set]]` walk. The `#5054` fast path was gated process-wide by
//! `object_proto_descriptors_in_use()`, so a single userland accessor on
//! `Object.prototype` made every wide-object build O(n²) (a 20k-property build
//! went from ~16ms to ~42s) — which hung real startup paths that build wide
//! config/registry objects while a polyfill had touched `Object.prototype`.
//!
//! Fix: gate the fast path per-key (`object_proto_may_intercept_key`). An absent
//! key on `Object.prototype` cannot be intercepted, so it stays on the fast path;
//! keys `Object.prototype` actually owns still take the slow walk. This test
//! checks correctness is preserved (the inherited setter / non-writable still
//! intercept) and that a wide build completes (would effectively hang if O(n²)).

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &std::path::Path, entry: &std::path::Path) -> (bool, String) {
    let output = dir.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(entry)
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
    (
        run.status.success(),
        String::from_utf8_lossy(&run.stdout).to_string(),
    )
}

#[test]
fn object_proto_descriptor_keeps_fast_path_for_absent_keys() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
// A userland accessor + a non-writable data prop on Object.prototype.
Object.defineProperty(Object.prototype, "intercepted", {
  set(v: any) { (this as any)._got = v; },
  get() { return (this as any)._got; },
  configurable: true,
})
Object.defineProperty(Object.prototype, "ro", { value: 7, writable: false, configurable: true })

const o: any = {}
o.fresh = 1                 // absent on Object.prototype → fast own-data write
o.intercepted = 42          // inherited accessor → setter must run, NO own prop
;(o as any).ro = 9          // inherited non-writable data → write blocked, NO own prop

console.log("fresh:", o.fresh, "own:", Object.prototype.hasOwnProperty.call(o, "fresh"))
console.log("setter-ran:", o._got, "intercepted-own:", Object.prototype.hasOwnProperty.call(o, "intercepted"))
console.log("ro-own:", Object.prototype.hasOwnProperty.call(o, "ro"))

// Wide build with a descriptor present must complete (was O(n²) → hang).
const w: any = {}
for (let i = 0; i < 5000; i++) w["k" + i] = i
console.log("wide:", Object.keys(w).length, w.k0, w.k4999)
console.log("DONE")
"#,
    )
    .expect("write entry");

    let (ok, stdout) = compile_and_run(dir.path(), &entry);
    assert!(ok, "binary failed\nstdout:\n{stdout}");
    for needle in [
        "fresh: 1 own: true",
        "setter-ran: 42 intercepted-own: false",
        "ro-own: false",
        "wide: 5000 0 4999",
        "DONE",
    ] {
        assert!(
            stdout.contains(needle),
            "expected `{needle}` in output:\n{stdout}"
        );
    }
}
