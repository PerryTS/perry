//! #5731 — embed static assets/files into standalone executables.
//!
//! `perry compile --embed "./dist/**"` bakes matched files into the binary.
//! At runtime they are reachable three ways, all exercised here:
//!   * `import { embeddedFiles } from "perry"` — `{ name, size, type }` per asset
//!   * `import { readEmbedded } from "perry"` — bytes as a `Buffer`
//!   * `node:fs` reads, existence, metadata, and directory listing through the
//!     `$perryfs/<path>` virtual filesystem
//! plus `isStandaloneExecutable` (always `true` in a compiled binary).
//!
//! Asset embedding is host-targeted: Unix-like systems compile a `cc` object;
//! Windows uses clang's COFF output and a `.CRT$XCU` startup initializer.
//!
//! The SEA checks below follow Node v26.12.1 and v27.0.0-alpha.0.0.0's tagged
//! `lib/sea.js` contract. Keep these pins beside the behavioral regression so
//! later API changes can be compared against the exact upstream implementation.
//! <https://github.com/nodejs/node/blob/v26.12.1/lib/sea.js>
//! <https://github.com/nodejs/node/blob/v27.0.0-alpha.0.0.0/lib/sea.js>

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
        .expect("canonical workspace")
}

fn runtime_dir() -> PathBuf {
    static BUILD_RUNTIME: Once = Once::new();
    let target = std::env::var_os("PERRY_RUNTIME_DIR")
        .or_else(|| std::env::var_os("CARGO_TARGET_DIR"))
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join("target"));
    let target = if target.is_absolute() {
        target
    } else {
        workspace_root().join(target)
    };
    if std::env::var_os("PERRY_RUNTIME_DIR").is_some() {
        return target;
    }
    BUILD_RUNTIME.call_once(|| {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        // Build the compiler and its runtime archives together so Cargo
        // unifies the runtime features used by Perry. Building only the two
        // staticlib wrappers leaves an incomplete export set that the GC
        // sidecar analyzer correctly rejects. Avoid building unrelated
        // optional extensions in this focused integration test.
        let build = Command::new(cargo)
            .current_dir(workspace_root())
            // `build-perry` is the repository's coherent prebuilt runtime
            // graph. The wrapper-only package set leaves many runtime exports
            // out, which the sidecar analyzer must reject. Add the analyzer
            // binary to the same Cargo graph so its static archive also has
            // the matching features.
            .args([
                "build-perry",
                "--profile",
                "perry-dev",
                "-p",
                "perry-gc-effects",
            ])
            // The sidecar analyzer relies on the same unwind-table build
            // setting as Perry's GC workflow to discover the runtime call
            // graph. Without it, the archive passes compilation but yields
            // only one analyzed export.
            .env("RUSTFLAGS", "-C force-unwind-tables=yes")
            .output()
            .expect("build perry runtime archive");
        assert!(
            build.status.success(),
            "runtime archive build failed\n{}",
            String::from_utf8_lossy(&build.stderr)
        );
        let profile_target = target.join("perry-dev");
        let sidecars = Command::new(profile_target.join("perry-gc-effects"))
            .current_dir(workspace_root())
            .arg("sidecars")
            .arg(&profile_target)
            .output()
            .expect("generate runtime GC sidecar");
        assert!(
            sidecars.status.success(),
            "runtime sidecar generation failed\n{}",
            String::from_utf8_lossy(&sidecars.stderr)
        );
    });
    target.join("perry-dev")
}

#[test]
fn embeds_assets_and_reads_them_back() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).expect("mkdir src");
    std::fs::create_dir_all(root.join("dist/assets")).expect("mkdir dist/assets");
    std::fs::write(root.join("dist/index.html"), b"HELLO_EMBED").expect("write index.html");
    std::fs::write(root.join("dist/assets/app.js"), b"console.log(1)").expect("write app.js");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"embed-regression","private":true}"#,
    )
    .expect("write package.json");

    // Keep the entry below the package root. The original regression used
    // `src/embed_probe.ts` with `--embed ./drizzle/**`: resolving patterns
    // relative to the entry directory silently searched `src/drizzle` and
    // embedded zero files.
    let entry = root.join("src/main.ts");
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
console.log("existsDir:", fs.existsSync("$perryfs/dist/assets"));
console.log("existsMissing:", fs.existsSync("$perryfs/nope.txt"));
const embeddedStat = fs.statSync("$perryfs/dist/assets/app.js");
console.log("stat:", embeddedStat.isFile(), embeddedStat.size);
console.log("statDir:", fs.statSync("$perryfs/dist/assets").isDirectory());
console.log("rootEntries:", fs.readdirSync("$perryfs").join(","));
console.log("distEntries:", fs.readdirSync("$perryfs/dist").join(","));
const assetEntry = fs.readdirSync("$perryfs/dist", { withFileTypes: true })[0];
console.log("dirent:", assetEntry.name, assetEntry.isDirectory(), assetEntry.parentPath);
try { readEmbedded("nope.txt"); console.log("throwMissing: no"); }
catch (e) { console.log("throwMissing: yes"); }
"#,
    )
    .expect("write entry");

    let output = root.join("app");
    let mut compile_command = Command::new(perry_bin());
    compile_command
        .current_dir(root)
        .env("PERRY_RUNTIME_DIR", runtime_dir())
        .arg("compile")
        .arg(&entry)
        .arg("--embed")
        .arg("./dist/**")
        .arg("-o")
        .arg(&output);
    let compile = compile_command.output().expect("run perry compile");
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
         existsDir: true\n\
         existsMissing: false\n\
         stat: true 14\n\
         statDir: true\n\
         rootEntries: dist\n\
         distEntries: assets,index.html\n\
         dirent: assets true $perryfs/dist\n\
         throwMissing: yes\n",
        "unexpected runtime output"
    );
}

