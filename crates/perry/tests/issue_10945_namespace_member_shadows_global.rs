//! Regression test for #10945: a NAMESPACE import registered every member of
//! the imported module in the flat `import_function_prefixes` map as a
//! best-effort fallback for bare-name calls (#5927). For a member named like a
//! global intrinsic, that entry hijacked the importer's own bare global:
//! `lower_new` found the name in the map and constructed the member instead of
//! the builtin, so `new Array(n).fill(0).length` came back `undefined`.
//!
//! effect's `src/Array.ts` exports `const Array = globalThis.Array`, and its
//! `Chunk.ts`, `Cron.ts` and `internal/effect.ts` do `import * as Arr from
//! "./Array.ts"` and then write `new Array(n)`. In the full OpenCode v1.18.30
//! build that referenced Array.ts's wrapper under a name the owning module
//! never emits, and the link failed.
//!
//! A namespace import binds exactly ONE name, the namespace, so a bare `Array`
//! in the importer is the global. Members still resolve through the namespace
//! (`Arr.Array`, `Arr.bump`), and an EXPLICIT named import of a
//! global-shadowing name (`import { Map }`) must still win — #10356's control.

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

/// Mirrors effect's `src/Array.ts`: a member named like the global it aliases,
/// next to an ordinary function member.
const ARRAY_MOD_SOURCE: &str = r#"
export const Array = globalThis.Array
export function bump(x: number) { return x + 1 }
"#;

/// The explicit-import control: a module whose `Map` IS imported by name.
const FAKE_MAP_SOURCE: &str = r#"
export class Map {
  readonly kind = "explicitly-imported-map"
}
"#;

const SHADOW_SOURCE: &str = r#"
import { Map } from "./fake_map.js"
export const explicitKind = new Map().kind
"#;

/// Uses the bare globals inside a function and a closure, as effect does.
const MAIN_SOURCE: &str = r#"
import * as Arr from "./array_mod.js"
import { explicitKind } from "./shadow.js"

function sized(n: number) {
  return new Array(n).fill(0).length
}
const pair = (a: number, b: number) => Array.of(a, b).join("+")

console.log("1 new Array(n):", sized(3))
console.log("2 Array.isArray:", Array.isArray([1]), Array.isArray("x"))
console.log("3 Array.from:", Array.from("abc").length)
console.log("4 Array.of in a closure:", pair(4, 5))
console.log("5 Arr.Array is the global:", Arr.Array === globalThis.Array)
console.log("6 new Arr.Array:", new Arr.Array(2).length)
console.log("7 Arr.bump:", Arr.bump(41))
console.log("8 explicit-import:", explicitKind)
"#;

/// Byte-for-byte what node 26.5.1 prints.
const EXPECTED: &str = "\
1 new Array(n): 3
2 Array.isArray: true false
3 Array.from: 3
4 Array.of in a closure: 4+5
5 Arr.Array is the global: true
6 new Arr.Array: 2
7 Arr.bump: 42
8 explicit-import: explicitly-imported-map
";

#[test]
fn namespace_member_does_not_shadow_a_global_intrinsic() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("array_mod.ts"), ARRAY_MOD_SOURCE).unwrap();
    std::fs::write(root.join("fake_map.ts"), FAKE_MAP_SOURCE).unwrap();
    std::fs::write(root.join("shadow.ts"), SHADOW_SOURCE).unwrap();
    std::fs::write(root.join("main.ts"), MAIN_SOURCE).unwrap();

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
        "namespace-shadowing probe must compile; stdout:\n{}\nstderr:\n{}",
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
    let stdout = String::from_utf8(run.stdout).expect("UTF-8 stdout");
    assert_eq!(
        stdout, EXPECTED,
        "a namespace import binds only the namespace: a member named like a \
         global must not shadow that global in the importer"
    );
}
