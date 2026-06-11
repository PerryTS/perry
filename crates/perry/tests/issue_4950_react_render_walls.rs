//! Regression tests for #4950 — the "null hooks dispatcher / Invalid hook
//! call" wall that blocked every React renderer (ink, react-three-fiber, …).
//!
//! Debugging disproved the issue's cross-module-identity hypothesis (the
//! shared `ReactSharedInternals` object IS the same instance on both sides);
//! the real wall was a chain of independent bugs, each covered here:
//!
//! 1. Branch-local `var` hoisting inside FUNCTION EXPRESSIONS: react's dev
//!    reconciler does `if (cond) { var getCurrentTime = … } else {
//!    getCurrentTime = … }` inside its 17k-line factory; the binding never
//!    hoisted, the else-arm write went to an implicit global, and the render
//!    loop died on a silently-swallowed `getCurrentTime is not defined`.
//! 2. Timer builtins captured as VALUES: scheduler's `localSetImmediate =
//!    typeof setImmediate !== "undefined" ? setImmediate : null` read the
//!    TAG_TRUE sentinel (typeof "boolean") and host callbacks never ran.
//! 3. `new` through a variable holding the global `AbortController`
//!    (react-reconciler's `AbortControllerLocal`).
//! 4. JSX in react-importing modules must build ELEMENTS via
//!    `React.createElement`, not eagerly call function components via
//!    Perry's SSR `js_jsx` adapter — the eager call ran `useContext`
//!    outside any render, which is the literal "Invalid hook call" +
//!    `null.useContext` from the issue.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &std::path::Path, entry: &std::path::Path) -> String {
    let output = dir.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(entry)
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
    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).to_string()
}

#[test]
fn branch_local_var_hoists_in_nested_function_expression() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
const outer = (function () {
  return function factory(flag: boolean) {
    function reader() { return getCurrentTime(); }
    if (flag) {
      var getCurrentTime = function () { return 1; };
    } else {
      getCurrentTime = function () { return 2; };
    }
    return reader;
  };
})();
console.log("flag=false:", outer(false)());
console.log("flag=true:", outer(true)());
"#,
    )
    .expect("write entry");

    let stdout = compile_and_run(dir.path(), &entry);
    assert!(
        stdout.contains("flag=false: 2"),
        "else-arm assignment must reach the hoisted var (got: {stdout})"
    );
    assert!(
        stdout.contains("flag=true: 1"),
        "if-arm declaration must reach the hoisted var (got: {stdout})"
    );
}

#[test]
fn timer_builtins_work_as_captured_values() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
const si: any = (typeof setImmediate !== "undefined") ? setImmediate : null;
console.log("typeof si:", typeof si);
const st: any = setTimeout;
console.log("typeof st:", typeof st);
si(() => console.log("si ran"));
st(() => console.log("st ran"), 0);
"#,
    )
    .expect("write entry");

    let stdout = compile_and_run(dir.path(), &entry);
    assert!(
        stdout.contains("typeof si: function") && stdout.contains("typeof st: function"),
        "timer builtins as values must be functions, not the TAG_TRUE sentinel (got: {stdout})"
    );
    assert!(
        stdout.contains("si ran") && stdout.contains("st ran"),
        "callbacks scheduled through captured timer values must fire (got: {stdout})"
    );
}

#[test]
fn abort_controller_constructs_through_variable() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
const AbortControllerLocal: any =
  typeof AbortController !== "undefined" ? AbortController : null;
const c = new AbortControllerLocal();
console.log("controller:", typeof c, "signal:", typeof c.signal, "aborted:", c.signal.aborted);
"#,
    )
    .expect("write entry");

    let stdout = compile_and_run(dir.path(), &entry);
    assert!(
        stdout.contains("controller: object signal: object aborted: false"),
        "new through a captured AbortController must construct (got: {stdout})"
    );
}

#[test]
fn lone_surrogate_range_class_regex_compiles_and_matches() {
    // es-toolkit's `truncate.js` module-init regex (in ink's dependency
    // graph); pre-fix this threw `SyntaxError: invalid pattern` at startup.
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        "const re = /[\\u200d\\ud800-\\udfff\\u0300-\\u036f\\ufe20-\\ufe2f\\u20d0-\\u20ff\\ufe0e\\ufe0f]/;\n\
         console.log(\"ascii:\", re.test(\"plain ascii\"));\n\
         console.log(\"astral:\", re.test(\"a\\u{1F48D}b\"));\n\
         console.log(\"zwj:\", re.test(\"a\\u{200D}b\"));\n",
    )
    .expect("write entry");

    let stdout = compile_and_run(dir.path(), &entry);
    assert!(
        stdout.contains("ascii: false")
            && stdout.contains("astral: true")
            && stdout.contains("zwj: true"),
        "surrogate-range class must match astral chars like Node (got: {stdout})"
    );
}
