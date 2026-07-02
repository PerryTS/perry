//! #5731 — embed static assets/files into standalone executables.
//!
//! `perry compile --embed "./dist/**"` bakes matched files into the binary.
//! At runtime they are reachable three ways, all exercised here:
//!   * `import { embeddedFiles } from "perry"` — `{ name, size, type }` per asset
//!   * `import { readEmbedded } from "perry"` — bytes as a `Buffer`
//!   * `node:fs` (`readFileSync` / `existsSync`) via the `$perryfs/<path>` path
//! plus `isStandaloneExecutable` (always `true` in a compiled binary).
//!
//! Asset embedding is host-only (Unix-like): it compiles a `cc` object that
//! MSVC `link.exe` can't consume, so the feature errors on a Windows host and
//! this end-to-end test is skipped there.
#![cfg(not(windows))]

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn embeds_assets_and_reads_them_back() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("dist/assets")).expect("mkdir dist/assets");
    std::fs::write(root.join("dist/index.html"), b"HELLO_EMBED").expect("write index.html");
    std::fs::write(root.join("dist/assets/app.js"), b"console.log(1)").expect("write app.js");

    let entry = root.join("main.ts");
    std::fs::write(
        &entry,
        r#"
import { embeddedFiles, readEmbedded, isStandaloneExecutable } from "perry";
import * as fs from "fs";

console.log("standalone:", isStandaloneExecutable);
const files = embeddedFiles();
console.log("count:", files.length);
console.log("names:", files.map(f => f.name).sort().join(","));
console.log("readEmbedded:", readEmbedded("dist/index.html").toString());
console.log("viaFs:", fs.readFileSync("$perryfs/dist/index.html", "utf8"));
const html = files.find(f => f.name === "dist/index.html");
console.log("type:", html.type, "size:", html.size);
console.log("exists:", fs.existsSync("$perryfs/dist/assets/app.js"));
console.log("existsMissing:", fs.existsSync("$perryfs/nope.txt"));
try { readEmbedded("nope.txt"); console.log("throwMissing: no"); }
catch (e) { console.log("throwMissing: yes"); }
"#,
    )
    .expect("write entry");

    let output = root.join("app");
    let compile = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(&entry)
        .arg("--embed")
        .arg("./dist/**")
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
        .current_dir(root)
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert_eq!(
        stdout,
        "standalone: true\n\
         count: 2\n\
         names: dist/assets/app.js,dist/index.html\n\
         readEmbedded: HELLO_EMBED\n\
         viaFs: HELLO_EMBED\n\
         type: text/html; charset=utf-8 size: 11\n\
         exists: true\n\
         existsMissing: false\n\
         throwMissing: yes\n",
        "unexpected runtime output"
    );
}
