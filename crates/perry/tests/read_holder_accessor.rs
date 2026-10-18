//! Class getters keep the original receiver across collection and observe
//! changed prototypes/descriptors in the primary agent and workers.
use std::path::PathBuf;
use std::process::Command;

const SOURCE: &str = r#"import { spawn } from 'perry/thread';
function makeHolder() {
  return class { get path(): number {
    if ((globalThis as any).accessorCollect) {
      (globalThis as any).accessorCollect = false;
      (globalThis as any).gc();
    }
    return (this as any).child.value + (this as any).factor;
  } };
}
const HolderA = makeHolder();
const HolderB = makeHolder();
class Host { n = 1; child = { value: 1 }; }
function read(o: any): number { return o.path; }
async function main(): Promise<void> {
  const o: any = new Host();
  const a: any = HolderA.prototype;
  const b: any = HolderB.prototype;
  a.factor = 1;
  b.factor = 7;
  // An explicit, serial prototype link gives Host a MIXED identity.
  Object.setPrototypeOf(o, a);
  let first = 0;
  for (let i = 0; i < 1000; i++) {
    if (i === 500) (globalThis as any).accessorCollect = true;
    first += read(o);
  }
  let keep: any[] = [];
  for (let i = 0; i < 20000; i++) keep.push({ i });
  (globalThis as any).gc();
  keep = [];
  let moved = 0;
  for (let i = 0; i < 1000; i++) moved += read(o);
  // A live receiver-link check rejects A after the prototype is replaced.
  Object.setPrototypeOf(o, b);
  let replaced = 0;
  for (let i = 0; i < 1000; i++) replaced += read(o);
  Object.defineProperty(b, 'path', {
    configurable: true,
    get() {
      if ((globalThis as any).accessorCollect) {
        (globalThis as any).accessorCollect = false;
        (globalThis as any).gc();
      }
      return (this as any).child.value + 30;
    }
  });
  let mutated = 0;
  for (let i = 0; i < 1000; i++) mutated += read(o);
  // A declared class with no user override is the bare CLASS identity that
  // resolves through the class registry, as Zod's ParseInputLazyPath does.
  class Bare { n = 2; get path(): number { return this.n + 40; } }
  function bareRead(x: any): number { return x.path; }
  const bare: any = new Bare();
  let declared = 0;
  for (let i = 0; i < 1000; i++) declared += bareRead(bare);
  const sab = new SharedArrayBuffer(8);
  const gate = new Int32Array(sab);
  const pending = spawn(() => {
    const workerGate = new Int32Array(sab);
    class WorkerHolder { get path(): number {
      if ((globalThis as any).workerAccessorCollect) {
        (globalThis as any).workerAccessorCollect = false;
        (globalThis as any).gc();
      }
      return (this as any).n + 100;
    } }
    const w: any = { n: 3 };
    Object.setPrototypeOf(w, WorkerHolder.prototype);
    (globalThis as any).gc();
    Atomics.store(workerGate, 0, 1);
    Atomics.notify(workerGate, 0);
    if (Atomics.wait(workerGate, 1, 0, 10000) === 'timed-out') throw new Error('primary did not overlap worker');
    let total = 0;
    for (let i = 0; i < 1000; i++) {
      if (i === 500) (globalThis as any).workerAccessorCollect = true;
      total += read(w);
    }
    return total;
  });
  if (Atomics.wait(gate, 0, 0, 10000) === 'timed-out') throw new Error('worker did not start');
  let overlap = 0;
  for (let i = 0; i < 1000; i++) {
    if (i === 500) (globalThis as any).accessorCollect = true;
    overlap += read(o);
  }
  Atomics.store(gate, 1, 1);
  Atomics.notify(gate, 1);
  const worker = await pending;
  (globalThis as any).gc();
  console.log(first, moved, replaced, mutated, declared, overlap, worker, read(o));
}
main();
"#;

#[test]
fn class_accessors_keep_the_original_receiver_across_collection_and_workers() {
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
        .env("PERRY_GC_INSTRUMENTS", "1")
        .output()
        .expect("compile");
    assert!(
        compile.status.success(),
        "compile failed:\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(&output)
        .current_dir(dir.path())
        .env("PERRY_GC_FORCE_EVACUATE", "1")
        .env("PERRY_GC_VERIFY_EVACUATION", "1")
        .output()
        .expect("run");
    let stderr = String::from_utf8_lossy(&run.stderr).into_owned();
    assert!(run.status.success(), "run failed:\n{stderr}");
    assert_eq!(
        String::from_utf8_lossy(&run.stdout).trim(),
        "2000 2000 8000 31000 42000 31000 103000 31",
        "{stderr}"
    );

    // Every original receiver/prototype/worker output remains checked above.
}
