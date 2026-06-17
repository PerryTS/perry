//! Regression test for #5223: `import X from "./file.txt"` used to make perry
//! TS-parse the asset and fail (`Failed to parse ...txt: Parse error`).
//!
//! Fix: a recognized text-asset extension (`.txt`, `.sql`, `.md`, `.html`,
//! `.htm`, `.css`, `.graphql`, `.gql`, `.glsl`, `.vert`, `.frag`) is read
//! verbatim and synthesized into a native module whose default export is the
//! file contents as a JS string — mirroring the existing JSON-module path. The
//! trigger is the extension; an explicit `with { type: "text" }` attribute is
//! parsed and tolerated but is not (yet) itself the trigger. `.wasm` is out of
//! scope.

use std::path::PathBuf;
use std::process::Command;
use std::sync::Once;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize workspace root")
}

fn target_debug_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join("target"))
        .join("debug")
}

/// Build `libperry_runtime.a` once so the compiled binaries can link. The CI
/// `cargo-test` job doesn't pre-build the runtime staticlib, and these tests
/// link real executables, so they'd otherwise fail with "Could not find
/// libperry_runtime.a" (mirrors module_import_forms.rs).
fn ensure_runtime_archive() {
    static BUILD_RUNTIME: Once = Once::new();
    BUILD_RUNTIME.call_once(|| {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let build = Command::new(cargo)
            .current_dir(workspace_root())
            .arg("build")
            .arg("-p")
            .arg("perry-runtime")
            .output()
            .expect("run cargo build -p perry-runtime");
        assert!(
            build.status.success(),
            "cargo build -p perry-runtime failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&build.stdout),
            String::from_utf8_lossy(&build.stderr)
        );
    });
}

fn runtime_dir() -> PathBuf {
    ensure_runtime_archive();
    target_debug_dir()
}

/// Compile `main.ts` in `dir` and return its stdout. Asset files must already
/// be written into `dir`.
fn compile_and_run(dir: &std::path::Path) -> String {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");

    let mut compile = Command::new(perry_bin());
    compile
        .current_dir(dir)
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_RUNTIME_DIR", runtime_dir())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output);
    let compile = compile.output().expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed (pre-fix: TS-parse error on .txt)\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output)
        .current_dir(dir)
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

#[test]
fn text_and_sql_default_exports_are_exact_string_contents() {
    let dir = tempfile::tempdir().expect("tempdir");
    let greeting = "Hello, Perry!\nLine two.\n  indented \"quoted\" third\n";
    let query = "SELECT *\nFROM users\nWHERE id = 1;\n";
    std::fs::write(dir.path().join("greeting.txt"), greeting).expect("write txt");
    std::fs::write(dir.path().join("query.sql"), query).expect("write sql");
    std::fs::write(
        dir.path().join("main.ts"),
        r#"
import greeting from "./greeting.txt";
import query from "./query.sql";
// JSON.stringify makes the byte-exact contents (newlines, quotes) visible.
process.stdout.write(JSON.stringify(greeting));
process.stdout.write("\n");
process.stdout.write(JSON.stringify(query));
process.stdout.write("\n");
"#,
    )
    .expect("write main");

    let stdout = compile_and_run(dir.path());
    let expected = format!(
        "{}\n{}\n",
        serde_json::to_string(greeting).unwrap(),
        serde_json::to_string(query).unwrap()
    );
    assert_eq!(stdout, expected, "text asset contents did not round-trip");
}

#[test]
fn text_import_with_type_text_attribute_compiles() {
    // The `with { type: "text" }` attribute is parsed and tolerated; the `.txt`
    // extension is what drives the text loader.
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("note.txt"), "attribute-form works\n").expect("write txt");
    std::fs::write(
        dir.path().join("main.ts"),
        r#"
import note from "./note.txt" with { type: "text" };
process.stdout.write(note.trim());
"#,
    )
    .expect("write main");

    let stdout = compile_and_run(dir.path());
    assert_eq!(stdout, "attribute-form works");
}

#[test]
fn json_import_still_works() {
    // No-regression: JSON-module imports continue to parse to a value.
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("data.json"),
        r#"{"name":"perry","count":3}"#,
    )
    .expect("write json");
    std::fs::write(
        dir.path().join("main.ts"),
        r#"
import data from "./data.json";
process.stdout.write(data.name + ":" + data.count);
"#,
    )
    .expect("write main");

    let stdout = compile_and_run(dir.path());
    assert_eq!(stdout, "perry:3");
}
