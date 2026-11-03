//! A `new C(...)` is an instance of C only when no constructor on C's chain
//! returns an object: `return expr` with an object value makes that value the
//! result of `new` (and a primitive return is ignored). Per unit, the class
//! is another unit's stand-in: its owner publishes whether its chain can
//! return (`ClassContract::ctor_returns`), and the reader's `new` result is
//! unknown then. Per unit must never prove a read the whole program does not.

use super::*;

fn units(src: &str) -> ProgramFacts {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    crate::compute_units(&[("t.ts".to_string(), &hir)], &FactsConfig::default())
}

fn proven(pf: &ProgramFacts, key: &str) -> usize {
    pf.sites
        .iter()
        .filter(|s| s.key == key && matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set))
        .count()
}

/// Per unit proves the read of `x` only if the whole program does too, and
/// exactly when `sound` says the instance is the whole result.
fn check(src: &str, instance: bool) {
    let (pu, pw) = (units(src), facts(src));
    let (u, w) = (proven(&pu, "x"), proven(&pw, "x"));
    assert!(
        u <= w,
        "per unit proves {u} > whole {w}\n{}",
        pu.dump_text()
    );
    assert_eq!(u > 0, instance, "per unit: {u} proven\n{}", pu.dump_text());
}

#[test]
fn a_constructor_returning_an_object_is_not_an_instance() {
    check(
        r#"
        function mk(): any { return { z: 1 }; }
        class B { x = 5; constructor() { return mk(); } }
        const b: any = new B();
        console.log(b.x);
        "#,
        false,
    );
}

#[test]
fn an_inherited_constructor_returning_an_object_is_not_an_instance() {
    check(
        r#"
        function mk(): any { return { z: 1 }; }
        class C { constructor() { return mk(); } }
        class D extends C { x = 5; }
        const d: any = new D();
        console.log(d.x);
        "#,
        false,
    );
}

#[test]
fn a_conditional_object_return_is_not_an_instance() {
    check(
        r#"
        function mk(): any { return { z: 1 }; }
        class B { x = 5; constructor(k: number) { if (k > 1) return mk(); } }
        const b: any = new B(Date.now());
        console.log(b.x);
        "#,
        false,
    );
}

#[test]
fn a_returned_proxy_is_not_an_instance() {
    check(
        r#"
        class B { x = 5; constructor() { return new Proxy({}, {}); } }
        const b: any = new B();
        console.log(b.x);
        "#,
        false,
    );
}

#[test]
fn a_primitive_return_is_still_the_instance() {
    check(
        r#"
        class B { x = 5; constructor() { return 3 as any; } }
        const b = new B();
        console.log(b.x);
        "#,
        true,
    );
}

#[test]
fn returning_this_is_still_the_instance() {
    check(
        r#"
        class B { x = 5; constructor() { return this; } }
        const b = new B();
        console.log(b.x);
        "#,
        true,
    );
}

/// Two modules (`lib` imported by `main`), per unit and whole program.
fn two(lib: &str, main: &str) -> (ProgramFacts, ProgramFacts) {
    let mods = [("lib", lib), ("main", main)];
    let h: Vec<(String, perry_hir::Module)> = mods
        .iter()
        .map(|(name, src)| {
            let path = format!("{name}.ts");
            let module = perry_parser::parse_typescript(src, &path).expect("parses");
            let mut hir = perry_hir::lower_module(&module, name, &path).expect("lowers");
            for imp in &mut hir.imports {
                if let Some(n) = imp.source.strip_prefix("./") {
                    imp.resolved_path = Some(format!("{n}.ts"));
                }
            }
            (path, hir)
        })
        .collect();
    let list: Vec<(String, &perry_hir::Module)> = h.iter().map(|(p, m)| (p.clone(), m)).collect();
    let cfg = FactsConfig::default();
    (
        crate::compute_units(&list, &cfg),
        crate::compute(&list, &cfg),
    )
}

