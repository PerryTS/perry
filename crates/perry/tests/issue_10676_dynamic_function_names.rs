//! Runtime-built Function sources keep their parsed function and class names.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn dynamic_function_and_class_names_are_visible() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let binary = dir.path().join("main_bin");
    std::fs::write(
        &entry,
        r#"
const literal: any = new Function("return function Literal() {}")();
const functionParts = ["return function ", "Named", "() {}"];
const dynamic: any = new Function(functionParts.join(""))();
const classParts = ["return class ", "Widget", " {}"];
const klass: any = new Function(classParts.join(""))();
console.log("FUNCTION", literal.name, dynamic.name);
console.log("CLASS", klass.name);
console.log("CONSTRUCTOR", new Function("return 1").name);
"#,
    )
    .expect("write fixture");

    let compiler = PathBuf::from(env!("CARGO_BIN_EXE_perry"));
    let compile = Command::new(compiler)
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&binary)
        .arg("--no-cache")
        .output()
        .expect("compile fixture");
    assert!(
        compile.status.success(),
        "compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(binary)
        .current_dir(dir.path())
        .output()
        .expect("run fixture");
    assert!(
        run.status.success(),
        "fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "FUNCTION Literal Named\nCLASS Widget\nCONSTRUCTOR anonymous\n"
    );
}
