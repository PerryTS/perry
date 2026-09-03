//! #9598 — compile source extracted from Bun standalone executables without a
//! host `/$bunfs` mount, while preserving Bun's original runtime path strings.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn write_fixture(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("assets")).expect("mkdir assets");
    std::fs::create_dir_all(root.join("node_modules/fixture-pkg")).expect("mkdir nested package");
    std::fs::write(
        root.join("entry.js"),
        r#"
import { answer, token as virtualToken } from "/$bunfs/root/chunk.js";
import { token as realToken } from "./chunk.js";
import { reexported } from "/$bunfs/root/barrel.js";
import { nestedValue } from "/$bunfs/root/node_modules/fixture-pkg/value.js";
import { readFileSync } from "node:fs";

const required = require("/$bunfs/root/required.js");
const dynamicModule = await import("/$bunfs/root/dynamic.js");
const assetPath = "/$bunfs/root/assets/help.zst";
const bunFile = Bun.file(assetPath);

console.log([
  answer,
  reexported,
  required.required,
  dynamicModule.dynamicValue,
  nestedValue,
  virtualToken === realToken,
  readFileSync(assetPath).length,
  await bunFile.text(),
  bunFile.size,
  await bunFile.exists(),
].join("|"));
"#,
    )
    .expect("write entry");
    std::fs::write(
        root.join("chunk.js"),
        "export const answer = 42; export const token = {};\n",
    )
    .expect("write chunk");
    std::fs::write(
        root.join("barrel.js"),
        "export { reexported } from \"/$bunfs/root/reexported.js\";\n",
    )
    .expect("write barrel");
    std::fs::write(root.join("reexported.js"), "export const reexported = 7;\n")
        .expect("write re-export");
    std::fs::write(root.join("required.js"), "export const required = 9;\n")
        .expect("write required");
    std::fs::write(root.join("dynamic.js"), "export const dynamicValue = 11;\n")
        .expect("write dynamic");
    std::fs::write(
        root.join("node_modules/fixture-pkg/value.js"),
        "export const nestedValue = 13;\n",
    )
    .expect("write nested package module");
    std::fs::write(root.join("assets/help.zst"), b"HELP").expect("write asset");
}

#[test]
fn resolves_modules_and_reads_assets_after_extracted_root_moves() {
    let dir = tempfile::tempdir().expect("tempdir");
    let project = dir.path();
    let root = project.join("root");
    write_fixture(&root);
    std::fs::write(
        project.join("package.json"),
        r#"{"name":"bunfs-root-regression","private":true}"#,
    )
    .expect("write package.json");

    let output = project.join("app");
    let compile = Command::new(perry_bin())
        .current_dir(project)
        .arg("compile")
        .arg("--bunfs-root")
        .arg(&root)
        .arg(root.join("entry.js"))
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

    let compile_stdout = String::from_utf8_lossy(&compile.stdout);
    let manifest_path = compile_stdout
        .lines()
        .find_map(|line| line.strip_prefix("Asset manifest: "))
        .expect("compile output names the asset manifest");
    let manifest = std::fs::read_to_string(manifest_path).expect("read asset manifest");
    assert!(
        manifest.contains(r#""packaged_path": "/$bunfs/root/assets/help.zst""#),
        "manifest did not preserve the Bun virtual path:\n{manifest}"
    );
    assert!(
        !manifest.contains("$perryfs//$bunfs/root/assets/help.zst"),
        "manifest incorrectly rebased the Bun virtual path:\n{manifest}"
    );

    std::fs::rename(&root, project.join("root-moved-away")).expect("move extracted root");
    let run = Command::new(&output)
        .current_dir(project)
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
        "42|7|9|11|13|true|4|HELP|4|true\n"
    );
}

#[test]
fn missing_virtual_module_names_mapping_in_diagnostic() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("root");
    std::fs::create_dir_all(&root).expect("mkdir root");
    let entry = root.join("entry.js");
    std::fs::write(
        &entry,
        "import { missing } from \"/$bunfs/root/missing.js\"; console.log(missing);\n",
    )
    .expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg("--bunfs-root")
        .arg(&root)
        .arg(&entry)
        .arg("-o")
        .arg(dir.path().join("app"))
        .output()
        .expect("run perry compile");
    assert!(!compile.status.success(), "missing mapped module compiled");
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(stderr.contains("/$bunfs/root/missing.js"), "{stderr}");
    assert!(
        stderr.contains(&root.join("missing.js").display().to_string()),
        "{stderr}"
    );
    assert!(stderr.contains("--bunfs-root"), "{stderr}");
}
