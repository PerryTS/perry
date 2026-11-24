//! `new.target` is the constructor being called: the one whose body reads it,
//! a class derived from another value (`super()` passes it along), or the new
//! target of a `Reflect.construct`. An object created on its `prototype`
//! reaches that class's methods as `this`.

use super::*;

const MAKE: &str = "const o: any = Object.create(new.target.prototype); o.other = 'ok'; return o;";

/// The method runs, and its `this` is not proven to be a class instance only.
fn reads_created_object(src: &str) -> bool {
    let ast = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&ast, "t", "t.ts").expect("lowers");
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    let pf = compute(&[("t.ts".to_string(), &hir)], &cfg);
    matches!(site(&pf, "ready").verdict, Verdict::No(_))
}

#[test]
fn the_class_whose_body_reads_it() {
    assert!(reads_created_object(&format!(
        "class B {{ read(): any {{ return (this as any).ready; }} constructor() {{ {MAKE} }} }} console.log((new B() as any).read());"
    )));
}

#[test]
fn a_derived_class_through_super() {
    assert!(reads_created_object(&format!(
        "class B {{ constructor() {{ {MAKE} }} }} class W extends B {{ read(): any {{ return (this as any).ready; }} }} console.log(new W().read());"
    )));
}

#[test]
fn a_function_constructor() {
    assert!(reads_created_object(&format!(
        "function F(this: any): any {{ {MAKE} }} F.prototype.read = function (): any {{ return this.ready; }}; console.log(new (F as any)().read());"
    )));
}

#[test]
fn a_reflect_construct_new_target() {
    assert!(reads_created_object(&format!(
        "class B {{ constructor() {{ {MAKE} }} }} class A {{ read(): any {{ return (this as any).ready; }} }} console.log((Reflect.construct(B, [], A) as any).read());"
    )));
}
