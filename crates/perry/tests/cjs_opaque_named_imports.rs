//! A named import of a CommonJS module whose `module.exports` value its
//! source does not spell out (UMD `factory(...)`, a function with properties)
//! binds that property of `module.exports`, as Bun and bundlers do: the
//! module's wrap exports every name the program's importers ask of it. A
//! module whose names are all visible (`exports.X = ...`) keeps the static
//! "does not provide an export named" error.

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
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
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

fn compile(dir: &std::path::Path) -> std::process::Output {
    Command::new(perry_bin())
        .current_dir(dir)
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_RUNTIME_DIR", runtime_dir())
        .arg("compile")
        .arg(dir.join("main.ts"))
        .arg("-o")
        .arg(dir.join("main_bin"))
        .output()
        .expect("run perry compile")
}

#[test]
fn named_imports_of_opaque_commonjs_bind_module_exports_properties() {
    let dir = tempfile::tempdir().expect("tempdir");
    // The `@libpg-query/parser` shape: an ES module importing a name from a
    // UMD protobuf bundle.
    std::fs::write(
        dir.path().join("proto.js"),
        r#"(function (global, factory) {
    if (typeof module === "object" && module && module.exports)
        module.exports = factory();
})(this, function () {
    var $root = {};
    $root.pg_query = { kind: "namespace" };
    return $root;
});
"#,
    )
    .expect("write proto");
    std::fs::write(
        dir.path().join("pino.js"),
        "function pino() { return 'logger'; }\npino.levels = { info: 30 };\nmodule.exports = pino;\n",
    )
    .expect("write pino");
    std::fs::write(
        dir.path().join("parser.mjs"),
        "import { pg_query } from './proto.js';\nexport const kind = pg_query.kind;\n",
    )
    .expect("write parser");
    std::fs::write(
        dir.path().join("main.ts"),
        r#"import { kind } from "./parser.mjs";
import { levels } from "./pino.js";
console.log(kind + " " + levels.info);
"#,
    )
    .expect("write main");

    assert_eq!(compile_and_run(dir.path()), "namespace 30\n");
}

#[test]
fn visible_commonjs_exports_still_reject_absent_names() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("lib.js"), "exports.present = 1;\n").expect("write lib");
    std::fs::write(
        dir.path().join("main.ts"),
        "import { absent } from \"./lib.js\";\nconsole.log(absent);\n",
    )
    .expect("write main");

    let output = compile(dir.path());
    assert!(!output.status.success(), "compile must fail");
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("does not provide an export named 'absent'"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
