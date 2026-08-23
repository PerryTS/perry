//! Cross-module export shapes found while compiling OpenCode's TypeScript
//! source graph. Each regression previously produced an undefined native
//! linker symbol despite all source modules lowering successfully.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Once;

const GC_ENV_OVERRIDES: &[&str] = &[
    "PERRY_GEN_GC",
    "PERRY_GC_SCAVENGE",
    "PERRY_GC_SCAVENGE_NURSERY_MB",
    "PERRY_GC_MOVING_SAFEPOINT",
    "PERRY_GC_MOVING_LOOP_POLLS",
    "PERRY_GC_FORCE_EVACUATE",
    "PERRY_CONSERVATIVE_STACK_SCAN",
    "PERRY_WRITE_BARRIERS",
    "PERRY_GC_INCREMENTAL",
    "PERRY_GC_HEAP_LIMIT",
];

fn remove_gc_env_overrides(command: &mut Command) {
    for key in GC_ENV_OVERRIDES {
        command.env_remove(key);
    }
}

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
    if let Some(runtime_dir) = std::env::var_os("PERRY_RUNTIME_DIR") {
        return PathBuf::from(runtime_dir);
    }
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

fn compile_and_run_with_llvm_trace(dir: &Path, entry: &str) -> (String, String) {
    let output = dir.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(entry)
        .arg("--no-cache")
        .arg("--trace")
        .arg("llvm")
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

    let trace_dir = dir.join(".perry-trace/llvm");
    let entry_stem = entry.replace(['/', '.', '-'], "_");
    let entry_ir_path = std::fs::read_dir(&trace_dir)
        .expect("read LLVM trace directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(&entry_stem) && name.ends_with(".ll"))
        })
        .unwrap_or_else(|| panic!("missing LLVM trace for {entry} in {}", trace_dir.display()));
    let entry_ir = std::fs::read_to_string(entry_ir_path).expect("read entry LLVM trace");

    (String::from_utf8_lossy(&run.stdout).into_owned(), entry_ir)
}

fn compile_and_run_with_all_llvm_trace(dir: &Path, entry: &str) -> (String, String) {
    let output = dir.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(entry)
        .arg("--no-cache")
        .arg("--trace")
        .arg("llvm")
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

    let trace_dir = dir.join(".perry-trace/llvm");
    let mut trace_paths: Vec<PathBuf> = std::fs::read_dir(&trace_dir)
        .expect("read LLVM trace directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("ll"))
        .collect();
    trace_paths.sort();
    let mut all_ir = String::new();
    for path in trace_paths {
        all_ir.push_str(&std::fs::read_to_string(path).expect("read LLVM trace"));
        all_ir.push('\n');
    }
    (String::from_utf8_lossy(&run.stdout).into_owned(), all_ir)
}

fn llvm_function_body_containing(
    ir: &str,
    definition_contains: &str,
    body_contains: &str,
) -> String {
    let lines: Vec<&str> = ir.lines().collect();
    for start in 0..lines.len() {
        if !lines[start].starts_with("define") || !lines[start].contains(definition_contains) {
            continue;
        }
        let body = lines[start..]
            .iter()
            .copied()
            .take_while(|line| *line != "}")
            .collect::<Vec<_>>()
            .join("\n");
        if body.contains(body_contains) {
            return body;
        }
    }
    panic!(
        "no definition containing {definition_contains:?} whose body contains \
         {body_contains:?}:\n{ir}"
    );
}

#[test]
fn cross_module_arrow_callback_dispatch_is_resolved_once_and_fails_closed() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "sink.ts",
        "export class Sink {\n\
           run(callback: (a: number, b: number, c: number) => number) {\n\
             let sum = 0;\n\
             for (let i = 0; i < 32; i++) sum += callback(i, i + 1, i + 2);\n\
             return sum;\n\
           }\n\
           ordinary(callback: (a: number, b: number, c: number) => number) {\n\
             return callback(1, 2, 3);\n\
           }\n\
         }\n",
    );
    write(
        dir.path(),
        "forwarder.ts",
        "import { Sink } from './sink';\n\
         export class Forwarder {\n\
           sink = new Sink();\n\
           run(callback: (a: number, b: number, c: number) => number) {\n\
             return this.sink.run(callback);\n\
           }\n\
           ordinary(callback: (a: number, b: number, c: number) => number) {\n\
             return this.sink.ordinary(callback);\n\
           }\n\
         }\n",
    );
    write(
        dir.path(),
        "main.ts",
        "import { Forwarder } from './forwarder';\n\
         const iterator = new Forwarder();\n\
         function exercise() {\n\
           let invocations = 0;\n\
           const total = iterator.run((a, b, c) => {\n\
             invocations++;\n\
             const churn = new Array(8192);\n\
             churn[0] = { value: a };\n\
             return a + b + c;\n\
           });\n\
           console.log(total);\n\
           console.log(invocations);\n\
         }\n\
         function ordinary(a: number, b: number, c: number) {\n\
           return this === undefined ? 42 : -1;\n\
         }\n\
         exercise();\n\
         console.log(iterator.ordinary(ordinary));\n",
    );

    let (stdout, all_ir) = compile_and_run_with_all_llvm_trace(dir.path(), "main.ts");
    assert_eq!(stdout, "1584\n32\n42\n");

    let sink_run = llvm_function_body_containing(
        &all_ir,
        "@perry_method_sink_ts__Sink__run",
        "@js_closure_resolve_arrow_direct_call(",
    );
    assert_eq!(
        sink_run
            .matches("@js_closure_resolve_arrow_direct_call(")
            .count(),
        1,
        "the callback target must be resolved once at method entry:\n{sink_run}"
    );
    assert!(
        sink_run
            .lines()
            .any(|line| line.contains(" = call double %") && line.contains("i64 ")),
        "the admitted arrow arm must call the resolved target:\n{sink_run}"
    );
    assert!(
        sink_run.contains("@js_closure_call3("),
        "the public method must retain full fallback dispatch:\n{sink_run}"
    );
    assert!(
        !all_ir.contains("$callback_"),
        "entry hoisting must not clone callback method bodies"
    );
    assert!(
        all_ir.contains("$trusted_boxes"),
        "the direct arrow with a mutable capture must get a bounded private body"
    );
    assert!(
        all_ir.contains("call void @js_register_closure_trusted_direct(")
            && all_ir.contains("i32 1, i64 1)"),
        "module init must register the exact one-box capture layout"
    );
    assert!(
        all_ir.contains("call i64 @js_box_get_bits_trusted(")
            && all_ir.contains("call void @js_box_set_bits_trusted_no_barrier("),
        "the private body must use the trusted box accessors"
    );

    let mut forced_command = Command::new(dir.path().join("main_bin"));
    forced_command.current_dir(dir.path());
    remove_gc_env_overrides(&mut forced_command);
    let forced = forced_command
        .env("PERRY_GC_SCAVENGE", "1")
        .env("PERRY_GC_SCAVENGE_NURSERY_MB", "1")
        .env("PERRY_GC_FORCE_EVACUATE", "1")
        .env("PERRY_GC_VERIFY_EVACUATION", "1")
        .env("PERRY_GC_INCREMENTAL", "0")
        .output()
        .expect("run compiled binary with forced evacuation");
    assert!(
        forced.status.success(),
        "forced-evacuation run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&forced.stdout),
        String::from_utf8_lossy(&forced.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&forced.stdout), "1584\n32\n42\n");
}

