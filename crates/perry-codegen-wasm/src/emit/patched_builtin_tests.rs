//! A program that replaces a builtin namespace member on the web target
//! (#15104). Expected output is Node's.

use super::run_web_source;

/// `run_web_source` with the whole-program scan of `source` installed, as
/// `perry compile` installs it before lowering.
fn run_patching_source(source: &str) -> String {
    let ast = perry_parser::parse_typescript(source, "patch.ts").unwrap();
    let mut set = perry_hir::PatchedBuiltins::new();
    perry_hir::scan_patched_builtins(&ast, &mut set);
    perry_hir::set_patched_builtins(std::sync::Arc::new(set));
    let out = run_web_source(source);
    perry_hir::clear_patched_builtins();
    out
}

// Every call of a replaced member reads it off `globalThis.Math`, the call
// before the store included.
#[test]
fn a_direct_patch_calls_the_replacement() {
    assert_eq!(
        run_patching_source(
            r#"
console.log("before patch: " + Math.max(1, 2));
(Math as any).max = (a: number, b: number) => -1;
console.log("typeof Math.max: " + typeof (Math as any).max);
console.log("patched Math.max(1, 2): " + Math.max(1, 2));
(Math as any).floor = (x: number) => 42;
(JSON as any).stringify = (v: any) => "patched";
console.log("floor: " + Math.floor(1.5) + ", stringify: " + JSON.stringify({ a: 1 }));
"#
        ),
        "before patch: 2\ntypeof Math.max: function\npatched Math.max(1, 2): -1\nfloor: 42, stringify: patched"
    );
}

// The runtime glue shares the host `Math`; a Perry closure or another host
// function stored there leaves the glue's own calls alone.
#[test]
fn a_computed_key_patch_leaves_the_runtime_glue_alone() {
    assert_eq!(
        run_patching_source(
            r#"
const k = ["m", "a", "x"].join("");
(Math as any)[k] = (a: number, b: number) => -1;
console.log("typeof: " + typeof (Math as any).max + ", " + Math.max(1, 2) + ", " + Math.max(1, 2, 3));
(Math as any)[k] = (Math as any)["m" + "in"];
console.log("host min: " + Math.max(1, 2) + ", " + Math.max(1, 2, 3));
"#
        ),
        "typeof: function, -1, -1\nhost min: 1, 1"
    );
}

// An async body reads the replaced member through the same global object.
#[test]
fn an_async_body_calls_the_replacement() {
    assert_eq!(
        run_patching_source(
            r#"
(Math as any).max = (a: number, b: number) => -1;
async function f(): Promise<number> {
  await null;
  return Math.max(1, 2) + Math.floor(2.5);
}
f().then((v) => console.log("async: " + v));
"#
        ),
        "async: 1"
    );
}
