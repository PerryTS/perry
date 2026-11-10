//! DECISIONS 225: each activation path is the only reason some body runs.
//! A program whose marker function `hit` runs only through the body under
//! test: `hit` is active, and with the paths that carry that body left out
//! (`PERRY_FACTS_SABOTAGE=activation:<path>[+<path>..]`) it is not, so a
//! claim that dropped those paths would exclude a body Node runs. Two paths
//! carry one transfer where the solver activates twice: a plain call into a
//! summarized callee (`call` and its `this` binding), a getter under a
//! constant computed key (`computed` at registration and `accessor` at the
//! read). Leaving out either alone keeps the body active (a control).
//! A private method called from the constructor of a class a mixin extends
//! is the first kind: its call selects the declaring member (CXCHK0 found
//!  excluded while Node ran it, before private calls were followed).
//! An iterator's `return` runs through the consumer's IteratorClose, a
//! protocol call on the value GetIterator produced: what
//! `v[Symbol.iterator]()` returned, not only `v` (CXCHK0 gap witnesses
//! test_gap_10524_untyped_array_destructuring and the iterator-close
//! family).
//! A `new` of a generic class without a constructor of its own names its
//! specialization (`S$num`), while the program constructs the class as
//! written when its heritage is live (codegen `super_constructor::new`), so
//! the written class's default constructor and field initializers run
//! (CXCHK0 witnesses test_gap_6232_class_extends_array, the Map/Promise
//! subclass family, test_gap_sp_live_super_defaults).
//! A static accessor is found on the class object (and through it on a
//! subclass), where the program reads it (CXCHK0 gap witnesses
//! test_gap_class_name_length_own, test_gap_class_accessor_shape_facts and
//! the static-accessor family).
//! `instance`, `composed` and `for-in` fire only inside a transfer that
//! `call`, `accessor`, `this` or `for-in`'s caller already activates (an
//! instance is made by a call binding; a composed callee's calls are
//! re-solved as calls of the instance), so no program reaches a body
//! through them alone.

use super::*;

/// Whether `hit` can run in `src` with the activation paths `sab` left out.
fn hit_runs(src: &str, sab: Option<&str>) -> bool {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let cfg = FactsConfig {
        sabotage: sab.map(|s| format!("activation:{s}")),
        max_work: Some(50_000_000),
        ..FactsConfig::default()
    };
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], &cfg);
    assert!(pf.body_reach.complete, "activated");
    let on = &pf.body_reach.bodies;
    let id = hir
        .functions
        .iter()
        .find(|f| f.name == "hit")
        .expect("hit")
        .id;
    on.iter().any(|(_, i, _)| *i == id)
}

/// Each fixture, the path set that alone excludes it, and the single paths
/// (controls) that leave it active.
const FIXTURES: &[(&str, &str, &str, &[&str])] = &[
    (
        "summarized-callee",
        r#"
function hit() { return 1; }
function m(x: any): any { if (x > 100) return m(x - 1); hit(); return x; }
function outer(y: any) { return m(y); }
console.log(outer(1), outer("s"));
"#,
        "call+this",
        &["call", "this", "instance", "composed"],
    ),
    (
        "computed-getter",
        r#"
function hit() { return 1; }
const k = "dyn";
class C { get [k]() { return hit() + 6; } }
console.log((new C() as any).dyn);
"#,
        "computed+accessor",
        &["computed", "accessor", "call"],
    ),
    (
        "private-method-of-a-mixin-base",
        r#"
function hit() { return 1; }
class Client {
  opts: any;
  constructor(o: any) { this.opts = this.#init(o); }
  #init(o: any) { return { ...o, ok: hit() > 0 }; }
}
function attach(Base: any) { return class extends Base {}; }
const Sub = attach(Client);
console.log(new Sub({ a: 1 }).opts.ok);
console.log(new Client({}).opts.ok);
"#,
        "call+this",
        &["call", "this", "escape", "accessor"],
    ),
    (
        "iterator-close-of-a-returned-iterator-destructuring",
        r#"
function hit() { return 1; }
const src: any = { [Symbol.iterator]() { return { next() { return { done: false, value: 1 }; }, return() { hit(); return { done: true }; } }; } };
const [a] = src;
console.log(a);
"#,
        "call+this",
        &["escape", "accessor", "computed"],
    ),
    (
        "iterator-close-of-a-returned-iterator-for-of-break",
        r#"
function hit() { return 1; }
function mk(): any { return { next() { return { done: false, value: 1 }; }, return() { hit(); return { done: true }; } }; }
const src: any = { [Symbol.iterator]() { return mk(); } };
for (const v of src) { console.log(v); break; }
"#,
        "call+this",
        &["escape", "accessor", "computed"],
    ),
    (
        "static-getter-of-a-class",
        r#"
function hit() { return 1; }
class G { static get x() { return hit(); } }
console.log(G.x);
"#,
        "accessor",
        &["call", "this", "escape", "computed"],
    ),
    (
        "inherited-static-setter",
        r#"
function hit() { return 1; }
class S { static set c(v: any) { hit(); } }
class T extends S {}
T.c = 1;
console.log("set");
"#,
        "accessor",
        &["call", "this", "escape", "computed"],
    ),
];

/// A static accessor is an own property of the class object, not of its
/// prototype: an instance read under its name does not run it (Node prints
/// `undefined`).
#[test]
fn an_instance_read_does_not_run_a_static_getter() {
    let src = r#"
function hit() { return 1; }
class G { static get x() { return hit(); } }
console.log((new G() as any).x);
"#;
    assert!(!hit_runs(src, None), "hit active through an instance read");
}

#[test]
fn each_fixture_runs_its_body() {
    for (name, src, _, _) in FIXTURES {
        assert!(hit_runs(src, None), "{name}: hit inactive without sabotage");
    }
}

#[test]
fn leaving_out_the_carrying_paths_excludes_the_body() {
    for (name, src, set, _) in FIXTURES {
        assert!(
            !hit_runs(src, Some(set)),
            "{name}: hit active with {set} left out"
        );
    }
}

#[test]
fn leaving_out_one_carrier_keeps_the_body() {
    for (name, src, _, controls) in FIXTURES {
        for p in *controls {
            assert!(
                hit_runs(src, Some(p)),
                "{name}: hit inactive with only {p} left out"
            );
        }
    }
}

/// The active bodies of `src` after the compiler's specialization pass, with
/// the activation paths `sab` left out.
fn active_bodies(src: &str, sab: Option<&str>) -> Vec<String> {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let mut hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    perry_hir::monomorphize_module(&mut hir);
    let cfg = FactsConfig {
        sabotage: sab.map(|s| format!("activation:{s}")),
        max_work: Some(50_000_000),
        ..FactsConfig::default()
    };
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], &cfg);
    assert!(pf.body_reach.complete, "activated");
    pf.body_reach
        .bodies
        .iter()
        .map(|(_, _, n)| n.clone())
        .collect()
}

