//! E11ROOT1: a `null` among the values a built-in parent name evaluates to
//! (`class HttpError extends Error` after the program wrote `Error = null`)
//! makes no instance: the derived constructor's `super()` finds
//! %Function.prototype% as the class's [[Prototype]] (ClassDefinitionEvaluation,
//! `extends null`), which is no constructor, and throws a realm TypeError
//! before `this` is bound (ECMA-262 13.3.7.1 SuperCall, 10.2.2 [[Construct]]).
//! Nothing reaches unknown code; the instance made through the other values
//! (the realm's `Error`) is the only instance. Red on base: the null escaped
//! the instance (c09 HttpError / DOMException, "E11 dynamic class parent").
//! The c09 DOMException form (`extends globalThis.Error`) is not here: in the
//! default configuration that read yields the `Error` namespace token, not
//! the declared constructor, and is unproven without any null (E11B).
use super::*;

fn site(pf: &ProgramFacts, src: &str) -> (bool, String) {
    let m = src.find("// SITE").expect("marker");
    let start = src[..m].rfind('\n').map_or(0, |i| i + 1) as u32;
    let s: Vec<_> = (pf.sites.iter())
        .filter(|s| !s.is_call && s.key == "x" && (start..m as u32).contains(&s.byte_offset))
        .collect();
    assert!(!s.is_empty(), "no x site");
    (
        s.iter().all(|s| !matches!(s.verdict, Verdict::No(_))),
        format!("{:?}", s.iter().map(|s| (&s.receivers, &s.verdict)).collect::<Vec<_>>()),
    )
}

fn program(before: &str, parent: &str) -> String {
    format!(
        "{before}\n\
         class HttpError extends {parent} {{ r: any; constructor(m: string, r: any) {{ super(m); this.r = r; }} }}\n\
         class V {{ x: number; constructor(x: number) {{ this.x = x; }} add(o: V): V {{ return new V(this.x + o.x); }} }}\n\
         const a = new V(1), b = new V(2);\n\
         function f(m: string): any {{ try {{ throw new HttpError(m, a); }} catch (e: any) {{ return e.message; }} }}\n\
         console.log(a.add(b).x, f(\"q\")); // SITE\n"
    )
}

/// Expected RED on base: the program's `Error = null` escaped the instance.
#[test]
fn e11_null_global_error_makes_no_instance() {
    let src = program("(globalThis as any).Error = null;", "Error");
    let (ok, v) = site(&facts(&src), &src);
    assert!(ok, "{v}");
}

/// Expected RED on base: null only under a store the solve cannot name
/// (an any-key program store onto the global object, R-GLOBAL).
#[test]
fn e11_null_any_key_global_store_makes_no_instance() {
    let src = program(
        "const kk = String(process.argv.length); (globalThis as any)[kk] = null;",
        "Error",
    );
    let (ok, v) = site(&facts(&src), &src);
    assert!(ok, "{v}");
}

/// Control (green on base and fix): another unit may write the global (⊤).
#[test]
fn e11_null_control_unseen_stays_unproven() {
    let src = unseen(&program("(globalThis as any).Error = null;", "Error"));
    let (ok, v) = site(&facts(&src), &src);
    assert!(!ok, "{v}");
}

/// Whether `hit` can run in `src` (as in `static_native_parent.rs`).
fn hit_runs(src: &str) -> bool {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let cfg = FactsConfig {
        max_work: Some(50_000_000),
        ..FactsConfig::default()
    };
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], &cfg);
    assert!(pf.body_reach.complete, "activated");
    let id = (hir.functions.iter())
        .find(|f| f.name == "hit")
        .expect("hit")
        .id;
    pf.body_reach.bodies.iter().any(|(_, i, _)| *i == id)
}

/// Under the null the class object's [[Prototype]] is %Function.prototype%
/// (ClassDefinitionEvaluation step 8.b): a static call on the class finds
/// what the program stored there. `parent` = how the null reaches the
/// `extends` of a class linked by name to `Error`.
fn null_static(before: &str, parent: &str) -> String {
    format!(
        "function hit() {{ return 5; }}\n\
         {before}\n\
         class X extends {parent} {{}}\n\
         (Function.prototype as any).g = function () {{ return hit(); }};\n\
         console.log((X as any).g());\n"
    )
}

#[test]
fn e11_null_parent_class_object_reads_function_prototype() {
    let src = null_static("(globalThis as any).Error = null;", "Error");
    assert!(hit_runs(&src));
}

/// The null alone (a local binding of the name; no realm `Error` beside it).
#[test]
fn e11_null_local_parent_class_object_reads_function_prototype() {
    let src = null_static("const Error: any = null;", "Error");
    assert!(hit_runs(&src));
}

/// Control: the store on %Function.prototype% reaches a class without a
/// parent (the test can see the call).
#[test]
fn e11_null_control_plain_class_reads_function_prototype() {
    let src = null_static("", "Object").replace("class X extends Object {}", "class X {}");
    assert!(hit_runs(&src));
}

/// `super()` on the null makes no instance: `a` reaches no one, also when
/// the null is the only value (a local binding of the name).
#[test]
fn e11_null_local_parent_makes_no_instance() {
    let src = program("const Error: any = null;", "Error");
    let (ok, v) = site(&facts(&src), &src);
    assert!(ok, "{v}");
}