#[test]
fn sea_profile_supports_relocated_assets_and_zero_asset_builds() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("assets")).expect("mkdir assets");
    std::fs::write(root.join("assets/message.txt"), b"SEA_OK").expect("write asset");
    std::fs::write(root.join("assets/a"), b"A\xEF\xBB\xBFtext").expect("write short-key asset");
    std::fs::write(root.join("assets/bom"), b"\xEF\xBB\xBFhello").expect("write BOM asset");
    std::fs::write(root.join("assets/bin"), [1u8, 2, 3]).expect("write binary asset");
    let entry = root.join("main.ts");
    std::fs::write(
        &entry,
        r#"
import { isSea, getAsset, getAssetKeys, getAssetAsBlob } from "node:sea";
import { resolveObjectURL } from "node:buffer";
import { createObjectURL } from "node:url";
const bytes = getAsset("assets/message.txt") as ArrayBuffer;
const decoder = new TextDecoder();
console.log(isSea(), getAssetKeys().join(","), decoder.decode(bytes));
console.log("full key and encoding", getAsset("assets/a", "utf8"));
console.log("BOM removed", getAsset("assets/bom", "utf-8"));
const first = new Uint8Array(getAsset("assets/bin") as ArrayBuffer);
first[0] = 99;
console.log("fresh copy", new Uint8Array(getAsset("assets/bin") as ArrayBuffer)[0]);
const keys = getAssetKeys(); keys.pop(); console.log("fresh keys", getAssetKeys().length);
console.log("blob type", getAssetAsBlob("assets/a", { type: "TEXT/PLAIN" }).type);
console.log("blob invalid type", getAssetAsBlob("assets/a", { type: "x\u0001y" }).type);
console.log("blob coerces type", getAssetAsBlob("assets/a", { type: 42 }).type);
const seaBlob = getAssetAsBlob("assets/bin", { type: "application/octet-stream" });
const seaBlobUrl = createObjectURL(seaBlob);
const resolvedSeaBlob = resolveObjectURL(seaBlobUrl)!;
console.log("blob URL roundtrip", resolvedSeaBlob.size, resolvedSeaBlob.type);
function errorCode(fn: () => unknown): string { try { fn(); return "none"; } catch (e) { return (e as any).code; } }
console.log("encoding type order", errorCode(() => getAsset(1 as any, 1 as any)));
console.log("key type", errorCode(() => getAsset(1 as any)));
console.log("missing key before label", errorCode(() => getAsset("missing", "not-an-encoding")));
console.log("bad encoding", errorCode(() => getAsset("assets/a", "not-an-encoding")));
console.log("exact key", errorCode(() => getAsset("./assets/a")));
try { getAsset("missing.txt"); }
catch (error) { console.log((error as any).code); }
"#,
    )
    .expect("write entry");
    let output = root.join("app");
    let compile = Command::new(perry_bin())
        .current_dir(root)
        .env("PERRY_RUNTIME_DIR", runtime_dir())
        .args(["compile"])
        .arg(&entry)
        .args(["--sea", "--embed", "./assets/**", "-o"])
        .arg(&output)
        .output()
        .expect("compile SEA app");
    assert!(
        compile.status.success(),
        "SEA compile failed\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let relocated_dir = tempfile::tempdir().expect("relocation tempdir");
    let relocated = relocated_dir.path();
    let moved = relocated.join("app");
    std::fs::rename(&output, &moved).expect("relocate executable");
    drop(dir);
    let run = Command::new(&moved)
        .current_dir(&relocated)
        .output()
        .expect("run relocated SEA app");
    assert!(
        run.status.success(),
        "SEA app failed\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "true assets/a,assets/bin,assets/bom,assets/message.txt SEA_OK\n\
full key and encoding A\u{feff}text\n\
BOM removed hello\n\
fresh copy 1\n\
fresh keys 4\n\
blob type text/plain\n\
blob invalid type \n\
blob coerces type 42\n\
encoding type order ERR_INVALID_ARG_TYPE\n\
key type ERR_INVALID_ARG_TYPE\n\
missing key before label ERR_SINGLE_EXECUTABLE_APPLICATION_ASSET_NOT_FOUND\n\
bad encoding ERR_ENCODING_NOT_SUPPORTED\n\
exact key ERR_SINGLE_EXECUTABLE_APPLICATION_ASSET_NOT_FOUND\n\
ERR_SINGLE_EXECUTABLE_APPLICATION_ASSET_NOT_FOUND\n"
    );

    let empty_dir = tempfile::tempdir().expect("empty app tempdir");
    let empty_root = empty_dir.path();
    std::fs::create_dir_all(empty_root).expect("create empty project");
    let empty_entry = empty_root.join("main.ts");
    std::fs::write(&empty_entry, "import { isSea, getAssetKeys } from 'node:sea'; console.log(isSea(), getAssetKeys().length);\n").expect("write empty entry");
    let empty_output = empty_root.join("app");
    let compile = Command::new(perry_bin())
        .current_dir(empty_root)
        .env("PERRY_RUNTIME_DIR", runtime_dir())
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .args(["compile"])
        .arg(&empty_entry)
        .args(["--sea", "-o"])
        .arg(&empty_output)
        .output()
        .expect("compile zero-asset SEA app");
    assert!(
        compile.status.success(),
        "empty SEA compile failed\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(&empty_output)
        .output()
        .expect("run zero-asset SEA app");
    assert!(
        run.status.success(),
        "empty SEA app failed\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "true 0\n");
}
