//! BUGFIX2 (2026-11-17): `Object.hasOwn`, `hasOwnProperty` and
//! `propertyIsEnumerable` on a Proxy run [[GetOwnProperty]] (ECMA-262
//! 10.5.5), which calls the handler's `getOwnPropertyDescriptor` trap. The
//! trap body must be reachable (it was compiled as unreachable: SIGILL).
use super::*;

fn trap_inactive(call: &str) -> bool {
    let src = format!(
        "function gopdTrap(t: any, k: any): any {{ return Reflect.getOwnPropertyDescriptor(t, k); }}\n\
const hop = Object.prototype.hasOwnProperty;\n\
const pie = Object.prototype.propertyIsEnumerable;\n\
const px: any = new Proxy({{ x: 1 }} as any, {{ getOwnPropertyDescriptor: gopdTrap }});\n\
const key = String(process.argv[2]);\n\
console.log({call});\n"
    );
    let m = perry_parser::parse_typescript(&src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&m, "t", "t.ts").expect("lowers");
    let id = hir
        .functions
        .iter()
        .find(|f| f.name == "gopdTrap")
        .expect("gopdTrap")
        .id;
    let pf = facts(&src);
    assert!(pf.body_reach.complete, "{call}");
    pf.body_reach
        .inactive(&("t.ts".to_string(), id, String::new()))
}

#[test]
fn own_prop_runs_the_gopd_trap() {
    for call in [
        "Object.hasOwn(px, \"x\")",
        "Object.hasOwn(px, key)",
        "px.hasOwnProperty(\"x\")",
        "hop.call(px, key)",
        "pie.call(px, \"x\")",
        "(() => { const f = Object.hasOwn; return f(px, key); })()",
    ] {
        assert!(!trap_inactive(call), "trap unreachable for {call}");
    }
}

// The control: an operation that never asks for an own descriptor leaves
// the trap body inactive, so the test above discriminates.
#[test]
fn own_prop_control_has_runs_no_gopd_trap() {
    assert!(trap_inactive("\"x\" in px"));
}
