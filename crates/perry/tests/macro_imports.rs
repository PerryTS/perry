//! The macro contract spans compilation and a later process without the inputs.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::{tempdir, TempDir};

fn compile(root: &Path, source: &str, macro_source: &str, output: &Path) -> Output {
    fs::write(root.join("main.ts"), source).unwrap();
    fs::write(root.join("macro.ts"), macro_source).unwrap();
    invoke(root, output, false)
}

fn invoke(root: &Path, output: &Path, caching: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_perry"));
    command.args(["compile", "--no-codegen"]);
    if !caching {
        command.arg("--no-cache");
    }
    command
        .arg("--cache-dir")
        .arg(root.join("objects"))
        .arg(root.join("main.ts"))
        .arg("-o")
        .arg(output)
        .env_remove("PERRY_SOURCE_ROOT")
        .env_remove("PERRY_SOURCE_ROOT_TARGET")
        .env_remove("PERRY_FACTS_ONLY")
        .env_remove("PERRY_FACTS_DUMP")
        .output()
        .unwrap()
}

fn diagnostics(output: &Output) -> String {
    format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn macro_reads_source_files_only_at_compile_time_and_respects_shadowing() {
    let source = tempdir().unwrap();
    let binaries = tempdir().unwrap();
    let output = binaries.path().join("program");
    fs::write(source.path().join("input.txt"), "build-time value").unwrap();
    let compiled = compile(
        source.path(),
        r#"
        import { read as load } from './macro.ts' with { type: 'macro' };
        function shadow(load: () => number) { return load(); }
        console.log(JSON.stringify({stack: await load(), shadow: shadow(() => 19)}));
    "#,
        r#"
        import { readFileSync } from 'node:fs';
        export async function read() {
            return {files: {"entry.ts": readFileSync(new URL('./input.txt', import.meta.url), 'utf8')}};
        }
    "#,
        &output,
    );
    assert!(compiled.status.success(), "{}", diagnostics(&compiled));
    drop(source);
    let run = Command::new(output).output().unwrap();
    assert!(run.status.success(), "{}", diagnostics(&run));
    assert_eq!(
        String::from_utf8_lossy(&run.stdout).trim(),
        r#"{"stack":{"files":{"entry.ts":"build-time value"}},"shadow":19}"#
    );
}

/// The runner awaits each macro at its top level. A macro that settles through
/// a callback-style fs completion (Effect's FileSystem, Supabase's
/// compute-stacks macro) resumes only in the poll phase of that await's turn.
#[test]
fn macro_settled_by_fs_callbacks_expands() {
    let source = tempdir().unwrap();
    let binaries = tempdir().unwrap();
    let output = binaries.path().join("program");
    fs::create_dir_all(source.path().join("stacks/node")).unwrap();
    fs::write(source.path().join("stacks/node/index.ts"), "hi").unwrap();
    let compiled = compile(
        source.path(),
        r#"
        import { stacks } from './macro.ts' with { type: 'macro' };
        console.log(JSON.stringify(stacks()));
    "#,
        r#"
        import { readdir, readFile } from 'node:fs';
        import { fileURLToPath } from 'node:url';
        const list = (dir: string) => new Promise<string[]>((ok, no) => readdir(dir, (e, names) => e ? no(e) : ok(names.sort())));
        const read = (file: string) => new Promise<string>((ok, no) => readFile(file, 'utf8', (e, text) => e ? no(e) : ok(text)));
        export async function stacks() {
            const root = fileURLToPath(new URL('stacks', import.meta.url));
            const out: Record<string, Record<string, string>> = {};
            for (const entry of await list(root)) {
                const files: Record<string, string> = {};
                for (const name of await list(root + '/' + entry)) files[name] = await read(root + '/' + entry + '/' + name);
                out[entry] = files;
            }
            return out;
        }
    "#,
        &output,
    );
    assert!(compiled.status.success(), "{}", diagnostics(&compiled));
    drop(source);
    let run = Command::new(output).output().unwrap();
    assert!(run.status.success(), "{}", diagnostics(&run));
    assert_eq!(
        String::from_utf8_lossy(&run.stdout).trim(),
        r#"{"node":{"index.ts":"hi"}}"#
    );
}

#[test]
fn macro_calls_share_one_module_instance_and_inline_awaited_results() {
    let source = tempdir().unwrap();
    let binaries = tempdir().unwrap();
    let output = binaries.path().join("program");
    let compiled = compile(
        source.path(),
        r#"
        import { next } from './macro.ts' with { type: 'macro' };
        const first = next();
        console.log(JSON.stringify([first, next(), next(next())]));
    "#,
        "let counter = 0; export async function next(n = 0) { counter++; return counter + n; }",
        &output,
    );
    assert!(compiled.status.success(), "{}", diagnostics(&compiled));
    drop(source);
    let run = Command::new(output).output().unwrap();
    assert!(run.status.success(), "{}", diagnostics(&run));
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "[1,2,7]");
}