/// The reader's `new` of another unit's constructor: per unit proves `x`
/// only if the whole program does, and exactly when `instance`.
fn check_two(lib: &str, main: &str, instance: bool) {
    let (pu, pw) = two(lib, main);
    let (u, w) = (proven(&pu, "x"), proven(&pw, "x"));
    assert!(
        u <= w,
        "per unit proves {u} > whole {w}\n{}",
        pu.dump_text()
    );
    assert_eq!(u > 0, instance, "per unit: {u} proven\n{}", pu.dump_text());
}

const MAIN_F: &str = r#"
    import { F } from "./lib";
    const b: any = new (F as any)();
    console.log(b.x);
"#;

#[test]
fn another_units_function_constructor_returning_an_object_is_not_an_instance() {
    check_two(
        r#"
        export function F(this: any): any { this.x = 5; return { z: 1 }; }
        "#,
        MAIN_F,
        false,
    );
}

#[test]
fn another_units_function_constructor_returning_a_proxy_is_not_an_instance() {
    check_two(
        r#"
        export function F(this: any): any { this.x = 5; return new Proxy({}, {}); }
        "#,
        MAIN_F,
        false,
    );
}

#[test]
fn another_units_function_constructor_with_a_primitive_return_is_the_instance() {
    check_two(
        r#"
        export function F(this: any): any { this.x = 5; return 3; }
        "#,
        MAIN_F,
        true,
    );
}

#[test]
fn another_units_function_constructor_returning_this_stays_sound() {
    // Not asserted precise: `this` returned from a function leaves it.
    let (pu, pw) = two(
        r#"
        export function F(this: any): any { this.x = 5; return this; }
        "#,
        MAIN_F,
    );
    assert!(proven(&pu, "x") <= proven(&pw, "x"), "{}", pu.dump_text());
}

#[test]
fn another_units_function_constructor_returning_its_argument_is_not_an_instance() {
    // The value `new` yields is the caller's, not the function's instance.
    check_two(
        r#"
        export function F(this: any, o: any): any { this.x = 5; return o; }
        "#,
        MAIN_F,
        false,
    );
}

#[test]
fn another_units_class_whose_constructor_returns_an_object_is_not_an_instance() {
    check_two(
        r#"
        export class C { x = 5; constructor() { return { z: 1 } as any; } }
        "#,
        r#"
        import { C } from "./lib";
        const b: any = new C();
        console.log(b.x);
        "#,
        false,
    );
}

#[test]
fn reflect_construct_of_another_units_returning_constructor_is_not_an_instance() {
    check_two(
        r#"
        export function F(this: any, o: any): any { this.x = 5; return o; }
        export class C { x = 5; constructor(o: any) { return o; } }
        "#,
        r#"
        import { F, C } from "./lib";
        const a: any = Reflect.construct(F as any, [{ z: 1 }]);
        const b: any = Reflect.construct(C as any, [{ z: 1 }]);
        console.log(a.x, b.x);
        "#,
        false,
    );
}

#[test]
fn a_bound_returning_constructor_of_another_unit_is_not_an_instance() {
    check_two(
        r#"
        export function F(this: any, o: any): any { this.x = 5; return o; }
        "#,
        r#"
        import { F } from "./lib";
        const B: any = (F as any).bind(null, { z: 1 });
        const b: any = new B();
        console.log(b.x);
        "#,
        false,
    );
}

#[test]
fn a_class_expression_with_a_returning_constructor_of_another_unit_is_not_an_instance() {
    check_two(
        r#"
        export const K = class { x = 5; constructor(o: any) { return o; } };
        "#,
        r#"
        import { K } from "./lib";
        const b: any = new K({ z: 1 });
        console.log(b.x);
        "#,
        false,
    );
}

#[test]
fn a_subclass_whose_super_call_runs_another_units_returning_constructor_is_not_an_instance() {
    check_two(
        r#"
        export class C { constructor(o: any) { return o; } }
        export function F(this: any, o: any): any { return o; }
        "#,
        r#"
        import { C, F } from "./lib";
        class D extends C { x = 5; constructor(o: any) { super(o); } }
        class E extends (F as any) { x = 5; constructor(o: any) { super(o); } }
        const d: any = new D({ z: 1 });
        const e: any = new E({ z: 1 });
        console.log(d.x, e.x);
        "#,
        false,
    );
}
