//! Runtime import attributes and OpenCode's legacy TOML config migration.
use std::process::Command;

fn compile_and_run(source: &str) -> String {
    let directory = tempfile::tempdir().unwrap();
    let entry = directory.path().join("main.ts");
    let binary = directory
        .path()
        .join(if cfg!(windows) { "main.exe" } else { "main" });
    let source = source.replace(
        "\"__LITERAL_DATA_PATH__\"",
        &serde_json::to_string(&directory.path().join("literal-data")).unwrap(),
    );
    std::fs::write(&entry, source).unwrap();
    let mut compiler = Command::new(env!("CARGO_BIN_EXE_perry"));
    // #7354: LLVM RS4GC does not support Windows exception funclets yet.
    // Exercise the supported shadow-root path for async rejection tests.
    if cfg!(windows) {
        compiler.env("PERRY_RS4GC", "0");
    }
    let compile = compiler
        .current_dir(directory.path())
        .args(["compile", "--no-cache"])
        .arg(&entry)
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "compile failed: {}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(binary)
        .current_dir(directory.path())
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "run failed: {}\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(
        run.stderr.is_empty(),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8(run.stdout).unwrap().replace("\r\n", "\n")
}

#[test]
fn runtime_data_loaders_and_legacy_migration_match_bun() {
    let output = compile_and_run(include_str!(
        "../../../test-files/test_dynamic_import_data_10104.ts"
    ));
    assert_eq!(
        output,
        concat!(
            "toml anthropic claude-sonnet-4-5 dark\n",
            "json 42\njson url true\n",
            "text \"hello π\\n\"\nfile true\n",
            "options 42 so\n",
            "bad toml true\nbad json SyntaxError true\n",
            "migrated true\n",
            "{\n  \"model\": \"anthropic/claude-sonnet-4-5\",\n",
            "  \"$schema\": \"https://opencode.ai/config.json\",\n",
            "  \"theme\": \"dark\"\n}\n",
        )
    );
}

#[test]
fn literal_absolute_data_path_is_read_after_compilation() {
    let output = compile_and_run(
        r#"
import { writeFileSync } from "node:fs";
writeFileSync("__LITERAL_DATA_PATH__", "answer = 42\n");
const mod = await import("__LITERAL_DATA_PATH__", { with: { type: "toml" } });
console.log(mod.default.answer);
"#,
    );
    assert_eq!(output, "42\n");
}

#[test]
fn runtime_code_imports_keep_the_deferred_error() {
    let output = compile_and_run(
        r#"
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
const source = join(process.cwd(), "runtime-code.js");
writeFileSync(source, "throw new Error('code must not execute')");
const load = (specifier: string, options?: any) => import(specifier, options);
await load(pathToFileURL(source).href).catch((error: any) => console.log(error.code));
await load(source, { with: { type: "javascript" } }).catch((error: any) => console.log(error.code));
await load("./runtime-code.js", { with: { type: "text" } }).catch((error: any) => console.log(error.code));
const badToml = join(process.cwd(), "broken.toml");
writeFileSync(badToml, "broken = [");
await load(pathToFileURL(badToml).href, { with: { type: "toml" } })
  .catch((error: any) => console.log(error.name, error instanceof SyntaxError));
"#,
    );
    assert_eq!(
        output,
        "ERR_MODULE_NOT_FOUND\nERR_MODULE_NOT_FOUND\nERR_MODULE_NOT_FOUND\nSyntaxError true\n"
    );
}
