//! The engine's iteration protocol calls are exact: IteratorStep is
//! `Call(next, iterator, «»)` (ECMA-262 IteratorNext with no value), so a
//! generator stepped by for-of or array destructuring resumes with
//! `undefined`, and a program-defined `next(v)` reached that way gets `v` =
//! `undefined` by arity (perry-runtime `js_iterator_step`: the intrinsic step
//! resumes with `undefined`, any other `next` is called with argc 0).
//! Controls: an explicit `next(x)` with `x` from unknown code, a `next`
//! stored into an unknown value, and `yield*` forwarding the received value
//! to the inner `next(v)`.
use super::*;

fn transformed(src: &str) -> ProgramFacts {
    let ast = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let mut hir = perry_hir::lower_module(&ast, "t", "t.ts").expect("lowers");
    perry_transform::transform_generators(&mut hir);
    compute(&[("t.ts".to_string(), &hir)], &FactsConfig::default())
}

const UNDEF: u16 = 1 << 4;

fn value_kind(pf: &ProgramFacts, class: &str) -> (bool, u16) {
    let k = &field(tmpl(pf, class), "value").kind;
    (k.top, k.mask())
}

fn undefined_only(pf: &ProgramFacts, class: &str) {
    let (top, mask) = value_kind(pf, class);
    assert!(!top && mask & UNDEF != 0, "{class}: top={top} mask={mask:#x}\n{}", pf.dump_text());
}

fn stays_top(pf: &ProgramFacts, class: &str) {
    let (top, mask) = value_kind(pf, class);
    assert!(top, "{class}: top={top} mask={mask:#x}\n{}", pf.dump_text());
}

const RESUMED: &str = "class Resumed { value: any; constructor(v: any) { this.value = v; } }\n";

/// S2: a generator stepped by for-of resumes with `undefined` only.
#[test]
fn for_of_next_resumes_a_generator_with_undefined() {
    let pf = transformed(&format!(
        r#"{RESUMED}
        function* g() {{ const r = yield 1; new Resumed(r); }}
        function run(it: any) {{ let n = 0; for (const x of it) {{ n += x; }} return n; }}
        console.log(run(g()));
        "#
    ));
    undefined_only(&pf, "Resumed");
}

/// S1: a program-defined `next(v)` reached through for-of gets no argument.
#[test]
fn for_of_next_calls_a_program_next_with_no_argument() {
    let pf = transformed(&format!(
        r#"{RESUMED}
        const fns = {{ next: function (v: any) {{ new Resumed(v); return {{ done: true, value: 0 }}; }} }};
        const it = {{ [Symbol.iterator]() {{ return fns; }} }};
        function run(o: any) {{ for (const x of o) {{ console.log(x); }} }}
        run(it);
        "#
    ));
    undefined_only(&pf, "Resumed");
}

/// Array destructuring steps the same way.
#[test]
fn for_of_next_destructuring_resumes_with_undefined() {
    let pf = transformed(&format!(
        r#"{RESUMED}
        function* g() {{ const r = yield 1; new Resumed(r); yield 2; }}
        const [a, b] = g();
        console.log(a, b);
        "#
    ));
    undefined_only(&pf, "Resumed");
}

/// Spread steps the same way.
#[test]
fn for_of_next_spread_resumes_with_undefined() {
    let pf = transformed(&format!(
        r#"{RESUMED}
        function* g() {{ const r = yield 1; new Resumed(r); yield 2; }}
        const all = [...g()];
        console.log(all.length);
        "#
    ));
    undefined_only(&pf, "Resumed");
}

/// Control: an explicit `next(x)` with `x` from unknown code keeps `x`.
#[test]
fn for_of_next_control_explicit_unknown_argument_stays_top() {
    let pf = transformed(&format!(
        r#"{RESUMED}
        function* g() {{ const r = yield 1; new Resumed(r); }}
        const unknown = new Function(process.argv[2])();
        const it = g(); it.next(); it.next(unknown);
        for (const x of g()) {{ console.log(x); }}
        "#
    ));
    stays_top(&pf, "Resumed");
}

/// Control: a `next` stored into an unknown value may be called with anything.
#[test]
fn for_of_next_control_next_stored_into_unknown_stays_top() {
    let pf = transformed(&format!(
        r#"{RESUMED}
        const fns = {{ next: function (v: any) {{ new Resumed(v); return {{ done: true, value: 0 }}; }} }};
        const it = {{ [Symbol.iterator]() {{ return fns; }} }};
        const sink = new Function(process.argv[2])();
        sink.f = fns.next;
        function run(o: any) {{ for (const x of o) {{ console.log(x); }} }}
        run(it);
        "#
    ));
    stays_top(&pf, "Resumed");
}

/// Control: `yield*` forwards the value its own `next(v)` received to the
/// inner `next(v)`: a string resume reaches the inner generator, and the
/// for-of step adds `undefined`.
#[test]
fn for_of_next_control_yield_star_forwards_the_resume_value() {
    let pf = transformed(&format!(
        r#"{RESUMED}
        function* inner() {{ const r = yield 1; new Resumed(r); }}
        function* outer() {{ yield* inner(); }}
        const o = outer(); o.next(); o.next("forwarded");
        for (const x of outer()) {{ console.log(x); }}
        "#
    ));
    let (top, mask) = value_kind(&pf, "Resumed");
    assert!(!top && mask & 2 != 0 && mask & UNDEF != 0, "top={top} mask={mask:#x}\n{}", pf.dump_text());
}

/// Control: a `next` passed to unknown code may be called with anything.
#[test]
fn for_of_next_control_next_passed_to_unknown_stays_top() {
    let pf = transformed(&format!(
        r#"{RESUMED}
        const fns = {{ next: function (v: any) {{ new Resumed(v); return {{ done: true, value: 0 }}; }} }};
        const it = {{ [Symbol.iterator]() {{ return fns; }} }};
        const unknown = new Function(process.argv[2]);
        unknown(fns.next);
        function run(o: any) {{ for (const x of o) {{ console.log(x); }} }}
        run(it);
        "#
    ));
    stays_top(&pf, "Resumed");
}

/// S1, the iterable its own iterator: its `next(v)` gets no argument.
#[test]
fn for_of_next_calls_a_self_iterator_next_with_no_argument() {
    let pf = transformed(&format!(
        r#"{RESUMED}
        const it = {{
            [Symbol.iterator]() {{ return this; }},
            next(v: any) {{ new Resumed(v); return {{ done: true, value: 0 }}; }}
        }};
        function run(o: any) {{ for (const x of o) {{ console.log(x); }} }}
        run(it);
        "#
    ));
    undefined_only(&pf, "Resumed");
}
