//! #10105: minimized OpenCode PluginLoader.load/report/startup contract.
//! This exercises compiled TypeScript recovery, not the full OpenTUI renderer.
use std::path::PathBuf;
use std::process::Command;

#[test]
fn configured_runtime_plugin_reports_once_and_startup_continues() {
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let perry = PathBuf::from(env!("CARGO_BIN_EXE_perry"));
    let runtime_dir = perry.parent().expect("compiler directory");
    let mut build = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    build.current_dir(&workspace).args([
        "build",
        "--locked",
        "-p",
        "perry",
        "-p",
        "perry-runtime-static",
        "-p",
        "perry-stdlib-static",
    ]);
    // Match the outer cargo invocation's profile, including --release and
    // --profile perry-dev; never silently link archives from another build.
    let profile = runtime_dir.file_name().unwrap().to_str().unwrap();
    if profile != "debug" {
        build.args(["--profile", profile]);
    }
    let build = build.output().expect("build compiler and static archives");
    assert!(
        build.status.success(),
        "static archive build failed: {}",
        String::from_utf8_lossy(&build.stderr)
    );

    let dir = tempfile::tempdir().expect("fixture directory");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        dir.path().join("bundled.ts"),
        "export const ready = 'bundled provider ready';\n",
    )
    .unwrap();
    std::fs::write(
        &entry,
        r#"
// Mirrors PluginLoader.load's catch-and-return and the caller's single report.
async function load(row: any): Promise<any> {
  try {
    const mod = await import(row.entry);
    return { ok: true, mod };
  } catch (error) {
    return { ok: false, error };
  }
}

const config = JSON.parse(process.argv[2]);
for (const spec of config.plugin) {
  const result = await load({ entry: spec });
  if (!result.ok) {
    console.error('[tui.plugin] failed to load tui plugin: ' + result.error.message);
    console.log('plugin error code: ' + result.error.code);
  } else {
    console.log('unexpected plugin success');
  }
}

// The same computed import site still accepts native builtins after a rejection.
const builtin = await load({ entry: 'node:os' });
console.log('builtin ready: ' + builtin.ok);
const bundled = await import('./bundled.ts');
console.log(bundled.ready);
console.log('startup continued');
"#,
    )
    .unwrap();
    let executable = dir
        .path()
        .join(if cfg!(windows) { "main.exe" } else { "main" });
    let mut compile = Command::new(&perry);
    compile
        .current_dir(dir.path())
        .args(["compile", "--no-cache"])
        .arg(&entry)
        .arg("-o")
        .arg(&executable)
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_RUNTIME_DIR", runtime_dir);
    // #7354: LLVM's statepoint pass cannot process Windows catchpad EH.
    // Exercise the supported shadow-frame path there; keep other hosts' GC
    // defaults so this does not conceal an import/recovery regression.
    if cfg!(windows) {
        compile.env("PERRY_RS4GC", "0");
    }
    let compiled = compile.output().expect("compile fixture");
    assert!(
        compiled.status.success(),
        "compile failed: {}",
        String::from_utf8_lossy(&compiled.stderr)
    );

    // Even an installed plugin is outside the AOT graph when added after
    // compilation. The diagnostic must not suggest another npm install.
    let package_dir = dir.path().join("node_modules/some-npm-plugin");
    std::fs::create_dir_all(&package_dir).unwrap();
    std::fs::write(
        package_dir.join("package.json"),
        r#"{"name":"some-npm-plugin","type":"module","main":"index.js"}"#,
    )
    .unwrap();
    std::fs::write(
        package_dir.join("index.js"),
        "export const plugin = true;\n",
    )
    .unwrap();

    let run = Command::new(&executable)
        .current_dir(dir.path())
        .arg(r#"{"plugin":["some-npm-plugin"]}"#)
        .output()
        .expect("run fixture");
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(run.status.success(), "startup failed: {stderr}");
    assert_eq!(
        String::from_utf8_lossy(&run.stdout).replace("\r\n", "\n"),
        "plugin error code: ERR_MODULE_NOT_FOUND\nbuiltin ready: true\nbundled provider ready\nstartup continued\n"
    );
    assert_eq!(
        stderr.lines().count(),
        1,
        "one application report: {stderr}"
    );
    for expected in [
        "[tui.plugin] failed to load tui plugin:",
        "some-npm-plugin",
        "not available in this native build",
        "Bun/Node distribution",
        "statically resolvable import()",
        "main.ts:",
    ] {
        assert!(stderr.contains(expected), "missing {expected:?}: {stderr}");
    }

    let without_plugins = Command::new(&executable)
        .current_dir(dir.path())
        .arg(r#"{"plugin":[]}"#)
        .output()
        .expect("run without plugins");
    assert!(without_plugins.status.success());
    assert!(without_plugins.stderr.is_empty());
    assert_eq!(
        String::from_utf8_lossy(&without_plugins.stdout).replace("\r\n", "\n"),
        "builtin ready: true\nbundled provider ready\nstartup continued\n"
    );
}
