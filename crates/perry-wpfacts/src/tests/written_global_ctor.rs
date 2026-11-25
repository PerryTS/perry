//! `new C(...)` by the name of a realm constructor (`CustomEvent`) after a
//! program store replaced `globalThis.C` with a program class: the global
//! binding holds that class (ECMA-262 10.1.9.2), so `new C(...)` constructs
//! it (13.3.5.1 EvaluateNew: GetValue of the identifier, then
//! [[Construct]] of that value), and the realm constructor where the store
//! did not run. Both are modeled values: the result and the arguments keep
//! their facts; nothing reaches unknown code. (perry-web browser.ts: the
//! DOM classes stored onto the global object in an `Object.entries` loop;
//! three.js `new CustomEvent('observe', { detail: this })`.)
//!
//! A delete that may reach the global object under any key (E7 through a
//! value that may be `globalThis`: three.js `delete stateMap[wireframe]`)
//! stores no value (ECMA-262 13.5.1.2, OrdinaryDelete 10.1.10): the binding
//! holds the realm constructor or is absent, and `new` of an absent binding
//! throws a realm ReferenceError (6.2.5.5 GetValue) before any argument is
//! handed anywhere. So the realm constructor's declaration still holds
//! (`builtins::native_value_written`).
use super::*;

fn whole(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

fn escape_with_field(pf: &ProgramFacts, key: &str) -> Option<String> {
    pf.templates
        .iter()
        .filter(|t| t.display.starts_with("t.ts"))
        .find(|t| t.fields.iter().any(|f| f.key == key))
        .unwrap_or_else(|| panic!("no t.ts template with field {key}\n{}", pf.dump_text()))
        .escape
        .clone()
}

fn reads_proven(pf: &ProgramFacts, key: &str) {
    let v: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == key && !s.is_call)
        .map(|s| s.verdict.clone())
        .collect();
    assert!(
        !v.is_empty() && v.iter().all(|v| !matches!(v, Verdict::No(_))),
        "reads of {key}: {v:?}\n{}",
        pf.dump_text()
    );
}

const USE: &str = r#"
const init = { pb: 2, detail: 0 };
const e: any = new CustomEvent("x", init);
console.log(init.pb + 1, e.type);
"#;

const CLASS: &str = r#"
class Ev { pc: number; constructor(t: string, i: any) { this.pc = 1; } }
"#;

fn check(store: &str) {
    let src = format!("{CLASS}\n{store}\n{USE}");
    let pf = whole(&src);
    assert!(
        !pf.dump_text().contains("argument of written constructor"),
        "{store}\n{}",
        pf.dump_text()
    );
    assert_eq!(escape_with_field(&pf, "pb"), None, "{store}\n{}", pf.dump_text());
    reads_proven(&pf, "pb");
}

/// perry-web browser.ts:316: the DOM classes stored onto the global object
/// under the keys of a record literal, where the global is undefined.
#[test]
fn entries_loop_store_then_new_by_name() {
    check(
        r#"
const g: any = globalThis;
const classes: Record<string, unknown> = { Event: Ev, CustomEvent: Ev };
for (const [k, v] of Object.entries(classes)) if (k === "Event" || typeof g[k] === "undefined") g[k] = v;
"#,
    );
}

/// A named store onto the declared writable data global.
#[test]
fn named_store_then_new_by_name() {
    check("(globalThis as any).CustomEvent = Ev;");
}

/// A store under a String key the solver cannot name.
#[test]
fn computed_store_then_new_by_name() {
    check(
        r#"
function put(k: string, v: unknown) { (globalThis as any)[k] = v; }
put(String(Date.now()) === "" ? "a" : "CustomEvent", Ev);
"#,
    );
}

/// RED on e4e552f2e1: a delete under a computed key of a value that may be
/// the global object (three.js `delete stateMap[wireframe]` with
/// `globalThis` among `stateMap`'s values) made `new CustomEvent(...)` the
/// unknown constructor ("argument of written constructor").
#[test]
fn computed_delete_on_global_then_new_by_name() {
    for del in [
        "for (const k in o) delete o[k];",
        "function drop(x: any, k: string) { delete x[k]; }\ndrop(o, String(Date.now()));",
    ] {
        check(&format!(
            "const o: any = Date.now() > 0 ? globalThis : {{ q: 1 }};\n{del}"
        ));
    }
}

/// The same with a named delete (`delete globalThis.CustomEvent`).
#[test]
fn named_delete_on_global_then_new_by_name() {
    check("delete (globalThis as any).CustomEvent;");
}
