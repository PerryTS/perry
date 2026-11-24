//! `SiteFacts::closed_methods`: the receiver's class names the body only where
//! nothing can put another value under the key.

use super::*;

fn closed(src: &str) -> bool {
    let ast = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&ast, "t", "t.ts").expect("lowers");
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    let pf = compute(&[("t.ts".to_string(), &hir)], &cfg);
    let calls: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == "m" && s.is_call)
        .collect();
    assert!(!calls.is_empty(), "no call of m");
    calls.iter().all(|s| s.closed_methods)
}

const CLASSES: &str = "class A { m() { return 1; } z() { return 5; } } class B extends A { m() { return 2; } } class C extends A {}
const k: string = process.argv[2] || 'm';
function call(x: A) { return x.m(); }
";

#[test]
fn declared_methods_own_and_inherited() {
    assert!(closed(&format!(
        "{CLASSES} console.log(call(new A()), call(new B()), call(new C()));"
    )));
}

#[test]
fn a_computed_write_elsewhere_keeps_it() {
    assert!(closed(&format!(
        "{CLASSES} const o: any = {{}}; o[k] = () => 3; console.log(call(new A()), call(new B()), o);"
    )));
}

#[test]
fn a_computed_write_to_a_prototype_opens_it() {
    assert!(!closed(&format!(
        "{CLASSES} (A.prototype as any)[k] = () => 3; console.log(call(new A()), call(new B()));"
    )));
}

#[test]
fn a_computed_own_write_opens_it() {
    assert!(!closed(&format!(
        "{CLASSES} const a: any = new A(); a[k] = () => 3; console.log(call(a), call(new B()));"
    )));
}

#[test]
fn another_method_of_the_class_opens_it() {
    assert!(!closed(&format!(
        "{CLASSES} (A.prototype as any)[k] = A.prototype.z; console.log(call(new A()), call(new B()));"
    )));
}
