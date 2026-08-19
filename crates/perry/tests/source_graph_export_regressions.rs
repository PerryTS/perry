//! Cross-module export shapes found while compiling OpenCode's TypeScript
//! source graph. Each regression previously produced an undefined native
//! linker symbol despite all source modules lowering successfully.

use std::path::{Path, PathBuf};
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
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join("target"));
    if cfg!(windows) {
        target.join("x86_64-pc-windows-msvc").join("debug")
    } else {
        target.join("debug")
    }
}

fn ensure_runtime_archive() {
    static BUILD_RUNTIME: Once = Once::new();
    BUILD_RUNTIME.call_once(|| {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let mut command = Command::new(cargo);
        command
            .current_dir(workspace_root())
            .arg("build")
            .arg("-p")
            .arg("perry-runtime-static")
            .arg("-p")
            .arg("perry-stdlib-static");
        if cfg!(windows) {
            command.arg("--target").arg("x86_64-pc-windows-msvc");
        }
        let build = command
            .output()
            .expect("run cargo build of static runtime archives");
        assert!(
            build.status.success(),
            "cargo build of static runtime archives failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&build.stdout),
            String::from_utf8_lossy(&build.stderr)
        );
    });
}

fn runtime_dir() -> PathBuf {
    ensure_runtime_archive();
    target_debug_dir()
}

fn write(dir: &Path, name: &str, source: &str) {
    std::fs::write(dir.join(name), source).expect("write fixture");
}

fn compile_and_run(dir: &Path, entry: &str) -> String {
    let output = dir.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(entry)
        .arg("--no-cache")
        .arg("-o")
        .arg(&output)
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_RUNTIME_DIR", runtime_dir())
        .output()
        .expect("run perry compile");
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
        "compiled binary failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

#[test]
fn namespace_reexport_survives_export_all_barrels() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "value.ts", "export const answer = 42;\n");
    write(
        dir.path(),
        "namespace.ts",
        "export * as Values from './value';\n",
    );
    write(dir.path(), "barrel.ts", "export * from './namespace';\n");
    write(
        dir.path(),
        "main.ts",
        "import { Values } from './barrel'; console.log(Values.answer);\n",
    );

    assert_eq!(compile_and_run(dir.path(), "main.ts"), "42\n");
}

#[test]
fn whole_type_only_import_does_not_wrap_same_named_runtime_builtin() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "types.ts",
        "const Array_ = 123; export { Array_ as Array };\n",
    );
    write(
        dir.path(),
        "main.ts",
        "import type { Array } from './types'; console.log(Array.isArray([1]));\n",
    );

    assert_eq!(compile_and_run(dir.path(), "main.ts"), "true\n");
}

#[test]
fn type_only_interface_dispatch_uses_runtime_class_registry() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "driver.ts",
        "export interface Driver { greet(name: string): string; }\n",
    );
    write(
        dir.path(),
        "consumer.ts",
        "import type { Driver } from './driver';\n\
         export function consume(driver: Driver) {\n\
           const greet = driver.greet;\n\
           return driver.greet('world') + '|' + typeof greet + '|' + greet('friend');\n\
         }\n",
    );
    write(
        dir.path(),
        "implementation.ts",
        "export class Hello { greet(name: string) { return 'hello ' + name; } }\n",
    );
    write(
        dir.path(),
        "main.ts",
        "import { consume } from './consumer';\n\
         import { Hello } from './implementation';\n\
         console.log(consume(new Hello()));\n",
    );

    assert_eq!(
        compile_and_run(dir.path(), "main.ts"),
        "hello world|function|hello friend\n"
    );
}

#[test]
fn json_module_parses_embedded_serialized_data() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "data.json",
        r#"{"name":"Perry","items":[1,true,null],"nested":{"message":"Grüße"}}"#,
    );
    write(
        dir.path(),
        "main.ts",
        "import data from './data.json';\n\
         console.log(data.name, data.items.length, data.items[1], data.nested.message);\n",
    );

    assert_eq!(
        compile_and_run(dir.path(), "main.ts"),
        "Perry 3 true Grüße\n"
    );
}

#[test]
fn renamed_export_exposes_raw_local_getter_spelling() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "filter.js",
        "var $i = (value) => value + 1; export { $i as filesFilter };\n",
    );
    write(
        dir.path(),
        "main.ts",
        "import { filesFilter } from './filter.js'; console.log(filesFilter(41));\n",
    );

    assert_eq!(compile_and_run(dir.path(), "main.ts"), "42\n");
}

#[test]
fn native_namespace_reexport_survives_export_all_barrels() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "namespace.ts",
        "export * as Path from 'node:path';\n",
    );
    write(dir.path(), "barrel.ts", "export * from './namespace';\n");
    write(
        dir.path(),
        "main.ts",
        "import { Path } from './barrel'; console.log(Path.join('a', 'b').replace('\\\\', '/'));\n",
    );

    assert_eq!(compile_and_run(dir.path(), "main.ts"), "a/b\n");
}

