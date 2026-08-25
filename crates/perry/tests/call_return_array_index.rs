//! Executable semantics for array-index stores whose assignment base is a
//! call expression. The HIR repeats that call as the PutValue target and
//! receiver, but the source evaluates it exactly once.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(source: &str) -> String {
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
        .current_dir(dir.path())
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

#[test]
fn call_returned_array_store_preserves_evaluation_proxy_and_descriptor_semantics() {
    let stdout = compile_and_run(
        r#"
class Store {
  calls = 0;
  constructor(public data: any[]) {}

  getData(): any[] {
    this.calls++;
    return this.data;
  }

  write(index: number, value: any): any {
    return this.getData()[index] = value;
  }
}

const plain: any[] = [10, 20];
const store = new Store(plain);
console.log("integer", store.write(1, 41), plain[1], store.calls);
console.log("fractional", store.write(1.5, 77), plain["1.5"], plain.length, store.calls);

const traps: string[] = [];
const target: any[] = [1, 2];
const proxy: any[] = new Proxy(target, {
  set(t: any, key: any, value: any, receiver: any) {
    traps.push(String(key) + ":" + String(value));
    return Reflect.set(t, key, value, receiver);
  },
});
const proxyStore = new Store(proxy);
console.log("proxy", proxyStore.write(0, 9), target[0], proxyStore.calls, traps.join(","));

const locked: any[] = [5];
Object.defineProperty(locked, "0", { value: 5, writable: false });
const lockedStore = new Store(locked);
let rejected = false;
try {
  lockedStore.write(0, 8);
} catch (_error) {
  rejected = true;
}
console.log("locked", rejected, locked[0], lockedStore.calls);

const getterOnly: any[] = [6];
Object.defineProperty(getterOnly, "0", { get() { return 6; } });
const getterStore = new Store(getterOnly);
rejected = false;
try {
  getterStore.write(0, 8);
} catch (_error) {
  rejected = true;
}
console.log("getter-only", rejected, getterOnly[0], getterStore.calls);

const accessor: any[] = [4];
let setterValue = 0;
Object.defineProperty(accessor, "0", {
  get() { return setterValue; },
  set(value: any) { setterValue = value; },
});
const accessorStore = new Store(accessor);
console.log("setter", accessorStore.write(0, 12), accessor[0], accessorStore.calls);

const hole: any[] = [1, 2, 3];
delete hole[1];
Object.preventExtensions(hole);
const holeStore = new Store(hole);
rejected = false;
try {
  holeStore.write(1, 9);
} catch (_error) {
  rejected = true;
}
console.log("sealed-hole", rejected, hole[1], holeStore.calls);

const fixedLength: any[] = [1];
Object.defineProperty(fixedLength, "length", { writable: false });
const fixedLengthStore = new Store(fixedLength);
rejected = false;
try {
  fixedLengthStore.write(1, 2);
} catch (_error) {
  rejected = true;
}
console.log("fixed-length", rejected, fixedLength.length, fixedLengthStore.calls);
"#,
    );

    assert_eq!(
        stdout,
        "integer 41 41 1\n\
         fractional 77 77 2 2\n\
         proxy 9 9 1 0:9\n\
         locked true 5 1\n\
         getter-only true 6 1\n\
         setter 12 12 1\n\
         sealed-hole true undefined 1\n\
         fixed-length true 1 1\n"
    );
}
