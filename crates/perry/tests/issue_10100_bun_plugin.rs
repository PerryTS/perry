//! Bun's inert runtime plugin registrar must let OpenTUI finish module init.

use std::process::Command;

fn compile_and_run(source: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let entry = dir.path().join("main.ts");
    let binary = dir
        .path()
        .join(if cfg!(windows) { "main.exe" } else { "main" });
    std::fs::write(&entry, source).unwrap();
    let mut compiler = Command::new(env!("CARGO_BIN_EXE_perry"));
    // LLVM's statepoint pass rejects Windows exception funclets (#7354).
    if cfg!(windows) {
        compiler.env("PERRY_RS4GC", "0");
    }
    let output = compiler
        .current_dir(dir.path())
        .args(["compile", "--no-cache"])
        .arg(&entry)
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "compile failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = Command::new(binary)
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "run failed: {:?}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .replace("\r\n", "\n")
}

#[test]
fn setup_is_synchronous_and_hooks_are_inert() {
    let stdout = compile_and_run(
        r#"
import { plugin } from "bun";
import * as bun from "bun";
let calls = 0;
let hooks = 0;
const result = plugin({ name: "x", setup(build) {
  calls++;
  console.log("receiver", this.name);
  build.onLoad({ filter: /a/ }, () => { hooks++; return {}; });
  build.onResolve({ filter: /a/ }, () => { hooks++; return {}; });
  build.onStart(() => { hooks++; });
  build.onEnd(() => { hooks++; });
  build.module("virtual:x", () => { hooks++; return {}; });
  build.config.target = "bun";
  console.log("config", build.config.target);
  return 42;
} });
console.log("sync", calls, hooks, result === undefined);
Bun.plugin((build) => { build.onLoad({ filter: /x/ }, () => ({})); calls++; });
bun.plugin({ name: "namespace", setup() { calls++; } });
const saved = Bun.plugin;
saved({ name: "value", setup() { calls++; } });
console.log("clear type", typeof plugin.clearAll, typeof Bun.plugin.clearAll);
console.log("export", Object.keys(bun).includes("plugin"), saved === plugin, saved.clearAll === plugin.clearAll);
console.log("clear", plugin.clearAll() === undefined, Bun.plugin.clearAll() === undefined, saved.clearAll() === undefined, bun.plugin.clearAll() === undefined);
console.log("count", calls, "detect", typeof Bun);
function shadowed() {
  const Bun = { plugin(fn) { fn("local"); } };
  Bun.plugin((value) => console.log("shadow", value));
}
shadowed();
"#,
    );
    assert_eq!(stdout, "receiver x\nconfig bun\nsync 1 0 true\nclear type function function\nexport true true true\nclear true true true true\ncount 4 detect undefined\nshadow local\n");
}

#[test]
fn async_setup_and_errors_propagate() {
    let stdout = compile_and_run(
        r#"
import { plugin } from "bun";
let phase = 0;
const pending = plugin({ name: "async", async setup(build) {
  phase = 1;
  await Promise.resolve();
  build.onLoad({ filter: /x/ }, () => { throw new Error("loader ran"); });
  phase = 2;
  return 42;
} });
console.log("before", phase, pending instanceof Promise);
console.log("await", await pending, phase);
try { plugin({ name: "throw", setup() { throw new Error("setup failed"); } }); }
catch (e) { console.log("throw", e.message); }
try { await plugin({ name: "reject", async setup() { await Promise.resolve(); throw new Error("async failed"); } }); }
catch (e) { console.log("reject", e.message); }
try { plugin({ name: "bad", setup: 123 }); }
catch (e) { console.log("invalid", e.name); }
plugin(() => console.log("recovered"));
"#,
    );
    assert_eq!(stdout, "before 1 true\nawait 42 2\nthrow setup failed\nreject async failed\ninvalid TypeError\nrecovered\n");
}

#[test]
fn opentui_install_state_survives_repeated_calls() {
    // Reduced from @opentui/solid 0.4.5's solid-plugin.js and
    // runtime-plugin-support-configure.js. Keep the actual Symbol.for keys,
    // nullish initialization, setup closure and module-init call ordering.
    let stdout = compile_and_run(
        r#"
import { plugin as registerBunPlugin } from "bun";
const solidTransformStateKey = Symbol.for("opentui.solid.transform");
const runtimePluginSupportInstalledKey = Symbol.for("opentui.solid.runtime-plugin-support");
const getSolidTransformState = () => {
  const state = globalThis;
  state[solidTransformStateKey] ??= { installed: false };
  return state[solidTransformStateKey];
};
function ensureSolidTransformPlugin() {
  const state = getSolidTransformState();
  if (state.installed) return false;
  registerBunPlugin({ name: "bun-plugin-solid", setup: (build) => {
    build.onLoad({ filter: /\.[jt]sx(?:[?#].*)?$/ }, async () => {
      throw new Error("runtime transform must not run");
    });
  } });
  state.installed = true;
  return true;
}
function ensureRuntimePluginSupport() {
  const state = globalThis;
  const install = state[runtimePluginSupportInstalledKey];
  if (install) return false;
  ensureSolidTransformPlugin();
  registerBunPlugin({ name: "opentui-runtime", setup(build) {
    build.onResolve({ filter: /^@opentui\// }, () => { throw new Error("resolver ran"); });
    build.onLoad({ filter: /.*/, namespace: "opentui" }, () => { throw new Error("loader ran"); });
  } });
  state[runtimePluginSupportInstalledKey] = { installed: true };
  return true;
}
console.log("install", ensureRuntimePluginSupport(), ensureRuntimePluginSupport());
console.log("solid", getSolidTransformState().installed, ensureSolidTransformPlugin());
console.log("past TUI runtime module init");
"#,
    );
    assert_eq!(
        stdout,
        "install true false\nsolid true false\npast TUI runtime module init\n"
    );
}

#[test]
fn dynamic_namespaces_expose_plugin_and_clear_all() {
    let stdout = compile_and_run(
        r#"
async function main() {
const spec = "bun";
const bun = await import(spec);
bun.plugin({ name: "dynamic", setup() { console.log("dynamic setup"); } });
console.log("dynamic clear", bun.plugin.clearAll() === undefined);
const required = require("bun");
const register = required.plugin;
register(() => console.log("required setup"));
console.log("required clear", register.clearAll() === undefined);
}
main();
"#,
    );
    assert_eq!(
        stdout,
        "dynamic setup\ndynamic clear true\nrequired setup\nrequired clear true\n"
    );
}

#[test]
fn deferred_jsx_import_explains_native_transform_boundary() {
    let stdout = compile_and_run(
        r#"
import { plugin } from "bun";
plugin({ name: "jsx", setup(build) { build.onLoad({ filter: /jsx|tsx/ }, () => ({})); } });
async function load(specifier: string) {
  try { await import(specifier); }
  catch (error) {
    console.log(error.code, error.message.includes("runtime transform"),
      error.message.includes("native build does not include"), error.message.includes("main.ts:"));
  }
}
await load("./user-plugin.tsx");
await load("file:///plugins/user.jsx?version=1#entry");
await load("./ordinary.js");
"#,
    );
    assert_eq!(stdout, "ERR_MODULE_NOT_FOUND true true true\nERR_MODULE_NOT_FOUND true true true\nERR_MODULE_NOT_FOUND false false true\n");
}