#[test]
fn namespace_exported_rest_closure_accepts_more_than_sixteen_args() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "layer.ts",
        "export const mergeAll = (...values: number[]) => values.length;\n",
    );
    write(
        dir.path(),
        "main.ts",
        "import * as Layer from './layer';\n\
         console.log(Layer.mergeAll(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18));\n",
    );

    assert_eq!(compile_and_run(dir.path(), "main.ts"), "18\n");
}

#[test]
fn named_namespace_reexport_rest_closure_accepts_more_than_sixteen_args() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "layer.ts",
        "export const mergeAll = (...values: number[]) => values.length;\n",
    );
    write(
        dir.path(),
        "barrel.ts",
        "export * as Layer from './layer';\n",
    );
    write(
        dir.path(),
        "main.ts",
        "import { Layer } from './barrel';\n\
         console.log(Layer.mergeAll(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18));\n",
    );

    assert_eq!(compile_and_run(dir.path(), "main.ts"), "18\n");
}

#[test]
fn self_namespace_reexport_is_not_treated_as_a_function() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "event.ts",
        "export * as Event from './event';\n\
         export const answer = 42;\n\
         export function add(left: number, right: number) { return left + right; }\n\
         export function inventory(...values: number[]) { return values.length; }\n",
    );
    write(
        dir.path(),
        "main.ts",
        "import { Event } from './event';\n\
         console.log(Event.answer, Event.add(1, 2), Event.inventory(1, 2, 3));\n",
    );

    assert_eq!(compile_and_run(dir.path(), "main.ts"), "42 3 3\n");
}

#[test]
fn materialized_namespace_keeps_nested_namespace_exports() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "core.ts", "export const answer = 42;\n");
    write(
        dir.path(),
        "external.ts",
        "export * as core from './core';\n",
    );
    write(
        dir.path(),
        "index.ts",
        "export * as schema from './external';\n",
    );
    write(
        dir.path(),
        "main.ts",
        "import { schema } from './index';\n\
         const assigned = schema;\n\
         console.log(assigned.core.answer);\n",
    );

    assert_eq!(compile_and_run(dir.path(), "main.ts"), "42\n");
}

#[test]
fn nested_namespaces_survive_named_and_export_all_barrels() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "util.ts", "export const name = 'util';\n");
    write(dir.path(), "regexes.ts", "export const name = 'regexes';\n");
    write(dir.path(), "coerce.ts", "export const name = 'coerce';\n");
    write(dir.path(), "iso.ts", "export const name = 'iso';\n");
    write(
        dir.path(),
        "core.ts",
        "export * as util from './util';\nexport * as regexes from './regexes';\n",
    );
    write(
        dir.path(),
        "external.ts",
        "export { util, regexes } from './core';\n\
         export * as coerce from './coerce';\n\
         export * as iso from './iso';\n",
    );
    write(dir.path(), "index.ts", "export * from './external';\n");
    write(
        dir.path(),
        "main.ts",
        "import * as z from './index';\n\
         const assigned = z;\n\
         console.log(assigned.util.name, assigned.regexes.name, assigned.coerce.name, assigned.iso.name);\n",
    );

    assert_eq!(
        compile_and_run(dir.path(), "main.ts"),
        "util regexes coerce iso\n"
    );
}

#[test]
fn self_namespace_can_be_aliased_as_an_exported_const() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "event.ts",
        "export * as Event from './event';\n\
         export function define(value: number) { return value; }\n",
    );
    write(
        dir.path(),
        "session-event.ts",
        "export * as SessionEvent from './session-event';\n\
         import { Event } from './event';\n\
         export const Started = Event.define(42);\n",
    );
    write(
        dir.path(),
        "session.ts",
        "export * as Session from './session';\n\
         import { SessionEvent } from './session-event';\n\
         export const Event = SessionEvent;\n",
    );
    write(
        dir.path(),
        "main.ts",
        "import { Session } from './session';\n\
         console.log(Session.Event.Started);\n",
    );

    assert_eq!(compile_and_run(dir.path(), "main.ts"), "42\n");
}

#[test]
fn imported_class_reexport_uses_the_defining_constructor() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "chunk-node.js",
        "class Box { constructor(value) { this.value = value; } }\nexport { Box };\n",
    );
    write(
        dir.path(),
        "chunk-bun.js",
        "class Box { constructor(value) { this.value = value + 100; } }\nexport { Box };\n",
    );
    write(
        dir.path(),
        "index.node.js",
        "import { Box } from './chunk-node.js';\n\
         class Child extends Box {}\n\
         export { Box, Child };\n",
    );
    write(
        dir.path(),
        "main.ts",
        "import './chunk-bun.js';\n\
         import { Box } from './index.node.js';\n\
         function build<T>(value: T) { return new Box(value); }\n\
         console.log(build(42).value);\n",
    );

    assert_eq!(compile_and_run(dir.path(), "main.ts"), "42\n");
}
