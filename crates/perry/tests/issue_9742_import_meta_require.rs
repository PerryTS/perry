use std::process::Command;

fn compile_and_run(source: &str, bunfs: bool) -> String {
    let dir = tempfile::tempdir().unwrap();
    let entry = dir.path().join("main.js");
    let binary = dir.path().join("app");
    std::fs::write(&entry, source).unwrap();
    std::fs::write(
        dir.path().join("dep.js"),
        "globalThis.loads = (globalThis.loads ?? 0) + 1; export const answer = 42;",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("other.js"),
        "globalThis.otherLoads = (globalThis.otherLoads ?? 0) + 1; export const answer = 99;",
    )
    .unwrap();
    let mut compile = Command::new(env!("CARGO_BIN_EXE_perry"));
    compile.args(["compile", "--platform", "bun"]);
    if bunfs {
        compile.arg("--bunfs-root").arg(dir.path());
    }
    let output = compile
        .arg(&entry)
        .arg("-o")
        .arg(&binary)
        .env("PERRY_NO_CACHE", "1")
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    // The executable must contain the chunks; runtime filesystem loading would
    // hide a missing AOT import edge.
    std::fs::remove_file(dir.path().join("dep.js")).unwrap();
    std::fs::remove_file(dir.path().join("other.js")).unwrap();
    let output = Command::new(binary).output().unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn relative_chunks_load_synchronously_once_at_the_call() {
    let output = compile_and_run(
        r#"
        console.log(globalThis.loads ?? 0);
        const first = import.meta.require("./dep.js");
        const second = import.meta["require"]("./dep.js");
        console.log(first.answer, second.answer, globalThis.loads);
        console.log(first === second, typeof first.then);
        const require = value => "local:" + value;
        const object = { require(value) { return "object:" + value; } };
        console.log(require("value"), object.require("value"));
        console.log(typeof import.meta.url, import.meta.main);
        console.log(typeof import.meta.require("node:os").platform());
    "#,
        false,
    );
    assert_eq!(
        output,
        "0\n42 42 1\ntrue undefined\nlocal:value object:value\nstring true\nstring\n"
    );
}

#[test]
fn bun_virtual_chunks_are_discovered_for_both_spellings() {
    assert_eq!(
        compile_and_run(
            r#"
        const first = import.meta.require("/$bunfs/root/dep.js");
        const second = import.meta["require"]("/$bunfs/root/dep.js");
        console.log(first.answer, second.answer, globalThis.loads);
    "#,
            true
        ),
        "42 42 1\n"
    );
}

#[test]
fn computed_literal_only_entry_and_finite_choices_are_lazy() {
    assert_eq!(
        compile_and_run(
            r#"
        const path = process.argv.length > 0 ? "./dep.js" : "./other.js";
        console.log(globalThis.loads ?? 0, globalThis.otherLoads ?? 0);
        const first = import.meta["require"](path);
        console.log(first.answer, globalThis.loads, globalThis.otherLoads ?? 0);
    "#,
            false
        ),
        "0 0\n42 1 0\n"
    );
}

#[test]
fn missing_and_runtime_paths_report_synchronous_require_errors() {
    assert_eq!(
        compile_and_run(
            r#"
        try { import.meta.require("./missing.js"); }
        catch (error) { console.log(error.code); }
        const path = process.argv[99] ?? "./missing-runtime.js";
        try { import.meta["require"](path); }
        catch (error) { console.log(error.code); }
    "#,
            false
        ),
        "MODULE_NOT_FOUND\nMODULE_NOT_FOUND\n"
    );
}

#[test]
fn whitespace_and_comments_do_not_turn_member_calls_into_eager_imports() {
    assert_eq!(
        compile_and_run(
            r#"
        console.log(globalThis.loads ?? 0);
        const object = { require(value) { return "object:" + value; } };
        console.log(object . require("./dep.js"));
        const first = import.meta . require("./dep.js");
        const second = import.meta. /* gap */ require("./dep.js");
        console.log(first.answer, second.answer, globalThis.loads);
    "#,
            false
        ),
        "0\nobject:./dep.js\n42 42 1\n"
    );
}
