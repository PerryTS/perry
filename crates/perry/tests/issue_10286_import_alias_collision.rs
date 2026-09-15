//! Regression test for #10286: an import alias must not be claimed by another
//! import's origin export name.
//!
//! `imported_vars` tells codegen whether an identifier is a variable read or a
//! direct call, and codegen looks it up with the name an `ExternFuncRef`
//! carries — the LOCAL name in the aliased case. Registering the origin
//! module's exported name as well let one import claim an identifier the
//! importing module binds to something else: `import { t as e } from "./a"`
//! put "t" in the set on behalf of module a, so the unrelated
//! `import { extend as t } from "./b"` compiled `t(...)` as a read of a slot
//! that is not published until a's populator runs. Undefined during module
//! init, correct afterwards.
//!
//! Minifiers reuse short aliases across import statements, so this shape is
//! routine in dist bundles — the case that surfaced it was a four-statement
//! minified module in `opentui-spinner`.

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

/// Exports a *function*, reached through an alias that collides below.
const ORIGIN: &str = "\
var cat = { br: 1 };
function extend(o) { Object.assign(cat, o); }
function getCat() { return cat; }
export { getCat, extend, cat };
";

/// Exports a non-function binding whose name is literally `t`.
const LEAF: &str = "\
const t = { kind: \"obj-not-a-function\" };
export { t };
";

/// The minified shape: `t` is this module's alias for `extend`, while the
/// other import's ORIGIN export is also called `t`. The call runs during
/// module initialization, which is when the wrong classification bites.
const CONSUMER: &str = "\
import { t as e } from \"./leaf.mjs\";
import { extend as t } from \"./origin.js\";
function n() { t({ spinner: e }); }
n();
export { n as reg };
";

const ENTRY: &str = "\
import { getCat } from \"./origin.js\";
import { reg } from \"./consumer.mjs\";
console.log(typeof reg, JSON.stringify(getCat()));
";

/// Control: identical but the alias does not collide.
const CONSUMER_NO_COLLISION: &str = "\
import { t as e } from \"./leaf.mjs\";
import { extend as q } from \"./origin.js\";
function n() { q({ spinner: e }); }
n();
export { n as reg };
";

const EXPECTED: &str = "function {\"br\":1,\"spinner\":{\"kind\":\"obj-not-a-function\"}}\n";

fn compile_and_run(consumer: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("origin.js"), ORIGIN).unwrap();
    std::fs::write(root.join("leaf.mjs"), LEAF).unwrap();
    std::fs::write(root.join("consumer.mjs"), consumer).unwrap();
    std::fs::write(root.join("entry.ts"), ENTRY).unwrap();

    let output = root.join("entry_bin");
    let out = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(root.join("entry.ts"))
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_RUNTIME_DIR", runtime_dir())
        .output()
        .expect("run perry compile");
    assert!(
        out.status.success(),
        "aliased imports must compile; stdout:\n{}\nstderr:\n{}",
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
fn colliding_import_alias_still_calls_its_own_module() {
    let stdout = compile_and_run(CONSUMER);
    assert!(
        !stdout.contains("undefined"),
        "the alias must hold the imported function during module init; stdout:\n{stdout}"
    );
    assert_eq!(
        stdout, EXPECTED,
        "a local alias colliding with another import's origin export name must \
         still resolve to its own module, byte-for-byte as node and bun"
    );
}

#[test]
fn non_colliding_alias_control() {
    assert_eq!(
        compile_and_run(CONSUMER_NO_COLLISION),
        EXPECTED,
        "the non-colliding control must be unaffected"
    );
}
