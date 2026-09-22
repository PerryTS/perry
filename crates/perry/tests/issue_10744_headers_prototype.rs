//! Regression for #10744: axios's fetch adapter copied no response headers.
//!
//! Axios accepts an iterable header source only when `Symbol.iterator` is an
//! own member somewhere below `Object.prototype`. Perry's handle-backed
//! `Headers` implemented iteration through native dispatch, but
//! `Object.getPrototypeOf(headers)` returned null and `Headers.prototype`
//! lacked the reflected symbol member. Axios consequently classified the
//! value as a plain record, iterated `Object.keys(headers)` (empty), and
//! produced an empty `AxiosHeaders` object.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn headers_handle_exposes_its_iterable_prototype() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(
        &entry,
        r#"
const headers = new Headers({ "content-type": "application/json", "x-probe": "works" });
const proto = Object.getPrototypeOf(headers);

console.log("prototype", proto === Headers.prototype);
console.log("own-iterator", Object.prototype.hasOwnProperty.call(proto, Symbol.iterator));
console.log("iterator-alias", proto[Symbol.iterator] === proto.entries);
console.log("entries", JSON.stringify(Array.from(headers)));
"#,
    )
    .expect("write fixture");

    let compile = Command::new(PathBuf::from(env!("CARGO_BIN_EXE_perry")))
        .current_dir(dir.path())
        .args([
            "compile",
            entry.to_str().unwrap(),
            "--no-cache",
            "-o",
            output.to_str().unwrap(),
        ])
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(output).output().expect("run compiled program");
    assert!(
        run.status.success(),
        "program failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "prototype true\nown-iterator true\niterator-alias true\nentries [[\"content-type\",\"application/json\"],[\"x-probe\",\"works\"]]\n"
    );
}
