use super::*;

#[test]
fn an_escaped_namespace_function_exposes_its_returned_array_elements() {
    let a =
        "export function make() { const r = { payload: 1 }; console.log(r.payload); return [r]; }";
    let b = "import * as api from './a'; (globalThis as any).keep(api);";
    let mods = [("a", a), ("b", b)];
    for (mode, pf) in [("whole", whole_of(&mods)), ("unit", units_of(&mods))] {
        let lits: Vec<_> = pf
            .templates
            .iter()
            .filter(|t| {
                t.display.starts_with("a.ts:") && t.fields.iter().any(|f| f.key == "payload")
            })
            .collect();
        assert!(!lits.is_empty(), "live payload ({mode})");
        assert!(
            lits.iter().all(|t| t.escape.is_some()),
            "{mode}: unknown code can call api.make(), retain its element and change payload\n{}",
            pf.dump_text()
        );
    }
}

#[test]
fn an_escaped_field_function_exposes_its_returned_array_elements() {
    let a = "export const api = { make: () => { const r = { payload: 1 }; console.log(r.payload); return [r]; } };";
    let b = "import { api } from './a'; (globalThis as any).keep(api);";
    let mods = [("a", a), ("b", b)];
    for (mode, pf) in [("whole", whole_of(&mods)), ("unit", units_of(&mods))] {
        let lits: Vec<_> = pf
            .templates
            .iter()
            .filter(|t| {
                t.display.starts_with("a.ts:") && t.fields.iter().any(|f| f.key == "payload")
            })
            .collect();
        assert!(!lits.is_empty(), "live payload ({mode})");
        assert!(
            lits.iter().all(|t| t.escape.is_some()),
            "{mode}: unknown code can call api.make(), retain its element and change payload\n{}",
            pf.dump_text()
        );
    }
}

#[test]
fn an_escaped_open_function_exposes_what_it_stores_in_its_argument() {
    let a = "export function fill(xs: any[]) { const r = { payload: 1 }; console.log(r.payload); xs.push(r); return xs; }";
    let b = "import { fill } from './a'; (globalThis as any).keep(fill);";
    let mods = [("a", a), ("b", b)];
    for (mode, pf) in [("whole", whole_of(&mods)), ("unit", units_of(&mods))] {
        let lits: Vec<_> = pf
            .templates
            .iter()
            .filter(|t| {
                t.display.starts_with("a.ts:") && t.fields.iter().any(|f| f.key == "payload")
            })
            .collect();
        assert!(!lits.is_empty(), "live payload ({mode})");
        assert!(
            lits.iter().all(|t| t.escape.is_some()),
            "{mode}: unknown code can call fill on its own array and retain the element\n{}",
            pf.dump_text()
        );
    }
}

#[test]
fn an_escaped_namespace_exposes_nested_result_objects() {
    let src = "export namespace Core { export function make(x: any) { if (x) return x; return [{ definition: { payload: 1 } }]; } } const foreign: any = (globalThis as any).o; foreign[String(process.argv[0])] = 1; const r = Core.make((globalThis as any).x); console.log(r[0].definition.payload);";
    for (mode, pf) in [("whole", facts(src)), ("unit", units(src))] {
        let lits: Vec<_> = pf
            .templates
            .iter()
            .filter(|t| t.fields.iter().any(|f| f.key == "payload"))
            .collect();
        assert!(!lits.is_empty(), "live payload ({mode})");
        assert!(lits.iter().all(|t| t.escape.is_some()), "{mode}: a possibly replaced namespace callee retains Core and can call its other fields\n{}", pf.dump_text());
    }
}

#[test]
fn an_unknown_function_import_on_a_cycle_backedge_exposes_its_result() {
    let a = "import { api } from './b'; export function make() { if (process.argv.length > 99) api.make(); return { payload: 1 }; }";
    let b = "import { make } from './a'; export const api = { make }; if ((globalThis as any).keep) (globalThis as any).keep(api);";
    let m = "import { api } from './b'; console.log(api.make().payload);";
    let mods = [("a", a), ("b", b), ("m", m)];
    for (mode, pf) in [("whole", whole_of(&mods)), ("unit", units_of(&mods))] {
        let lits: Vec<_> = pf
            .templates
            .iter()
            .filter(|t| {
                t.display.starts_with("a.ts:") && t.fields.iter().any(|f| f.key == "payload")
            })
            .collect();
        assert!(!lits.is_empty(), "live owner result ({mode})");
        assert!(lits.iter().all(|t| t.escape.is_some()), "{mode}: a function read without its contract can be called and its result retained\n{}", pf.dump_text());
    }
}

#[test]
fn an_opaque_builtin_value_can_return_its_receiver() {
    // reverse stored as a value is an opaque builtin callee. Returning
    // the holder makes its fields available through the unknown result.
    let pf = facts("const reverse = [].reverse; const child = { payload: 1 }; const holder = { child, length: 0, f: reverse }; const retained: any = holder.f(); retained.child.payload = 9; console.log(child.payload);");
    let child = pf
        .templates
        .iter()
        .find(|t| t.fields.iter().any(|f| f.key == "payload"))
        .expect("child layout");
    assert!(child.escape.is_some(), "{}", pf.dump_text());
}
