//! `Object.setPrototypeOf(O, proto)` returns O itself (ECMA-262 20.1.2.23
//! step 5; a primitive O is returned unchanged, step 3); only O's
//! [[Prototype]] changes, so absent-key reads on O go through `proto`.
//! `Reflect.setPrototypeOf` returns a boolean (28.1.13). tssrc: every type
//! mapper passes through `Debug.attachDebugPrototypeIfDebug` (`if
//! (isDebugging) return Object.setPrototypeOf(mapper, D.prototype); return
//! mapper;`), and `mapper.targets[i]()` calls the deferred mapper's thunks.

use super::*;

/// The escape of the `t.ts` template that has an own field `key`.
fn escape_with_field(pf: &ProgramFacts, key: &str) -> Option<String> {
    pf.templates
        .iter()
        .filter(|t| t.display.starts_with("t.ts"))
        .find(|t| t.fields.iter().any(|f| f.key == key))
        .unwrap_or_else(|| panic!("no t.ts template with field {key}\n{}", pf.dump_text()))
        .escape
        .clone()
}

/// The mappers of the checker: made through `attach`, whose debug branch
/// (a condition the solver cannot decide) changes the chain.
const MAPPERS: &str = r#"
class D { hook(x: any) { return 1; } }
function attach(m: any): any {
    if (process.argv.length > 9) return Object.setPrototypeOf(m, D.prototype);
    return m;
}
function simple(source: any, target: any) { return attach({ kind: 0, source, target }); }
function deferred(sources: any[], targets: any[]) { return attach({ kind: 3, sources, targets }); }
const arg = { pb: 1 };
const s = simple(1, 2);
const m = deferred([1, 2], [(x: any) => x.pb, (x: any) => x.pb + 1]);
const i = process.argv.length & 1;
"#;

#[test]
fn a_mapper_through_set_prototype_of_calls_its_own_thunks() {
    let src = format!("{MAPPERS}\nconsole.log(s.kind, m.kind, m.targets[i](arg));\n");
    let pf = facts(&src);
    assert_eq!(escape_with_field(&pf, "pb"), None, "{}", pf.dump_text());
}

/// Control: the mapper handed to unknown code may get any `targets`.
#[test]
fn a_mapper_given_to_unknown_code_calls_unknown_thunks() {
    let src = format!(
        "{MAPPERS}\nconst u: any = new Function(process.argv[2]);\nu(m);\nconsole.log(m.targets[i](arg));\n"
    );
    let pf = facts(&src);
    assert!(escape_with_field(&pf, "pb").is_some(), "{}", pf.dump_text());
}

/// Control: after the change, an absent key is read from the NEW chain:
/// `o.hook` is `D.prototype.hook`, which hands its argument to unknown code.
#[test]
fn a_read_through_the_new_prototype_finds_its_member() {
    let src = r#"
const u: any = new Function(process.argv[2]);
class D { hook(x: any) { u(x); return 1; } }
const argw = { pw: 1 };
const o: any = { kind: 1 };
const p: any = Object.setPrototypeOf(o, D.prototype);
console.log(p.hook(argw), o.kind);
"#;
    let pf = facts(src);
    assert!(escape_with_field(&pf, "pw").is_some(), "{}", pf.dump_text());
}

/// Control: an unknown prototype leaves inherited reads unknown.
#[test]
fn an_unknown_prototype_leaves_inherited_reads_unknown() {
    let src = r#"
const q: any = new Function(process.argv[2])();
const argv = { pv: 1 };
const o: any = Object.setPrototypeOf({ kind: 1 }, q);
console.log(o.hook(argv));
"#;
    let pf = facts(src);
    assert!(escape_with_field(&pf, "pv").is_some(), "{}", pf.dump_text());
}

/// `Reflect.setPrototypeOf` returns a boolean, not its target: handing the
/// result to unknown code hands it no object.
#[test]
fn reflect_set_prototype_of_returns_a_boolean() {
    let src = r#"
class D { hook(x: any) { return 1; } }
const u: any = new Function(process.argv[2]);
const o4: any = { pr: 1 };
const b: any = Reflect.setPrototypeOf(o4, D.prototype);
u(b);
console.log(o4.pr);
"#;
    let pf = facts(src);
    assert_eq!(escape_with_field(&pf, "pr"), None, "{}", pf.dump_text());
}

/// Control: the target of `Reflect.setPrototypeOf` still gets the new chain.
#[test]
fn reflect_set_prototype_of_changes_the_chain() {
    let src = r#"
const u: any = new Function(process.argv[2]);
class D { hook(x: any) { u(x); return 1; } }
const argw = { pw: 1 };
const o: any = { kind: 1 };
Reflect.setPrototypeOf(o, D.prototype);
console.log(o.hook(argw));
"#;
    let pf = facts(src);
    assert!(escape_with_field(&pf, "pw").is_some(), "{}", pf.dump_text());
}

/// tssrc shape: the mapper factory is a namespace member (`Debug.attach..`,
/// assigned in the namespace body), and some store goes through ⊤.
const NS_MAPPERS: &str = r#"
class D { hook(x: any) { return 1; } }
namespace NS {
    export let dbg = false;
    export function attach(m: any): any {
        if (dbg) return Object.setPrototypeOf(m, D.prototype);
        return m;
    }
}
function deferred(sources: any[], targets: any[]) { return NS.attach({ kind: 3, sources, targets }); }
const arg = { pb: 1 };
const m = deferred([1, 2], [(x: any) => x.pb, (x: any) => x.pb + 1]);
const i = process.argv.length & 1;
"#;

#[test]
fn a_namespace_mapper_calls_its_own_thunks() {
    let src = format!("{NS_MAPPERS}\nconsole.log(m.kind, m.targets[i](arg));\n");
    let pf = facts(&src);
    assert_eq!(escape_with_field(&pf, "pb"), None, "{}", pf.dump_text());
}

#[test]
fn a_namespace_mapper_after_a_store_through_top_calls_its_own_thunks() {
    let src = format!(
        "{NS_MAPPERS}\nconst U: any = new Function(process.argv[2])();\nU[process.argv[3]] = (q: any) => q;\nconsole.log(m.kind, m.targets[i](arg));\n"
    );
    let pf = facts(&src);
    assert_eq!(escape_with_field(&pf, "pb"), None, "{}", pf.dump_text());
}
