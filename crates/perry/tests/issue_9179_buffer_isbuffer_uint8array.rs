//! Regression coverage for #9179: `Buffer.isBuffer()` is a brand check, not a
//! test for Perry's shared Buffer/Uint8Array storage representation.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn buffer_isbuffer_rejects_plain_uint8arrays_in_every_call_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(
        &entry,
        r#"
const buffer = Buffer.alloc(4);
const uint8array = new Uint8Array(4);
const plain = {};

console.log(
  Buffer.isBuffer(buffer),
  Buffer.isBuffer(uint8array),
  Buffer.isBuffer(plain),
);

const predicate = Buffer.isBuffer;
console.log(
  predicate(buffer),
  predicate(uint8array),
  predicate(plain),
);
"#,
    )
    .expect("write fixture");

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
        .output()
        .expect("run compiled fixture");
    assert!(
        run.status.success(),
        "compiled fixture failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "true false false\ntrue false false\n"
    );
}
