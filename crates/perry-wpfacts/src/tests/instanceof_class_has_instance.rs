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
