//! Conditional CommonJS dependencies must run at the require call site.

use std::path::{Path, PathBuf};
use std::process::Command;

fn compile(root: &Path, entry: &str) -> PathBuf {
    let compiler = PathBuf::from(env!("CARGO_BIN_EXE_perry"));
    let runtime = std::env::var_os("PERRY_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| compiler.parent().unwrap().to_owned());
    let output = root.join("native");
    let result = Command::new(compiler)
        .current_dir(root)
        .args(["compile", entry, "--no-cache", "--no-auto-optimize"])
        .arg("-o")
        .arg(&output)
        .env("PERRY_RUNTIME_DIR", runtime)
        .output()
        .expect("compile fixture");
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    output
}

fn run(binary: &Path, args: &[&str]) -> String {
    let result = Command::new(binary)
        .args(args)
        .output()
        .expect("run fixture");
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).unwrap()
}

#[test]
fn conditional_require_defers_the_transitive_graph_and_initializes_once() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("leaf.cjs"),
        "console.log('leaf'); module.exports = 42;",
    )
    .unwrap();
    std::fs::write(root.join("dep.cjs"),
        "const leaf = require('./leaf.cjs');\nconsole.log('dependency');\nmodule.exports = {value: leaf};").unwrap();
    std::fs::write(
        root.join("entry.cjs"),
        r#"
console.log('entry');
if (process.argv.includes('--load')) {
    const first = require('./dep.cjs');
    const second = require('./dep.cjs');
    console.log(first.value, first === second);
}
console.log('done');
"#,
    )
    .unwrap();
    let binary = compile(root, "entry.cjs");
    assert_eq!(run(&binary, &[]), "entry\ndone\n");
    assert_eq!(
        run(&binary, &["--load"]),
        "entry\nleaf\ndependency\n42 true\ndone\n"
    );
}

#[test]
fn concise_arrow_and_short_circuit_require_stay_lazy() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("dep.cjs"),
        "console.log('dependency'); module.exports = {value: 42};",
    )
    .unwrap();
    std::fs::write(
        root.join("entry.cjs"),
        r#"
const load = () => require('./dep.cjs');
console.log('entry');
process.argv.includes('--load') && console.log(load().value);
console.log('done');
"#,
    )
    .unwrap();
    let binary = compile(root, "entry.cjs");
    assert_eq!(run(&binary, &[]), "entry\ndone\n");
    assert_eq!(run(&binary, &["--load"]), "entry\ndependency\n42\ndone\n");
}

#[test]
fn require_exception_is_caught_at_its_original_try_boundary() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("dep.cjs"),
        "console.log('dependency'); throw new Error('fixture');",
    )
    .unwrap();
    std::fs::write(
        root.join("entry.cjs"),
        r#"
console.log('entry');
try { require('./dep.cjs'); }
catch (error) { console.log('caught', error.message); }
console.log('done');
"#,
    )
    .unwrap();
    let binary = compile(root, "entry.cjs");
    assert_eq!(
        run(&binary, &[]),
        "entry\ndependency\ncaught fixture\ndone\n"
    );
}

#[test]
fn static_import_still_evaluates_before_the_entry_body() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("dep.mjs"),
        "console.log('static'); export const value = 42;",
    )
    .unwrap();
    std::fs::write(
        root.join("lazy.mjs"),
        "console.log('unexpected dynamic init'); export const value = 0;",
    )
    .unwrap();
    std::fs::write(
        root.join("entry.mjs"),
        r#"
import { value } from './dep.mjs';
export function unused() { return import('./lazy.mjs'); }
console.log('entry', value);
"#,
    )
    .unwrap();
    let binary = compile(root, "entry.mjs");
    assert_eq!(run(&binary, &[]), "static\nentry 42\n");
}

#[test]
fn conditional_class_require_initializes_static_state() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("dep.cjs"),
        "console.log('dependency');\nclass Value { static answer = 42; }\nmodule.exports = Value;",
    )
    .unwrap();
    std::fs::write(
        root.join("entry.cjs"),
        r#"
class Unrelated {}
console.log('entry');
if (process.argv.includes('--load')) {
    const Value = require('./dep.cjs');
    console.log(Value.answer);
}
console.log('done');
"#,
    )
    .unwrap();
    let binary = compile(root, "entry.cjs");
    assert_eq!(run(&binary, &[]), "entry\ndone\n");
    assert_eq!(run(&binary, &["--load"]), "entry\ndependency\n42\ndone\n");
}

#[test]
fn conditional_named_export_does_not_forward_an_unloaded_dependency() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("dep.cjs"),
        "console.log('dependency'); module.exports = {value: 42};",
    )
    .unwrap();
    std::fs::write(
        root.join("wrapper.cjs"),
        r#"
console.log('wrapper');
if (process.argv.includes('--load')) exports.optional = require('./dep.cjs');
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("entry.mjs"),
        r#"
import { optional } from './wrapper.cjs';
console.log('entry', optional === undefined ? 'absent' : optional.value);
"#,
    )
    .unwrap();
    let binary = compile(root, "entry.mjs");
    assert_eq!(run(&binary, &[]), "wrapper\nentry absent\n");
    assert_eq!(run(&binary, &["--load"]), "wrapper\ndependency\nentry 42\n");
}

