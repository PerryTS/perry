//! Regression test: `new` on a value that IS the global `RegExp` constructor,
//! reached through a local rebinding rather than the literal `RegExp`
//! identifier, must produce a real, correctly-branded regex instance.
//!
//! This is the shape of `@socketsecurity/lib`'s rolldown-bundled
//! `dist/primordials/regexp.js` (`const RegExpCtor = RegExp; exports
//! .RegExpCtor = RegExpCtor;`), consumed elsewhere as
//! `new _p_RegExpCtor(pattern)`.
//!
//! Pre-fix, `identify_global_builtin_constructor` never recognized RegExp's
//! own constructor thunk, so a rebound `RegExp` value fell through to the
//! generic empty-object construction path instead of the `"RegExp"` arm
//! already in `construct.rs`. The resulting object wasn't registered as a
//! real regex, so reading `.source`/`.flags` off it threw `TypeError: get
//! RegExp.prototype.source called on incompatible receiver`, and `.test()`
//! would have silently misbehaved rather than matching.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &std::path::Path, source: &str) -> String {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
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

    let run = Command::new(&output)
        .current_dir(dir)
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(
        run.stderr.is_empty(),
        "compiled binary wrote to stderr\nstderr:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

#[test]
fn rebound_regexp_constructor_produces_real_regex() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const RegExpCtor = RegExp;
const re = new RegExpCtor("^\\.[a-z]+$", "i");
console.log("source:", re.source);
console.log("flags:", re.flags);
console.log("test-match:", re.test(".JS"));
console.log("test-nomatch:", re.test("nope"));
console.log("instanceof:", re instanceof RegExp);
"#,
    );
    assert_eq!(
        stdout,
        "source: ^\\.[a-z]+$\nflags: i\ntest-match: true\ntest-nomatch: false\ninstanceof: true\n"
    );
}

/// The exact socket-lib shape: derive a second regex's source from an
/// already-rebound-constructed one (`rSlash.source` inside another `new
/// RegExpCtor(...)` call) — the concrete pattern from `@npmcli/promise-spawn`'s
/// bundled `dist` that surfaced this bug.
#[test]
fn rebound_regexp_source_composes_into_another_rebound_regexp() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const RegExpCtor = RegExp;
const rSlash = new RegExpCtor("[/]");
const rRel = new RegExpCtor(`^\\.${rSlash.source}`);
console.log("rSlash-source:", rSlash.source);
console.log("rRel-test:", rRel.test("./foo"));
"#,
    );
    assert_eq!(stdout, "rSlash-source: [/]\nrRel-test: true\n");
}