#[test]
fn non_json_results_throws_and_rejections_name_the_import() {
    for body in [
        "return () => 1;",
        "return {nested: undefined};",
        "return {nested: Symbol('bad')};",
        "return 1n;",
        "return NaN;",
        "const view = new DataView(new ArrayBuffer(8)); view.setUint32(0, 0x80000000); return view.getFloat64(0);",
        "return String.fromCharCode(0xd800);",
        "return new Date();",
        "const value = {}; value.self = value; return value;",
        "throw new Error('macro-sentinel');",
        "return Promise.reject(new Error('macro-sentinel'));",
    ] {
        let source = tempdir().unwrap();
        let binaries = tempdir().unwrap();
        let compiled = compile(
            source.path(),
            "import { read } from './macro.ts' with { type: 'macro' }; console.log(read());",
            &format!("export async function read() {{ {body} }}"),
            &binaries.path().join("program"),
        );
        let text = diagnostics(&compiled);
        assert!(
            !compiled.status.success(),
            "invalid macro result compiled: {body}\n{text}"
        );
        assert!(text.contains("macro.ts") && text.contains("read"), "{text}");
    }
}

#[test]
fn dynamic_arguments_and_escaping_bindings_are_named_compile_errors() {
    for call in [
        "read(process.argv.length)",
        "read(Math.random())",
        "(() => { const alias = read; return alias(); })()",
    ] {
        let source = tempdir().unwrap();
        let binaries = tempdir().unwrap();
        let compiled = compile(source.path(),
            &format!("import {{ read }} from './macro.ts' with {{ type: 'macro' }}; console.log({call});"),
            "export function read(value) { return value; }", &binaries.path().join("program"));
        let text = diagnostics(&compiled);
        assert!(
            !compiled.status.success(),
            "dynamic macro compiled: {call}\n{text}"
        );
        assert!(text.contains("macro.ts") && text.contains("read"), "{text}");
    }
}

#[test]
fn recursive_macro_expansion_reports_a_named_cycle() {
    let source: TempDir = tempdir().unwrap();
    let binaries = tempdir().unwrap();
    fs::write(
        source.path().join("other.ts"),
        "import { read } from './macro.ts' with { type: 'macro' }; export const other = read();",
    )
    .unwrap();
    let compiled = compile(source.path(),
        "import { read } from './macro.ts' with { type: 'macro' }; console.log(read());",
        "import { other } from './other.ts' with { type: 'macro' }; export function read() { return other(); }", &binaries.path().join("program"));
    let text = diagnostics(&compiled);
    assert!(
        !compiled.status.success(),
        "recursive macro compiled: {text}"
    );
    assert!(
        text.contains("cyclic macro expansion") && text.contains("macro.ts"),
        "{text}"
    );
}

#[test]
fn macro_object_order_and_own_data_keys_survive_transport() {
    let source = tempdir().unwrap();
    let binaries = tempdir().unwrap();
    let output = binaries.path().join("program");
    let compiled = compile(
        source.path(),
        r#"
        import { read } from './macro.ts' with { type: 'macro' };
        const value = read();
        console.log(Object.keys(value).join(','));
        console.log(Object.hasOwn(value, '__proto__'), value['__proto__']);
    "#,
        r#"export function read() { return JSON.parse('{"z":0,"a":2,"__proto__":3}'); }"#,
        &output,
    );
    assert!(compiled.status.success(), "{}", diagnostics(&compiled));
    drop(source);
    let run = Command::new(output).output().unwrap();
    assert!(run.status.success(), "{}", diagnostics(&run));
    assert_eq!(
        String::from_utf8_lossy(&run.stdout).trim(),
        "z,a,__proto__\ntrue 3"
    );
}