#[test]
fn conditional_side_effect_only_require_runs_once_when_reached() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("dep.cjs"), "console.log('dependency');").unwrap();
    std::fs::write(
        root.join("entry.cjs"),
        r#"
console.log('entry');
if (process.argv.includes('--load')) {
    require('./dep.cjs');
    require('./dep.cjs');
}
console.log('done');
"#,
    )
    .unwrap();
    let binary = compile(root, "entry.cjs");
    assert_eq!(run(&binary, &[]), "entry\ndone\n");
    assert_eq!(run(&binary, &["--load"]), "entry\ndependency\ndone\n");
}

#[test]
fn esm_function_local_class_require_initializes_static_state() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("dep.cjs"),
        "console.log('dependency');\nclass Value { static answer = 42; }\nmodule.exports = Value;",
    )
    .unwrap();
    std::fs::write(
        root.join("entry.mjs"),
        r#"
import { createRequire } from 'node:module';
const require = createRequire(import.meta.url);
const load = () => require('./dep.cjs');
console.log('entry');
if (process.argv.includes('--load')) console.log(load().answer);
console.log('done');
"#,
    )
    .unwrap();
    let binary = compile(root, "entry.mjs");
    assert_eq!(run(&binary, &[]), "entry\ndone\n");
    assert_eq!(run(&binary, &["--load"]), "entry\ndependency\n42\ndone\n");
}

/// A deferred target in a require cycle must still see its partner's exports
/// assigned at run time: the partner reads the target's partial exports object
/// during the cycle and the value only exists after the target's body ends.
#[test]
fn conditional_require_cycle_partner_sees_runtime_assigned_exports() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("a.cjs"),
        r#"
console.log('a start');
exports.value = undefined;
const b = require('./b.cjs');
exports.value = () => 'a-runtime';
exports.read = () => b.callA();
console.log('a end');
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("b.cjs"),
        r#"
console.log('b start', typeof require('./a.cjs').value);
const a = require('./a.cjs');
exports.callA = () => a.value();
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("entry.cjs"),
        r#"
console.log('entry');
if (process.argv.includes('--load')) {
    const a = require('./a.cjs');
    console.log(a.read(), typeof a.value);
}
console.log('done');
"#,
    )
    .unwrap();
    let binary = compile(root, "entry.cjs");
    assert_eq!(run(&binary, &[]), "entry\ndone\n");
    assert_eq!(
        run(&binary, &["--load"]),
        "entry\na start\nb start undefined\na end\na-runtime function\ndone\n"
    );
}

/// The same cycle shape reached from ESM through `createRequire`, next to an
/// ESM partner whose export is assigned (not declared) at module run time.
#[test]
fn esm_conditional_require_cycle_keeps_runtime_assigned_bindings() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("partner.mjs"),
        "export let handler;\nhandler = () => 'partner-runtime';\nexport function readLater() { return typeof handler; }\n",
    )
    .unwrap();
    std::fs::write(
        root.join("dep.cjs"),
        "console.log('dep');\nconst cyc = require('./cycle.cjs');\nmodule.exports = { run: () => cyc.call() };\n",
    )
    .unwrap();
    std::fs::write(
        root.join("cycle.cjs"),
        "const dep = require('./dep.cjs');\nlet fn;\nfn = () => 'cycle-runtime:' + typeof dep;\nexports.call = () => fn();\n",
    )
    .unwrap();
    std::fs::write(
        root.join("entry.mjs"),
        r#"
import { createRequire } from 'node:module';
import { readLater } from './partner.mjs';
const require = createRequire(import.meta.url);
console.log('entry');
if (process.argv.includes('--load')) {
    const dep = require('./dep.cjs');
    console.log(dep.run(), readLater());
}
console.log('done');
"#,
    )
    .unwrap();
    let binary = compile(root, "entry.mjs");
    assert_eq!(run(&binary, &[]), "entry\ndone\n");
    assert_eq!(
        run(&binary, &["--load"]),
        "entry\ndep\ncycle-runtime:object function\ndone\n"
    );
}

/// A `do…while` test runs only if the body falls through, so a `require` in
/// either half is conditional. Node never evaluates the dependency below.
#[test]
fn do_while_require_stays_at_its_call_site() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("dep.cjs"),
        "console.log('dependency');\nmodule.exports = 7;",
    )
    .unwrap();
    std::fs::write(
        root.join("entry.cjs"),
        r#"
console.log('entry');
if (!process.argv.includes('--load')) {
    do { break; } while (require('./dep.cjs'));
} else {
    let seen = 0;
    do { seen += require('./dep.cjs'); } while (false);
    console.log('sum', seen);
}
console.log('done');
"#,
    )
    .unwrap();
    let binary = compile(root, "entry.cjs");
    assert_eq!(run(&binary, &[]), "entry\ndone\n");
    assert_eq!(
        run(&binary, &["--load"]),
        "entry\ndependency\nsum 7\ndone\n"
    );
}
