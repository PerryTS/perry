//! A named read on a positive number whose low 48 bits look like an address
//! (every 0xRRGGBB colour) must answer `undefined`, not read a forged header.

use std::path::PathBuf;
use std::process::Command;

fn compile_and_run(source: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, source).expect("write entry");
    let compile = Command::new(PathBuf::from(env!("CARGO_BIN_EXE_perry")))
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
        "compiled binary failed (exit {:?})\nstdout:\n{}\nstderr:\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

#[test]
fn named_read_on_colour_number_is_undefined() {
    let stdout = compile_and_run(
        r#"function f(value: any) { return value.isColor; } console.log("ok", f(0xffffff));
"#,
    );
    assert_eq!(stdout, "ok undefined\n");
}

#[test]
fn named_read_on_many_numbers_is_undefined() {
    let stdout = compile_and_run(
        r#"
function f(value: any) { return value.isColor; }
const xs: any[] = [255, 256, 65535, 65536, 1048576, 16777215, 16777216, 2147483647, 1.5, -1, 1e10];
for (const x of xs) { console.log(x, f(x)); }
"#,
    );
    assert_eq!(
        stdout,
        "255 undefined\n256 undefined\n65535 undefined\n65536 undefined\n1048576 undefined\n16777215 undefined\n16777216 undefined\n2147483647 undefined\n1.5 undefined\n-1 undefined\n10000000000 undefined\n"
    );
}