fn runs(bodies: &[String], name: &str) -> bool {
    bodies
        .iter()
        .any(|b| b == name || b.ends_with(&format!(":{name}")))
}

/// Generic subclasses without a constructor of their own: the program
/// constructs the class as written (a native parent, and a user parent
/// whose constructor link a store replaced), so its field initializers run.
const WRITTEN_CLASS_NEW: &[(&str, &str)] = &[
    (
        "generic-array-subclass",
        r#"
class S<T> extends Array<T> { tag = 1; }
const s = new S<number>();
s.push(1);
console.log(s.length, (s as any).tag);
"#,
    ),
    (
        "generic-map-subclass",
        r#"
class S<K, V> extends Map<K, V> { hits = 0; }
const s = new S<string, number>();
s.set("a", 1);
console.log(s.size, s.hits);
"#,
    ),
    (
        "generic-subclass-of-a-replaced-parent",
        r#"
class Fixed { x: any; constructor(x: any) { this.x = x; } }
class Rest { n: any; constructor(...a: any[]) { this.n = a.length; } }
class S<T> extends Fixed { marker = 9; }
Object.setPrototypeOf(S, Rest);
const s: any = new S<number>(1, 2, 3);
console.log(s.marker, s.n);
"#,
    ),
];

#[test]
fn a_generic_subclass_runs_the_written_class_fields() {
    for (name, src) in WRITTEN_CLASS_NEW {
        let on = active_bodies(src, None);
        assert!(
            runs(&on, "S::<fields>"),
            "{name}: S::<fields> inactive: {on:?}"
        );
    }
}

/// The same class constructed without type arguments (no specialization):
/// the facts were right before and stay so.
#[test]
fn a_non_generic_new_runs_the_class_fields() {
    let src = r#"
class S<T> extends Array<T> { tag = 1; }
const s = new S();
console.log(s.length, (s as any).tag);
"#;
    assert!(runs(&active_bodies(src, None), "S::<fields>"));
}

/// Control: with a constructor of its own the specialization is constructed
/// as itself (codegen inlines `new S$num`), so the written class's field
/// initializers stay inactive.
#[test]
fn a_generic_class_with_its_own_constructor_runs_only_the_specialization() {
    let src = r#"
class B { b = 1; }
class S<T> extends B { tag = 1; constructor() { super(); } }
const s = new S<number>();
console.log(s.tag, s.b);
"#;
    let on = active_bodies(src, None);
    assert!(!runs(&on, "S::<fields>"), "S::<fields> active: {on:?}");
    assert!(
        on.iter()
            .any(|b| b.contains("S$") && b.ends_with("::<fields>")),
        "{on:?}"
    );
}

/// The written class's field initializers are carried by the construction
/// (`call` and its `this` binding): leaving those out excludes them.
#[test]
fn leaving_out_the_construction_excludes_the_written_class_fields() {
    for (name, src) in WRITTEN_CLASS_NEW {
        let on = active_bodies(src, Some("call+this"));
        assert!(
            !runs(&on, "S::<fields>"),
            "{name}: S::<fields> active with call+this left out"
        );
    }
}

#[test]
fn owned_async_module_body_is_active_only_when_its_module_runs() {
    let module = perry_parser::parse_typescript(
        "async function body() { await import('./target.ts'); }",
        "t.ts",
    )
    .expect("parses");
    let mut hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let id = hir.functions[0].id;
    // Collection normally resolves the literal before activation. This unit
    // fixture starts at lowered HIR, so supply that existing execution edge.
    let perry_hir::Stmt::Expr(perry_hir::Expr::Await(value)) = &mut hir.functions[0].body[0] else {
        panic!("await expression");
    };
    let perry_hir::Expr::DynamicImport { paths, .. } = value.as_mut() else {
        panic!("dynamic import");
    };
    paths.push("./target.ts".into());
    let cfg = FactsConfig {
        roots: vec!["t.ts".into()],
        ..FactsConfig::default()
    };
    let inactive = crate::activation(&[("t.ts".into(), &hir)], &cfg);
    assert!(inactive
        .live_dynamic_imports()
        .expect("complete")
        .is_empty());
    hir.async_init_function = Some(id);
    let active = crate::activation(&[("t.ts".into(), &hir)], &cfg);
    assert!(active
        .live_dynamic_imports()
        .expect("complete")
        .contains(&("t.ts".into(), "./target.ts".into())));
}
