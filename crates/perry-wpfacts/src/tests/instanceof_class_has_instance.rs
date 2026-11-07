//! `x instanceof C` with `C` a declared class name (HIR `InstanceOf` with
//! `ty` and no `ty_expr`): codegen tests against the class VALUE
//! (`js_instanceof_dynamic`), so InstanceofOperator runs the
//! `@@hasInstance` the class object holds, also one installed by
//! `Object.defineProperty` (SOUND9 H: class_static_has_instance_inherited,
//! instanceof_miss_ladder, 10502_s5).
use super::*;

fn facts(src: &str) -> crate::ProgramFacts {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], &FactsConfig::default());
    assert!(pf.body_reach.complete, "activated");
    pf
}

/// The program has one closure (id 0): the installed `@@hasInstance`.
fn hook_active(pf: &crate::ProgramFacts) -> bool {
    pf.body_reach.bodies.iter().any(|(_, i, _)| *i == 0)
}

#[test]
fn a_defined_has_instance_runs_for_a_class_name() {
    let pf = facts(
        r#"class Odd {}
Object.defineProperty(Odd, Symbol.hasInstance, { value: (n: unknown) => typeof n === "number" && n % 2 === 1 });
console.log("Odd", 3 instanceof Odd);"#,
    );
    assert!(hook_active(&pf), "@@hasInstance of Odd inactive");
}

#[test]
fn a_has_instance_under_a_computed_key_runs_for_a_class_name() {
    let pf = facts(
        r#"class Odd {}
const s: any = process.argv.length > 99 ? "x" : Symbol.hasInstance;
Object.defineProperty(Odd, s, { value: (n: unknown) => typeof n === "number" && n % 2 === 1 });
console.log("Odd", 3 instanceof Odd);"#,
    );
    assert!(hook_active(&pf), "@@hasInstance of Odd inactive");
}

/// Control: with no `instanceof` nothing calls the installed function.
#[test]
fn a_has_instance_no_instanceof_reaches_stays_inactive() {
    let pf = facts(
        r#"class Odd {}
Object.defineProperty(Odd, Symbol.hasInstance, { value: (n: unknown) => typeof n === "number" && n % 2 === 1 });
console.log("Odd", typeof Odd);"#,
    );
    assert!(!hook_active(&pf), "nothing runs @@hasInstance");
}

/// The verdict of the one `p.f()` call in `probe`.
fn probe_call(pf: &crate::ProgramFacts) -> Verdict {
    let calls: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == "f" && s.is_call)
        .collect();
    assert_eq!(calls.len(), 1, "{}", pf.dump_text());
    calls[0].verdict.clone()
}

/// InstanceofOperator calls the hook with the left-hand side
/// (`Call(instOfHandler, C, « V »)`): what the hook stores on it is a store
/// on that value. `holder.f` holds the literals method and the hooks.
const REPLACES_F: &str = r#"const o: any = { f() { return 1; } };
function probe(p: any) { return p.f(); }
console.log(probe(o), o instanceof RHS, probe(o));"#;

fn hook_on(base: &str, rhs: &str, body: &str) -> crate::ProgramFacts {
    facts(&format!(
        "{base}\nObject.defineProperty(Base, Symbol.hasInstance, {{ value: (v: any) => {{ {body} return true; }} }});\n{}",
        REPLACES_F.replace("RHS", rhs)
    ))
}

#[test]
fn the_has_instance_hook_receives_the_left_hand_side() {
    let pf = hook_on("class Base {}", "Base", "v.f = function () { return 7; };");
    assert_eq!(probe_call(&pf), Verdict::T0Set, "{}", pf.dump_text());
}

#[test]
fn an_inherited_has_instance_hook_receives_the_left_hand_side() {
    let pf = hook_on(
        "class Base {}\nclass Sub extends Base {}",
        "Sub",
        "v.f = function () { return 7; };",
    );
    assert_eq!(probe_call(&pf), Verdict::T0Set, "{}", pf.dump_text());
}

/// Control: a hook that writes nothing on its argument leaves the call
/// monomorphic.
#[test]
fn a_reading_has_instance_hook_keeps_the_call_proven() {
    let pf = hook_on("class Base {}", "Base", "if (v.f === 0) return false;");
    assert_eq!(probe_call(&pf), Verdict::T0Mono, "{}", pf.dump_text());
}

/// A right-hand side from unmodeled code runs an unknown hook: the
/// left-hand side escapes.
#[test]
fn an_unknown_right_hand_side_escapes_the_left_hand_side() {
    let pf = facts(&REPLACES_F.replace("RHS", "(globalThis as any)[process.argv[2]]"));
    assert!(
        matches!(probe_call(&pf), Verdict::No(_)),
        "{}",
        pf.dump_text()
    );
}