#[test]
fn cached_compilations_rerun_macros_after_only_input_data_changes() {
    for initially_unused in [false, true] {
        let source = tempdir().unwrap();
        let binaries = tempdir().unwrap();
        let output = binaries.path().join("program");
        let used =
            "import { read } from './macro.ts' with { type: 'macro' }; console.log(await read());";
        let unused =
            "import { read } from './macro.ts' with { type: 'macro' }; console.log('unused');";
        fs::write(
            source.path().join("main.ts"),
            if initially_unused { unused } else { used },
        )
        .unwrap();
        fs::write(source.path().join("macro.ts"), "import {readFileSync} from 'node:fs'; export function read() { return readFileSync(new URL('./input.txt',import.meta.url),'utf8'); }").unwrap();
        fs::write(source.path().join("input.txt"), "first").unwrap();
        if initially_unused {
            let initial = invoke(source.path(), &output, true);
            assert!(initial.status.success(), "{}", diagnostics(&initial));
            fs::write(source.path().join("main.ts"), used).unwrap();
        }
        let first = invoke(source.path(), &output, true);
        assert!(first.status.success(), "{}", diagnostics(&first));
        // Keep both TS files and their mtimes unchanged for the cache probe.
        fs::write(source.path().join("input.txt"), "second").unwrap();
        let second = invoke(source.path(), &output, true);
        assert!(second.status.success(), "{}", diagnostics(&second));
        drop(source);
        let run = Command::new(output).output().unwrap();
        assert!(run.status.success(), "{}", diagnostics(&run));
        assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "second");
    }
}

#[test]
fn every_macro_result_is_snapshotted_before_later_calls_or_argument_mutation() {
    let source = tempdir().unwrap();
    let binaries = tempdir().unwrap();
    let output = binaries.path().join("program");
    let compiled = compile(
        source.path(),
        r#"
        import { read, bump } from './macro.ts' with { type: 'macro' };
        console.log(JSON.stringify([read(), bump(read()), read()]));
    "#,
        r#"
        const shared = {n:1};
        export function read() { return shared; }
        export function bump(value) { value.n = 99; shared.n++; return shared; }
    "#,
        &output,
    );
    assert!(compiled.status.success(), "{}", diagnostics(&compiled));
    drop(source);
    let run = Command::new(output).output().unwrap();
    assert!(run.status.success(), "{}", diagnostics(&run));
    assert_eq!(
        String::from_utf8_lossy(&run.stdout).trim(),
        r#"[{"n":1},{"n":2},{"n":2}]"#
    );
}

#[test]
fn repeated_identifier_defines_preserve_independent_macro_results() {
    let source = tempdir().unwrap();
    let binaries = tempdir().unwrap();
    let output = binaries.path().join("program");
    fs::write(source.path().join("main.ts"), "import { next } from './macro.ts' with { type: 'macro' }; console.log(JSON.stringify([TOKEN(),TOKEN()]));").unwrap();
    fs::write(
        source.path().join("macro.ts"),
        "const shared = {n:0}; export function next() { shared.n++; return shared; }",
    )
    .unwrap();
    let compiled = Command::new(env!("CARGO_BIN_EXE_perry"))
        .args([
            "compile",
            "--no-cache",
            "--no-codegen",
            "--define",
            "TOKEN=next",
        ])
        .arg("--cache-dir")
        .arg(source.path().join("objects"))
        .arg(source.path().join("main.ts"))
        .arg("-o")
        .arg(&output)
        .env_remove("PERRY_SOURCE_ROOT")
        .env_remove("PERRY_SOURCE_ROOT_TARGET")
        .env_remove("PERRY_FACTS_ONLY")
        .env_remove("PERRY_FACTS_DUMP")
        .output()
        .unwrap();
    assert!(compiled.status.success(), "{}", diagnostics(&compiled));
    drop(source);
    let run = Command::new(output).output().unwrap();
    assert!(run.status.success(), "{}", diagnostics(&run));
    assert_eq!(
        String::from_utf8_lossy(&run.stdout).trim(),
        r#"[{"n":1},{"n":2}]"#
    );
}

