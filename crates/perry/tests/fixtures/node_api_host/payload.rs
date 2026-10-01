//! Relocation must preserve loader-relative libraries and runtime-opened data.
use super::*;

#[test]
#[cfg(unix)]
fn pruned_payload_preserves_libraries_data_and_explicit_inputs_after_relocation() {
    if !require_tool("clang") {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    let package = source.join("node_modules/fixture-addon");
    let platform = package.join("prebuilds/selected");
    std::fs::create_dir_all(&platform).unwrap();
    std::fs::write(
        source.join("package.json"),
        r#"{
      "perry": {"nativeAddons":["fixture-addon"],
        "compilePackages":["fixture-addon"],"allow":{"compilePackages":["fixture-addon"]},
        "nativeAddonFiles":{"fixture-addon":["prebuilds/selected/table.js"]}}
    }"#,
    )
    .unwrap();
    std::fs::write(
        package.join("package.json"),
        r#"{"name":"fixture-addon","version":"1","main":"index.js"}"#,
    )
    .unwrap();
    std::fs::write(platform.join("table.js"), "8523\n").unwrap();
    std::fs::write(
        platform.join("table.dat"),
        format!("42\n{}", " ".repeat(3 * 64 * 1024 + 17)),
    )
    .unwrap();
    for path in [
        "src/unused.cc",
        "deps/sqlite3.c",
        "test/test.js",
        ".github/workflows/build.yml",
        "README.md",
    ] {
        let path = package.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, vec![b'x'; 1024 * 1024]).unwrap();
    }
    std::fs::write(
        package.join("index.js"),
        "module.exports = require('./prebuilds/selected/addon.node')\n",
    )
    .unwrap();
    let foreign = package.join("prebuilds/foreign/addon.node");
    std::fs::create_dir_all(foreign.parent().unwrap()).unwrap();
    let foreign_source = source.join("foreign.c");
    std::fs::write(&foreign_source, "int ignored_foreign_binary;\n").unwrap();
    let mut clang = Command::new("clang");
    #[cfg(target_os = "macos")]
    clang.args([
        "-arch",
        if cfg!(target_arch = "aarch64") {
            "x86_64"
        } else {
            "arm64"
        },
    ]);
    #[cfg(not(target_os = "macos"))]
    clang.arg(if cfg!(target_arch = "aarch64") {
        "--target=x86_64-unknown-linux-gnu"
    } else {
        "--target=aarch64-unknown-linux-gnu"
    });
    clang.arg("-c").arg(&foreign_source).arg("-o").arg(&foreign);
    run(clang, "foreign architecture payload build");
    let mut bytes = std::fs::read(&foreign).unwrap();
    bytes.resize(1024 * 1024, 0);
    std::fs::write(&foreign, bytes).unwrap();
    let helper = source.join("helper.c");
    std::fs::write(
        &helper,
        r#"
#define _GNU_SOURCE
#include <dlfcn.h>
#include <stdio.h>
#include <string.h>
int fixture_answer(void) {
  Dl_info info;
  char path[4096];
  if (!dladdr((void *)fixture_answer, &info)) return -1;
  snprintf(path, sizeof(path), "%s", info.dli_fname);
  char *slash = strrchr(path, '/');
  if (!slash) return -2;
  strcpy(slash + 1, "table.js");
  FILE *file = fopen(path, "r");
  int result = -3;
  if (file) { fscanf(file, "%d", &result); fclose(file); }
  strcpy(slash + 1, "table.dat");
  file = fopen(path, "r");
  int data = -4;
  if (file) { fscanf(file, "%d", &data); fclose(file); }
  return data == 42 ? result : -5;
}
"#,
    )
    .unwrap();
    let library_name = if cfg!(target_os = "macos") {
        "libfixture.2.dylib"
    } else {
        "libfixture.so.2"
    };
    let library = platform.join(library_name);
    let mut clang = Command::new("clang");
    clang
        .arg("-shared")
        .arg("-fPIC")
        .arg(&helper)
        .arg("-o")
        .arg(&library);
    #[cfg(target_os = "macos")]
    clang.arg(format!("-Wl,-install_name,@loader_path/{library_name}"));
    #[cfg(not(target_os = "macos"))]
    clang.arg(format!("-Wl,-soname,{library_name}")).arg("-ldl");
    run(clang, "runtime data dependency build");
    let addon_source = source.join("addon.c");
    std::fs::write(
        &addon_source,
        format!(
            "extern int fixture_answer(void);\n{}",
            ADDON_C.replace("env, 8523, &answer", "env, fixture_answer(), &answer")
        ),
    )
    .unwrap();
    let mut clang = Command::new("clang");
    clang
        .arg("-shared")
        .arg("-fPIC")
        .arg(&addon_source)
        .arg(&library)
        .arg("-o")
        .arg(platform.join("addon.node"));
    #[cfg(target_os = "macos")]
    clang.args(["-undefined", "dynamic_lookup"]);
    #[cfg(not(target_os = "macos"))]
    clang.arg("-Wl,-rpath,$ORIGIN");
    run(clang, "dependent Node-API addon build");
    let entry = source.join("main.js");
    std::fs::write(
        &entry,
        r#"
const first = require("fixture-addon")
const second = {exports:{}}
process.dlopen(second, "fixture-addon/prebuilds/selected/addon.node")
console.log("payload", first.answer, first.add(19,23), first === second.exports)
"#,
    )
    .unwrap();
    let executable = source.join("app");
    let compiled = compile_app(&source, &entry, &executable);
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let sidecar = source.join("app.perry-native");
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(sidecar.join("manifest.json")).unwrap()).unwrap();
    let files = manifest["addons"][0]["files"].as_array().unwrap();
    assert_eq!(
        files.len(),
        5,
        "only addon, dependency, two data inputs and package.json: {files:?}"
    );
    let shipped_bytes: u64 = files
        .iter()
        .map(|file| file["size"].as_u64().unwrap())
        .sum();
    assert!(
        shipped_bytes < 512 * 1024,
        "development payload leaked: {shipped_bytes}"
    );
    let install = dir.path().join("install");
    std::fs::create_dir_all(&install).unwrap();
    let installed = install.join("app");
    std::fs::rename(executable, &installed).unwrap();
    std::fs::rename(sidecar, install.join("app.perry-native")).unwrap();
    std::fs::remove_dir_all(&source).unwrap();
    let output = run(Command::new(&installed), "relocated pruned payload");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "payload 8523 42 true"
    );
    // Same-size damage to a dependent library must fail before dlopen, too.
    let dependency = files
        .iter()
        .find(|file| file["path"].as_str().unwrap().ends_with(library_name))
        .unwrap();
    let dependency = install
        .join("app.perry-native")
        .join(dependency["path"].as_str().unwrap());
    let mut bytes = std::fs::read(&dependency).unwrap();
    bytes[0] ^= 1;
    std::fs::write(dependency, bytes).unwrap();
    let output = Command::new(&installed).output().unwrap();
    assert!(!output.status.success());
    let diagnostic = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(diagnostic.contains("SHA-256"), "{diagnostic}");
}
