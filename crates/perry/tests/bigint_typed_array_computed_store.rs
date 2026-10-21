//! A computed store into a `BigInt64Array` / `BigUint64Array` whose key is not
//! proven to be an array index (`layout.offset / 8`, read from an untyped
//! record) must write the element. The receiver was lowered as a plain `Array`
//! and the array setter replaced it with a fresh empty one, so the binding
//! turned into `Array []` (OpenCode v2.1.2: `@opentui/core` fills its native
//! session record this way and failed with "Unable to convert the value to a
//! pointer").
//!
//! `node main.ts` and `bun main.ts` print exactly the asserted lines.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn bigint_typed_array_computed_store_keeps_the_typed_array() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(
        root.join("package.json"),
        r#"{ "name": "bigint-ta-computed-store", "private": true, "type": "module" }"#,
    )
    .expect("write package.json");
    let entry = root.join("main.ts");
    std::fs::write(
        &entry,
        r#"const layout: any = { fields: { max_bytes: { offset: 16 }, id: { offset: 8 } } }
const u = new BigUint64Array(4)
u[layout.fields.max_bytes.offset / 8] = 77n
const s = new BigInt64Array(4)
s[layout.fields.id.offset / 8] = -5n
const k: any = layout.fields.id.offset / 8
u[k] = 9n
console.log(u.constructor.name, u.length, u[2], u[1])
console.log(s.constructor.name, s.length, s[1], new Uint32Array(u.buffer)[4])
"#,
    )
    .expect("write entry");

    let output = root.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
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
    assert!(run.status.success(), "status: {:?}", run.status);
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "BigUint64Array 4 77n 9n\nBigInt64Array 4 -5n 77\n"
    );
}