#[test]
fn build_time_console_output_preserves_structured_compiler_stdout() {
    let source = tempdir().unwrap();
    let binaries = tempdir().unwrap();
    let output = binaries.path().join("program");
    fs::write(
        source.path().join("main.ts"),
        "import { read } from './macro.ts' with { type: 'macro' }; console.log(read());",
    )
    .unwrap();
    fs::write(
        source.path().join("macro.ts"),
        "export function read() { console.log('macro-build-message'); return 7; }",
    )
    .unwrap();
    let compiled = Command::new(env!("CARGO_BIN_EXE_perry"))
        .args(["--format", "json", "compile", "--no-cache", "--no-codegen"])
        .arg("--cache-dir")
        .arg(source.path().join("objects"))
        .arg(source.path().join("main.ts"))
        .arg("-o")
        .arg(&output)
        .env_remove("PERRY_SOURCE_ROOT")
        .env_remove("PERRY_SOURCE_ROOT_TARGET")
        .env_remove("PERRY_FACTS_ONLY")
        .env_remove("PERRY_FACTS_DUMP")
        .output()
        .unwrap();
    assert!(compiled.status.success(), "{}", diagnostics(&compiled));
    serde_json::from_slice::<serde_json::Value>(&compiled.stdout)
        .expect("compiler stdout is one JSON value");
    assert!(
        String::from_utf8_lossy(&compiled.stderr).contains("macro-build-message"),
        "{}",
        diagnostics(&compiled)
    );
    drop(source);
    let run = Command::new(output).output().unwrap();
    assert!(run.status.success(), "{}", diagnostics(&run));
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "7");
}

#[test]
fn graph_refinement_executes_build_time_code_once_per_compilation() {
    let source = tempdir().unwrap();
    let binaries = tempdir().unwrap();
    let output = binaries.path().join("program");
    fs::write(
        source.path().join("package.json"),
        r#"{"type":"module","sideEffects":false}"#,
    )
    .unwrap();
    fs::write(
        source.path().join("normal.ts"),
        "export const needed = 1; export const unused = 2;",
    )
    .unwrap();
    fs::write(
        source.path().join("main.ts"),
        r#"
        import { read } from './macro.ts' with {type:'macro'};
        import { needed } from './normal.ts';
        class Child { constructor() { this.value = 1; } }
        class Box { constructor() { this.child = new Child(); } }
        const box = new Box();
        console.log(read(), box.child.value, needed);
    "#,
    )
    .unwrap();
    fs::write(
        source.path().join("macro.ts"),
        r#"
        import { appendFileSync, readFileSync } from 'node:fs';
        export function read() {
            const calls = new URL('./calls.txt', import.meta.url);
            appendFileSync(calls, 'x');
            return readFileSync(calls, 'utf8').length;
        }
    "#,
    )
    .unwrap();
    for expected in 1..=2 {
        let compiled = Command::new(env!("CARGO_BIN_EXE_perry"))
            .args(["compile", "--no-codegen"])
            .arg("--cache-dir")
            .arg(source.path().join("objects"))
            .arg(source.path().join("main.ts"))
            .arg("-o")
            .arg(&output)
            .env_remove("PERRY_SOURCE_ROOT")
            .env_remove("PERRY_SOURCE_ROOT_TARGET")
            .env_remove("PERRY_FACTS_ONLY")
            .env_remove("PERRY_FACTS_DUMP")
            .output()
            .unwrap();
        assert!(compiled.status.success(), "{}", diagnostics(&compiled));
        assert_eq!(
            fs::read_to_string(source.path().join("calls.txt")).unwrap(),
            "x".repeat(expected),
            "{}",
            diagnostics(&compiled)
        );
    }
    drop(source);
    let run = Command::new(output).output().unwrap();
    assert!(run.status.success(), "{}", diagnostics(&run));
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "2 1 1");
}
