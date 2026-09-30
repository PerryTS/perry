//! A class getter uses the read site's collecting accessor entry. The entry
//! must keep the original receiver, follow a changed prototype and descriptor,
//! survive a moving holder, and stop at worker startup.
use std::path::PathBuf;
use std::process::Command;

const SOURCE: &str = r#"import { spawn } from 'perry/thread';
class HolderA { get path(): number { return (this as any).n + 1; } }
class HolderB { get path(): number { return (this as any).n + 7; } }
class Host { n = 1; }
function read(o: any): number { return o.path; }
async function main(): Promise<void> {
  const o: any = new Host();
  const a: any = HolderA.prototype;
  const b: any = HolderB.prototype;
  // An explicit, serial prototype link gives Host a MIXED identity.
  Object.setPrototypeOf(o, a);
  let first = 0;
  for (let i = 0; i < 1000; i++) first += read(o);
  let keep: any[] = [];
  for (let i = 0; i < 20000; i++) keep.push({ i });
  (globalThis as any).gc();
  keep = [];
  let moved = 0;
  for (let i = 0; i < 1000; i++) moved += read(o);
  // Both prototypes declare the same accessor key. A receiver-link check,
  // not holder shape alone, must reject a stale answer from A.
  Object.setPrototypeOf(o, b);
  let replaced = 0;
  for (let i = 0; i < 1000; i++) replaced += read(o);
  Object.defineProperty(b, 'path', {
    configurable: true,
    get() { return (this as any).n + 30; }
  });
  let mutated = 0;
  for (let i = 0; i < 1000; i++) mutated += read(o);
  const sab = new SharedArrayBuffer(8);
  const gate = new Int32Array(sab);
  const pending = spawn(() => {
    const workerGate = new Int32Array(sab);
    class WorkerHolder { get path(): number { return (this as any).n + 100; } }
    const w: any = { n: 3 };
    Object.setPrototypeOf(w, WorkerHolder.prototype);
    (globalThis as any).gc();
    Atomics.store(workerGate, 0, 1);
    Atomics.notify(workerGate, 0);
    if (Atomics.wait(workerGate, 1, 0, 10000) === 'timed-out') throw new Error('primary did not overlap worker');
    let total = 0;
    for (let i = 0; i < 1000; i++) total += read(w);
    return total;
  });
  if (Atomics.wait(gate, 0, 0, 10000) === 'timed-out') throw new Error('worker did not start');
  let overlap = 0;
  for (let i = 0; i < 1000; i++) overlap += read(o);
  Atomics.store(gate, 1, 1);
  Atomics.notify(gate, 1);
  const worker = await pending;
  (globalThis as any).gc();
  console.log(first, moved, replaced, mutated, overlap, worker, read(o));
}
main();
"#;

fn stat(stderr: &str, name: &str) -> u64 {
    let line = stderr
        .lines()
        .find(|l| l.starts_with("[method-site]"))
        .unwrap_or_else(|| panic!("missing method-site stats: {stderr}"));
    line.split_whitespace()
        .find_map(|w| w.strip_prefix(name).and_then(|v| v.strip_prefix('=')))
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| panic!("missing {name}: {line}"))
}

#[test]
fn class_accessor_entry_collects_with_original_receiver_and_stops_for_workers() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, SOURCE).expect("write entry");
    let compile = Command::new(PathBuf::from(env!("CARGO_BIN_EXE_perry")))
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .env("PERRY_NO_CACHE", "1")
        .output()
        .expect("compile");
    assert!(
        compile.status.success(),
        "compile failed:\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(&output)
        .current_dir(dir.path())
        .env("PERRY_METHOD_SITE_STATS", "1")
        .env("PERRY_GC_FORCE_EVACUATE", "1")
        .env("PERRY_GC_VERIFY_EVACUATION", "1")
        .env("PERRY_GC_POISON_FROMSPACE", "1")
        .output()
        .expect("run");
    let stderr = String::from_utf8_lossy(&run.stderr).into_owned();
    assert!(run.status.success(), "run failed:\n{stderr}");
    assert_eq!(
        String::from_utf8_lossy(&run.stdout).trim(),
        "2000 2000 8000 31000 31000 103000 31",
        "{stderr}"
    );
    assert!(
        stat(&stderr, "read_accessor_primes") > 0,
        "accessor entry never primed: {stderr}"
    );
    assert!(
        stat(&stderr, "read_accessor_hits") > 0,
        "accessor entry never hit: {stderr}"
    );
    assert!(
        stat(&stderr, "read_holder_rewrites") > 0,
        "holder never moved: {stderr}"
    );
}
