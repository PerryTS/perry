//! A call the solver makes through a bound function (`new` of a bound
//! constructor, `Reflect.construct`, `Array.from` constructing its `this`)
//! is one call per (program call, bound value, kind): a bound target that
//! holds bound values again (a value rebound in a loop) joins that call
//! instead of nesting, so the solver-made calls are a finite set the
//! program fixes. The bound below is the test's, computed from the
//! program; the solver has none.
use super::*;

fn facts(src: &str) -> ProgramFacts {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let cfg = FactsConfig {
        max_work: Some(50_000_000),
        ..FactsConfig::default()
    };
    crate::compute(&[("t.ts".to_string(), &hir)], &cfg)
}

/// Every solver-made call is keyed by (program call, maker kind, a bound
/// value or a protocol step), so there are at most calls x (3 x binds +
/// 12 makers x 3 steps) of them; `(` counts the program's calls from above
/// and `.bind(` its bind sites.
fn bound(src: &str) -> u64 {
    let calls = src.matches('(').count() as u64;
    let binds = src.matches(".bind(").count() as u64;
    calls * (3 * binds + 36)
}

fn check(src: &str) {
    let pf = facts(src);
    assert!(!pf.stats.aborted, "the solve finishes: {:?}", pf.stats);
    let b = bound(src);
    assert!(
        pf.stats.solver_calls <= b,
        "solver-made calls {} > {b}: {:?}",
        pf.stats.solver_calls,
        pf.stats
    );
}

const MAT: &str = r#"
class Mat {
  e: number[];
  constructor(n?: number) { this.e = [n ?? 1, 0, 0, 1]; }
  multiply(m: Mat) { return this.multiplyMatrices(this, m); }
  premultiply(m: Mat) { return this.multiplyMatrices(m, this); }
  multiplyMatrices(a: Mat, b: Mat) {
    const x = a.e, y = b.e;
    this.e = [x[0] * y[0] + x[1] * y[2], x[0] * y[1] + x[1] * y[3], x[2] * y[0] + x[3] * y[2], x[2] * y[1] + x[3] * y[3]];
    return this;
  }
}
class Obj3 {
  matrix = new Mat();
  quaternion = new Mat();
  applyMatrix4(m: Mat) { this.matrix.premultiply(m); return this; }
  applyQuaternion(q: Mat) { this.quaternion.premultiply(q); return this; }
}
"#;

#[test]
fn new_of_a_rebound_constructor_joins_one_call() {
    check(&format!(
        "{MAT}{}",
        r#"
let C: any = Mat;
const o = new Obj3();
for (let i = 0; i < 8; i++) {
  C = C.bind(null);
  const m = new C(i);
  o.applyMatrix4(m).applyQuaternion(new C());
}
console.log(o.matrix.e[0]);
"#
    ));
}

#[test]
fn reflect_construct_and_array_from_of_a_rebound_constructor_join_one_call() {
    check(&format!(
        "{MAT}{}",
        r#"
let C: any = Mat;
const o = new Obj3();
for (let i = 0; i < 8; i++) {
  C = C.bind(null, i);
  const m = Reflect.construct(C, [i]);
  const a: any = Array.from.call(C, [m, new Mat()]);
  o.applyMatrix4(m).applyQuaternion(a[0] ?? m);
}
console.log(o.matrix.e[0]);
"#
    ));
}

/// A declared native whose callback is the native itself, on a primitive
/// receiver (ToObject: a wrapper born per call) whose wrapper's elements
/// hold it: each run's callback call runs the native on a new wrapper. Its
/// callback call is one per (program call, entry, effect) (`MK_NATIVE_CB`),
/// so the solve ends (before: a call per run, a recursion without end).
#[test]
fn native_callback_that_is_the_native_on_a_primitive() {
    check(
        r#"
const f: any = Array.prototype.forEach;
(Number.prototype as any)[0] = f;
(Number.prototype as any).length = 1;
(Object.prototype as any)[0] = f;
f.call(1, f, 2);
const g: any = Array.prototype.map;
(String.prototype as any).length = 1;
g.call(3, g, 4);
"#,
    );
}

/// The same recursion through a builtin method model's own callback call
/// (`array_method`'s `findIndex`, no declared entry): each run's callback
/// call ran `findIndex` on a new Number wrapper whose chain's element is
/// `findIndex` again. Its callback call is one per (program call, model
/// site, method) (`MK_BUILTIN_CB`), so the solve ends.
#[test]
fn builtin_method_callback_that_is_the_method_on_a_primitive() {
    check(
        r#"
const f: any = Array.prototype.findIndex;
(Number.prototype as any)[0] = f;
(Number.prototype as any).length = 1;
(Object.prototype as any)[0] = f;
f.call(1, f, 2);
"#,
    );
}

/// DECISIONS 225: the activation pass solves before the summaries
/// (`sum.ready` is false), so the wrapper of a primitive `this` is born per
/// call (`birth_ctx`). A builtin model whose callback call is new per run
/// then nests a new call and a new wrapper per level inside one `cons`
/// replay (r7 on the main merge). The pass has to finish, within the bound.
fn act_check(src: &str) {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let cfg = FactsConfig {
        roots: vec!["t.ts".into()],
        max_work: Some(50_000_000),
        ..FactsConfig::default()
    };
    let a = crate::activation(&[("t.ts".into(), &hir)], &cfg);
    assert!(a.live_dynamic_imports().is_some(), "the activation pass finishes");
}

#[test]
fn activation_builtin_method_callback_that_is_the_method_on_a_primitive() {
    act_check(
        r#"
const f: any = Array.prototype.findIndex;
(Number.prototype as any)[0] = f;
(Number.prototype as any).length = 1;
(Object.prototype as any)[0] = f;
f.call(1, f, 2);
"#,
    );
}

/// The r7 shape: `f.apply(x, arguments)` forwards a call whose positions
/// hold the method itself (`Model::positional` reads the spread elements).
#[test]
fn activation_forwarded_arguments_into_a_builtin_method_on_a_primitive() {
    act_check(
        r#"
const f: any = Array.prototype.findIndex;
function g(this: any) { return f.apply(7, arguments); }
(Number.prototype as any)[0] = f;
(Number.prototype as any)[1] = g;
(Number.prototype as any).length = 2;
g(g, 2);
"#,
    );
}
