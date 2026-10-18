//! An array birth inside a closure is born in its named-property layout.
//! Codegen lowers closure bodies from clones of the HIR, so the layout must
//! travel with the birth expression: an address-keyed table of births missed
//! every closure birth (all 606 of tsc's, whose module is one CJS wrapper
//! closure) and the closure's descriptor was emitted but never attached.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn runtime_dir() -> PathBuf {
    std::env::var_os("PERRY_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            perry_bin()
                .parent()
                .expect("compiler directory")
                .to_path_buf()
        })
}

const SOURCE: &str = r#"function mk() { const a: any = [1, 2]; a.foo = 3; a.bar = 4; return a; }
const make = () => { const b: any = [5]; b.baz = 6; return b; };
const x = mk();
const y = make();
console.log(x.foo, x.bar, x.length, y.baz, y.length);
"#;

/// `node --experimental-strip-types` output of `SOURCE`.
const EXPECTED: &str = "3 4 2 6 1\n";

#[test]
fn a_closure_birth_attaches_its_layout() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(&entry, SOURCE).expect("write fixture");
    let output = dir.path().join("main_bin");
    let ll_dir = dir.path().join("ll");
    std::fs::create_dir(&ll_dir).expect("create IR dump dir");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env_remove("PERRY_LLVM_INPROCESS")
        .env("PERRY_SAVE_LL", &ll_dir)
        .env("PERRY_RUNTIME_DIR", runtime_dir())
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    // Every function that attaches a layout, from the saved IR.
    let mut attaching: Vec<String> = Vec::new();
    for file in std::fs::read_dir(&ll_dir).expect("IR dump dir") {
        let text = std::fs::read_to_string(file.expect("IR file").path()).expect("read IR");
        let mut function = "";
        for line in text.lines() {
            if line.starts_with("define ") {
                function = line;
            } else if line.contains("@perry_array_attach_layout(") && line.contains("call ") {
                attaching.push(function.to_owned());
            }
        }
    }
    assert!(
        attaching.iter().any(|f| f.contains("@perry_fn_main_ts__mk(")),
        "the function birth attaches no layout: {attaching:?}"
    );
    assert!(
        attaching.iter().any(|f| f.contains("@perry_closure_main_ts__")),
        "the closure birth attaches no layout: {attaching:?}"
    );

    let run = Command::new(&output).output().expect("run binary");
    assert!(run.status.success(), "binary failed: {run:?}");
    assert_eq!(String::from_utf8_lossy(&run.stdout), EXPECTED);
}
