//! Regression tests for #2656 — WeakMap/WeakSet are *actually* weak.
//!
//! Entries are stored as `CLASS_ID_WEAK_ENTRY` objects whose field-0 key is a
//! weak GC slot (skipped by the strong-edge scanners, like a WeakRef target).
//! A post-mark pass tombstones entries whose key was collected, so a key/value
//! reachable only through the collection becomes collectible — while live
//! entries are retained and values are released when their key dies.
//!
//! Verified under the default GC (and the auto-evacuation policy). The
//! `PERRY_GC_FORCE_EVACUATE` full-evacuation stress mode is out of scope here:
//! it over-collects weak targets generally (FinalizationRegistry too) and is
//! also subject to the separate strong-array-in-closure bug #5467.

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

/// A WeakMap key reachable only through the map is collected (deref→undefined),
/// its value is released, a live key's entry is retained, and a WeakSet member
/// behaves the same.
#[test]
fn weakmap_weakset_keys_are_collectible() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
declare function gc(): void;
function churn(n: number): void {
  let j: any[] = [];
  for (let i = 0; i < n; i++) { j.push({ i, p: "weak-" + i }); if (j.length > 64) j = []; }
}

const wm = new WeakMap<object, object>();
const ws = new WeakSet<object>();
const liveKey: any = { tag: "liveKey" };     // kept alive on purpose
let weakDeadKey = new WeakRef({ m: "ph" });
let weakDeadVal = new WeakRef({ m: "ph" });
let weakLiveVal = new WeakRef({ m: "ph" });
let weakDeadMember = new WeakRef({ m: "ph" });

(function setup() {
  // live key -> its value must be retained
  const lv: any = { v: "liveVal" };
  wm.set(liveKey, lv);
  weakLiveVal = new WeakRef(lv);
  // dead key -> key collected AND value released
  let dk: any = { m: "deadKey" };
  let dv: any = { m: "deadVal" };
  wm.set(dk, dv);
  weakDeadKey = new WeakRef(dk);
  weakDeadVal = new WeakRef(dv);
  dk = null; dv = null;
  // live + dead WeakSet members
  ws.add(liveKey);
  let dm: any = { m: "deadMember" };
  ws.add(dm);
  weakDeadMember = new WeakRef(dm);
  dm = null;
})();

for (let i = 0; i < 8; i++) { churn(40000); gc(); await Promise.resolve(); }

console.log("deadKey collected:", weakDeadKey.deref() === undefined);
console.log("deadVal released:", weakDeadVal.deref() === undefined);
console.log("deadMember collected:", weakDeadMember.deref() === undefined);
console.log("liveKey entry kept:", wm.has(liveKey) && (wm.get(liveKey) as any)?.v === "liveVal");
console.log("liveVal kept:", weakLiveVal.deref() !== undefined);
console.log("liveKey in set:", ws.has(liveKey));
"#,
    )
    .expect("write entry");

    let (ok, stdout) = compile_and_run(dir.path(), &entry);
    assert!(ok, "binary crashed\nstdout:\n{stdout}");
    for needle in [
        "deadKey collected: true",
        "deadVal released: true",
        "deadMember collected: true",
        "liveKey entry kept: true",
        "liveVal kept: true",
        "liveKey in set: true",
    ] {
        assert!(
            stdout.contains(needle),
            "expected `{needle}` in output:\n{stdout}"
        );
    }
}
