//! End-to-end regression coverage for #8511. OpenCode Code Mode imports a
//! narrow runtime surface from `typescript`; Perry must route it to the native
//! SWC-backed binding instead of compiling or embedding TypeScript's compiler.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn opencode_typescript_transpile_subset_compiles_and_runs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(
        &entry,
        r#"
import {
  DiagnosticCategory,
  ModuleKind,
  ScriptTarget,
  flattenDiagnosticMessageText,
  transpileModule,
} from "typescript";

const compilerOptions = {
  target: ScriptTarget.ESNext,
  module: ModuleKind.ESNext,
};
const transpiled = transpileModule(
  "async function __codemode__() { const value: number = await Promise.resolve(1); return value as number; }",
  { reportDiagnostics: true, compilerOptions },
);
const diagnostic = transpiled.diagnostics?.find(
  (item: any) => item.category === DiagnosticCategory.Error,
);

const invalid = transpileModule("const value: = 1", {
  reportDiagnostics: true,
  compilerOptions,
});
const invalidDiagnostic = invalid.diagnostics?.find(
  (item: any) => item.category === DiagnosticCategory.Error,
);

console.log([
  diagnostic === undefined,
  transpiled.outputText.includes("async function __codemode__()"),
  transpiled.outputText.includes("const value = await Promise.resolve(1)"),
  !transpiled.outputText.includes(": number"),
  flattenDiagnosticMessageText(invalidDiagnostic?.messageText ?? "", "\n").length > 0,
].join("|"));
"#,
    )
    .expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
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
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "true|true|true|true|true\n"
    );
}
