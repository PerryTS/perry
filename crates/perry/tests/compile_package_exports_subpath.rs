//! A compile package's subpath import resolves through its package.json
//! `exports` map, as in Node and Bun, even when a sibling file is named like
//! the subpath. `@opencode/tui` maps `./mini/tool` to
//! `./src/mini/tool.public.ts`, which re-exports from `./tool.ts` and adds
//! `nonEmptyToolContent`; Perry resolved `src/mini/tool.ts` instead and the
//! importer failed to link ("does not provide an export named").
//!
//! The fixture has the workspace shape (`node_modules/@fx/tui` is a symlink
//! to `packages/tui`), runs unchanged under `node` (type stripping) and
//! `bun`, and prints the expected line asserted below.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[cfg(unix)]
#[test]
fn compile_package_subpath_follows_exports_over_same_named_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "exports-subpath-consumer",
  "private": true,
  "type": "module",
  "perry": {
    "compilePackages": ["@fx/tui"],
    "allow": { "compilePackages": ["@fx/tui"] }
  }
}"#,
    )
    .expect("write consumer package.json");

    let pkg = root.join("packages").join("tui");
    std::fs::create_dir_all(pkg.join("src").join("mini")).expect("mkdir src/mini");
    std::fs::create_dir_all(root.join("node_modules").join("@fx")).expect("mkdir node_modules/@fx");
    std::os::unix::fs::symlink("../../packages/tui", root.join("node_modules/@fx/tui"))
        .expect("link workspace package");
    std::fs::write(
        pkg.join("package.json"),
        r#"{
  "name": "@fx/tui",
  "version": "1.0.0",
  "private": true,
  "type": "module",
  "exports": {
    ".": "./src/index.ts",
    "./mini/tool": "./src/mini/tool.public.ts"
  }
}"#,
    )
    .expect("write @fx/tui package.json");
    std::fs::write(
        pkg.join("src").join("index.ts"),
        "export const root = \"root\";\n",
    )
    .expect("write index.ts");
    std::fs::write(
        pkg.join("src").join("mini").join("tool.ts"),
        "export const toolName = (name: string): string => \"tool:\" + name;\n",
    )
    .expect("write tool.ts");
    std::fs::write(
        pkg.join("src").join("mini").join("tool.public.ts"),
        r#"export { toolName } from "./tool.ts";
export const nonEmptyToolContent = (text: string): string | undefined =>
  text.trim().length > 0 ? text : undefined;
"#,
    )
    .expect("write tool.public.ts");

    let entry = root.join("main.ts");
    std::fs::write(
        &entry,
        r#"import { nonEmptyToolContent, toolName } from "@fx/tui/mini/tool";
import { root } from "@fx/tui";
console.log(root, toolName("read"), nonEmptyToolContent("  "), nonEmptyToolContent("ok"));
"#,
    )
    .expect("write entry");

    let output = root.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed (exports subpath resolved to the same-named file?)\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output).output().expect("run compiled binary");
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        stdout,
        String::from_utf8_lossy(&run.stderr)
    );
    // node main.ts / bun main.ts print exactly this line.
    assert_eq!(stdout, "root tool:read undefined ok\n");
}
