//! `--module-alias <specifier>=<file>` resolves a virtual module specifier to a
//! source file, as a Bun build plugin's onResolve hook does (OpenCode v2
//! imports `virtual:opencode-app-assets`). Both a static import and a dynamic
//! `import()` of the specifier must bind the same module instance.
//! `--module-alias <module-file>=<file>` replaces a module's source, as an
//! onLoad hook does; the replacement's imports resolve from the module file
//! it replaces.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn write_project(root: &std::path::Path) -> PathBuf {
    std::fs::write(
        root.join("package.json"),
        r#"{ "name": "module-alias-consumer", "private": true, "type": "module" }"#,
    )
    .expect("write package.json");
    std::fs::create_dir_all(root.join("gen")).expect("mkdir gen");
    std::fs::write(
        root.join("gen").join("assets.gen.ts"),
        r#"import { readFileSync } from "node:fs";
import archive from "../data.bin" with { type: "file" };
export default () => readFileSync(archive);
"#,
    )
    .expect("write generated module");
    std::fs::write(root.join("data.bin"), b"archive-bytes").expect("write data.bin");
    let entry = root.join("main.ts");
    std::fs::write(
        &entry,
        r#"import load from "virtual:test-assets";
const dynamic = await import("virtual:test-assets");
console.log(load().toString(), dynamic.default === load);
"#,
    )
    .expect("write entry");
    entry
}

#[test]
fn module_alias_resolves_static_and_dynamic_imports() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let entry = write_project(root);
    let output = root.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--module-alias")
        .arg("virtual:test-assets=gen/assets.gen.ts")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    // Run from elsewhere: the aliased module's file import is embedded.
    let elsewhere = tempfile::tempdir().expect("tempdir");
    let run = Command::new(&output)
        .current_dir(elsewhere.path())
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "archive-bytes true\n");
}

#[test]
fn module_alias_rejects_missing_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let entry = write_project(root);
    let compile = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(root.join("main_bin"))
        .arg("--module-alias")
        .arg("virtual:test-assets=gen/missing.ts")
        .output()
        .expect("run perry compile");
    assert!(
        !compile.status.success(),
        "a missing alias file must fail the compile"
    );
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(
        stderr.contains("--module-alias file for `virtual:test-assets` was not found"),
        "stderr:\n{stderr}"
    );
}

#[test]
fn module_file_alias_replaces_source_and_keeps_its_import_base() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(
        root.join("package.json"),
        r#"{ "name": "module-file-alias", "private": true, "type": "module" }"#,
    )
    .expect("write package.json");
    std::fs::create_dir_all(root.join("lib")).expect("mkdir lib");
    std::fs::create_dir_all(root.join("gen")).expect("mkdir gen");
    std::fs::write(
        root.join("lib").join("impl.ts"),
        "export const value: string = \"original\";\n",
    )
    .expect("write impl.ts");
    std::fs::write(
        root.join("lib").join("helper.ts"),
        "export const helper = \"helper-next-to-impl\";\n",
    )
    .expect("write helper.ts");
    // `./helper.ts` exists only next to the replaced module, not in gen/.
    std::fs::write(
        root.join("gen").join("impl.gen.ts"),
        "import { helper } from \"./helper.ts\";\nexport const value: string = \"replaced \" + helper;\n",
    )
    .expect("write impl.gen.ts");
    let entry = root.join("main.ts");
    std::fs::write(
        &entry,
        "import { value } from \"./lib/impl.ts\";\nconsole.log(value);\n",
    )
    .expect("write entry");
    let output = root.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--module-alias")
        .arg("./lib/impl.ts=gen/impl.gen.ts")
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
        "replaced helper-next-to-impl\n"
    );
}
