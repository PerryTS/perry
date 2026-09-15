//! Regression test for #10303: a member READ through Node's `global` alias
//! must see the same object `globalThis` does.
//!
//! `global` is a globalThis self-reference installed by
//! `populate_global_this_builtins`, so the bare identifier lowers to
//! `PropertyGet { GlobalGet(0), "global" }`. `member_tail.rs` undoes that
//! reroute in member-object position — the #973 fix that keeps
//! `Number.parseFloat === parseFloat` on the intrinsic static surface — and
//! its guard exempted `globalThis` but not `global`. So `global.<x>`
//! collapsed to the intrinsic `GlobalGet(0).<x>` lookup, which knows only the
//! built-in names and answered `undefined` for anything user code installed.
//!
//! Assignment never went through that path, so the write half worked and the
//! asymmetry hid the bug: `global.c = 3` then `globalThis.c` is 3 while
//! `global.c` is `undefined`.
//!
//! The second test is the control that makes the fix falsifiable in the other
//! direction: every intrinsic surface reached through `global` must still
//! resolve, because un-collapsing the receiver is only safe if it is exactly
//! what `globalThis.<same>` already does.
//!
//! It is not a PURE control. Fifteen of its sixteen cells already passed before
//! the fix; `new global.Map().size` answered `undefined` (bun: `0`), because
//! the collapsed `GlobalGet(0).Map` is the intrinsic namespace rather than the
//! reified constructor, so constructing it produced an object with no
//! collection backing. The same change fixes that, and this test pins it.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn runtime_dir() -> PathBuf {
    std::env::var_os("PERRY_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            perry_bin()
                .parent()
                .expect("compiler directory")
                .to_path_buf()
        })
}

const ALIAS_SOURCE: &str = r#"
// @ts-nocheck
globalThis.a = 1
const g = globalThis
g.b = 2
global.c = 3

console.log("alias read      ", g.a)
console.log("globalThis read ", globalThis.b)
console.log("globalThis.c    ", globalThis.c)
console.log("alias.c         ", g.c)
console.log("global.c        ", global.c)
console.log("global === gT   ", global === globalThis)

// @opentui/core's CliRenderer constructor, reduced.
global.requestAnimationFrame = (cb) => 7
const window = global.window
if (!window) global.window = {}
global.window.requestAnimationFrame = global.requestAnimationFrame
console.log("window rAF      ", typeof global.window.requestAnimationFrame)
"#;

const ALIAS_EXPECTED: &str = "\
alias read       1
globalThis read  2
globalThis.c     3
alias.c          3
global.c         3
global === gT    true
window rAF       function
";

const INTRINSIC_SOURCE: &str = r#"
// @ts-nocheck
console.log("Math.max        ", global.Math.max(1, 5, 3))
console.log("JSON.stringify  ", global.JSON.stringify({ a: 1 }))
console.log("Array.isArray   ", global.Array.isArray([1]))
console.log("Object.keys     ", JSON.stringify(global.Object.keys({ x: 1, y: 2 })))
console.log("parseInt        ", global.parseInt("42px"))
console.log("String(5)       ", global.String(5))
console.log("new Map().size  ", new global.Map().size)
console.log("parseFloat id   ", global.Number.parseFloat === parseFloat)
console.log("process.platform", typeof global.process.platform)
console.log("console.log     ", typeof global.console.log)
console.log("Date.now() > 0  ", global.Date.now() > 0)
"#;

const INTRINSIC_EXPECTED: &str = "\
Math.max         5
JSON.stringify   {\"a\":1}
Array.isArray    true
Object.keys      [\"x\",\"y\"]
parseInt         42
String(5)        5
new Map().size   0
parseFloat id    true
process.platform string
console.log      function
Date.now() > 0   true
";

fn compile_and_run(source: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("main.ts"), source).unwrap();

    let output = root.join("main_bin");
    let out = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(root.join("main.ts"))
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_RUNTIME_DIR", runtime_dir())
        .output()
        .expect("run perry compile");
    assert!(
        out.status.success(),
        "global-alias probe must compile; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary must run; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8(run.stdout).expect("UTF-8 stdout")
}

#[test]
fn global_alias_reads_see_user_installed_properties() {
    let stdout = compile_and_run(ALIAS_SOURCE);
    assert!(
        !stdout.contains("undefined"),
        "a property written through one spelling of the global object must be \
         readable through every other; stdout:\n{stdout}"
    );
    assert_eq!(
        stdout, ALIAS_EXPECTED,
        "byte-for-byte what node and bun print"
    );
}

#[test]
fn intrinsic_surfaces_through_global_are_unaffected() {
    assert_eq!(
        compile_and_run(INTRINSIC_SOURCE),
        INTRINSIC_EXPECTED,
        "un-collapsing the `global` receiver must not disturb the intrinsic \
         static surfaces it used to collapse to"
    );
}
