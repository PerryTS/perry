//! A class object is a function value: `Function.prototype.bind` on it makes
//! a bound function whose `new` (or `Reflect.construct`) runs the class
//! constructor on a fresh instance of the class (SOUND7 C: the bound
//! constructor body was excluded while Node ran it).
use super::*;

fn compute_src(src: &str) -> (perry_hir::Module, ProgramFacts) {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let cfg = FactsConfig {
        max_work: Some(50_000_000),
        ..FactsConfig::default()
    };
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], &cfg);
    (hir, pf)
}

fn check(src: &str) {
    let (hir, pf) = compute_src(src);
    assert!(pf.body_reach.complete, "activated");
    let id = hir
        .functions
        .iter()
        .find(|f| f.name == "hit")
        .expect("hit")
        .id;
    assert!(
        pf.body_reach.bodies.iter().any(|(_, i, _)| *i == id),
        "the bound class constructor runs\n{}",
        pf.dump_text()
    );
    let sites: Vec<_> = pf.sites.iter().filter(|s| s.key == "s").collect();
    assert!(!sites.is_empty(), "{}", pf.dump_text());
    for s in sites {
        let got: Vec<_> = s
            .receivers
            .iter()
            .map(|r| r.rsplit(':').next().unwrap())
            .collect();
        assert!(!s.top && got == ["P"], "{s:?}\n{}", pf.dump_text());
    }
}

#[test]
fn new_of_a_bound_class_constructs_the_class() {
    check(
        r#"
function hit() { return 1; }
class P { s: number; constructor(a: number, b: number) { hit(); this.s = a + b; } }
const B: any = P.bind(null, 5);
const p = new B(6);
console.log(p.s);
"#,
    );
}

#[test]
fn reflect_construct_of_a_class_bound_through_a_parameter_constructs_the_class() {
    check(
        r#"
function hit() { return 1; }
class P { s: number; constructor(a: number, b: number) { hit(); this.s = a + b; } }
function mk(k: any) { return k.bind(null, 5); }
const p: any = Reflect.construct(mk(P), [6]);
console.log(p.s);
"#,
    );
}

/// Control: a bound plain function was already constructed (passes on the base).
#[test]
fn new_of_a_bound_function_constructs_it() {
    let (hir, pf) = compute_src(
        r#"
function hit() { return 1; }
function P(this: any, a: number, b: number) { hit(); this.s = a + b; }
const B: any = (P as any).bind(null, 5);
const p = new B(6);
console.log(p.s);
"#,
    );
    let id = hir
        .functions
        .iter()
        .find(|f| f.name == "hit")
        .expect("hit")
        .id;
    assert!(
        pf.body_reach.bodies.iter().any(|(_, i, _)| *i == id),
        "{}",
        pf.dump_text()
    );
}