#[test]
fn imported_class_return_types_pull_in_transitive_dispatch_metadata() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "returned.ts",
        "export class Returned {\n\
           value: number;\n\
           constructor(value = 0) { this.value = value; }\n\
           methodPing() { return 41; }\n\
           getterPing() { return 42; }\n\
           staticPing() { return 43; }\n\
           valuePing() { return this.value; }\n\
         }\n\
         export function makeReturned(value = 0) { return new Returned(value); }\n",
    );
    write(
        dir.path(),
        "factory.ts",
        "import { makeReturned } from './returned';\n\
         import type { Returned as Result } from './returned';\n\
         class Hidden { pong() { return 44; } }\n\
         export class Factory {\n\
           make(): Result { return makeReturned(); }\n\
           get result(): Result { return makeReturned(); }\n\
           static create(): Result { return makeReturned(); }\n\
           static defaulted(value = 45): Result {\n\
             return makeReturned(value);\n\
           }\n\
           static rested(head: number, ...tail: number[]): Result {\n\
             return makeReturned(head + tail.length);\n\
           }\n\
           static argumentsBacked(head: number): Result {\n\
             return makeReturned(head + arguments.length);\n\
           }\n\
           static restAndArguments(head: number, ...tail: number[]): Result {\n\
             return makeReturned(head + tail.length + arguments.length);\n\
           }\n\
           hidden(): Hidden { return new Hidden(); }\n\
         }\n",
    );
    write(
        dir.path(),
        "main.ts",
        "import { Factory } from './factory';\n\
         const overridden = new Factory().make();\n\
         (overridden as any).methodPing = () => 99;\n\
         console.log(\n\
           overridden.methodPing(),\n\
           new Factory().result.getterPing(),\n\
           Factory.create().staticPing(),\n\
           new Factory().hidden().pong(),\n\
           Factory.defaulted().valuePing(),\n\
           Factory.rested(45, 1, 2).valuePing(),\n\
           Factory.argumentsBacked(45, 1, 2).valuePing(),\n\
           Factory.restAndArguments(40, 1, 2).valuePing(),\n\
         );\n",
    );

    let (stdout, entry_ir) = compile_and_run_with_llvm_trace(dir.path(), "main.ts");
    assert_eq!(stdout, "99 42 43 44 45 47 48 45\n");
    for symbol in [
        "perry_method_returned_ts__Returned__methodPing",
        "perry_method_returned_ts__Returned__getterPing",
        "perry_method_returned_ts__Returned__staticPing",
        "perry_method_returned_ts__Returned__valuePing",
        "perry_method_factory_ts__Hidden__pong",
    ] {
        assert!(
            entry_ir.contains(&format!("call double @{symbol}(")),
            "the consumer did not route the returned instance directly through {symbol}:\n{entry_ir}"
        );
    }
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
