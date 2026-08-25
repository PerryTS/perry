//! Regression coverage for the `Array.prototype.slice` invalid-length
//! subcluster in #5898. Generic slice receivers may have a `ToLength` above
//! the Array length limit; the result length must be rejected before indexed
//! reads, without narrowing or trying to materialise the full receiver.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn generic_slice_rejects_oversized_results_before_index_access() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    let runtime_dir = perry_bin()
        .parent()
        .expect("perry binary directory")
        .to_path_buf();
    std::fs::write(
        &entry,
        r#"
let plainIndexReads = 0;
const plain: any = { length: 2 ** 32 };
Object.defineProperty(plain, "0", {
  get() {
    plainIndexReads++;
    return 1;
  }
});
try {
  Array.prototype.slice.call(plain);
  console.log("plain no throw");
} catch (error) {
  console.log("plain", error instanceof RangeError, plainIndexReads);
}

const aliased: any = { length: 2 ** 32 };
aliased.slice = Array.prototype.slice;
try {
  aliased.slice(0, 2 ** 32);
  console.log("aliased no throw");
} catch (error) {
  console.log("aliased", error instanceof RangeError);
}

let proxyLengthReads = 0;
let proxyIndexReads = 0;
let proxyWrites = 0;
const proxy = new Proxy([], {
  get(target: any, key: any, receiver: any) {
    if (key === "length") {
      proxyLengthReads++;
      return 2 ** 32;
    }
    proxyIndexReads++;
    return Reflect.get(target, key, receiver);
  },
  set(target: any, key: any, value: any, receiver: any) {
    proxyWrites++;
    return Reflect.set(target, key, value, receiver);
  }
});
try {
  Array.prototype.slice.call(proxy, 0, 2 ** 32);
  console.log("proxy no throw");
} catch (error) {
  console.log(
    "proxy",
    error instanceof RangeError,
    proxyLengthReads,
    proxyIndexReads,
    proxyWrites
  );
}

// A huge array-like is valid when the selected interval itself is small.
const tail: any = { length: 2 ** 32 + 1 };
tail[2 ** 32] = "last";
const selected = Array.prototype.slice.call(tail, -1);
console.log("tail", selected.length, selected[0]);
"#,
    )
    .expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .env("PERRY_LIB_DIR", &runtime_dir)
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_RS4GC", "0")
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
        "compiled binary failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        concat!(
            "plain true 0\n",
            "aliased true\n",
            "proxy true 1 0 0\n",
            "tail 1 last\n"
        )
    );
}
