//! Regression for #8968: the read half of assignment to a private member used
//! the source spelling (`#x`) instead of the field's mangled storage key.

use std::path::PathBuf;
use std::process::Command;

fn run_source(source: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, source).expect("write source");
    let compile = Command::new(PathBuf::from(env!("CARGO_BIN_EXE_perry")))
        .current_dir(dir.path())
        .args([
            "compile",
            entry.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
        ])
        .env("PERRY_NO_CACHE", "1")
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
    String::from_utf8_lossy(&run.stdout).trim().to_string()
}

#[test]
fn private_assignment_reads_the_declared_member() {
    let stdout = run_source(
        r#"class Context {
  #res: any;
  #count = 10;
  get res(): any { return this.#res ||= { status: 200 }; }
  set res(value: any) { this.#res = value; }
  bump(): number { this.#count += 2; return this.#count; }
}
const context = new Context();
const finalized = { status: 404 };
context.res = finalized;
console.log(context.res.status, context.res === finalized, context.bump());

class Accessor {
  #raw = 7;
  get #value(): number { return this.#raw; }
  set #value(value: number) { this.#raw = value; }
  bump(): number { this.#value += 1; return this.#raw; }
}
console.log(new Accessor().bump());
"#,
    );
    assert_eq!(stdout.lines().collect::<Vec<_>>(), ["404 true 12", "8"]);
}
