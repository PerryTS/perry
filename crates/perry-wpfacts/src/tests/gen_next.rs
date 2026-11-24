//! for-of over a generator object reads `next` ONCE through the iterator
//! object's chain (ECMA-262 7.4.2 GetIteratorFromMethod, 7.4.3
//! GetIterator) and calls that function at every step (7.4.4
//! IteratorNext). A generator object inherits `next` from its function's
//! `prototype` object and then %GeneratorPrototype% (27.5.1); an own
//! `next`, a `next` on the function's `prototype` or an own
//! `[Symbol.iterator]` is what for-of calls instead. (U) fixtures write one
//! of them; (P) fixtures write none. Unknown values come from
//! `process.argv`.

use super::*;

fn facts_of(src: &str) -> ProgramFacts {
    facts_and_sources(src).0
}

/// The facts, and the source local of every for-of record in the
/// transformed program (`arrayRecordNeedsIterator(LocalSet(source, ..))`,
/// what codegen's record site names `source`), in program order.
fn facts_and_sources(src: &str) -> (ProgramFacts, Vec<u32>) {
    let src = format!(
        r#"
        declare function external(x?: unknown): any;
        class Svc {{ size = 3; }}
        class Alt {{ other = 1; size = 4; }}
        class Called {{ value: any; constructor(v: any) {{ this.value = v; }} }}
        function g(a: any) {{ return a.size; }}
        function* gen() {{ yield new Svc(); }}
        {src}
        "#
    );
    let ast = perry_parser::parse_typescript(&src, "t.ts").expect("parses");
    let mut hir = perry_hir::lower_module(&ast, "t", "t.ts").expect("lowers");
    perry_transform::transform_generators(&mut hir);
    let text = format!("{hir:?}");
    let mut sources = Vec::new();
    for (at, _) in text.match_indices("\"arrayRecordNeedsIterator\"") {
        // Codegen's record site: the first argument binds the source.
        let head = "\"arrayRecordNeedsIterator\", args: [LocalSet(";
        let Some(rest) = text[at..].strip_prefix(head) else {
            continue;
        };
        let n: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        sources.push(n.parse().expect("local id"));
    }
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    (compute(&[("t.ts".to_string(), &hir)], &cfg), sources)
}

/// `generator_source` of each for-of record source local.
fn generator_sources(src: &str) -> Vec<bool> {
    let (pf, sources) = facts_and_sources(src);
    assert!(!sources.is_empty(), "no for-of record");
    let mb = pf.bindings_for("t.ts").expect("module bindings");
    sources
        .iter()
        .map(|id| {
            mb.locals
                .iter()
                .find(|(l, _)| l == id)
                .is_some_and(|(_, b)| b.generator_source)
        })
        .collect()
}

/// No for-of record source of `src` is a generator source.
#[track_caller]
fn excluded(src: &str) {
    let g = generator_sources(src);
    assert!(g.iter().all(|&b| !b), "{g:?}");
}

/// Every for-of record source of `src` is a generator source.
#[track_caller]
fn holds(src: &str) {
    let g = generator_sources(src);
    assert!(g.iter().all(|&b| b), "{g:?}");
}

fn proven(v: &Verdict) -> bool {
    !matches!(v, Verdict::No(_))
}

/// The `size` site's receivers and verdict.
fn at_site(pf: &ProgramFacts) -> (Vec<String>, Verdict) {
    let s = site(pf, "size");
    (s.receivers.clone(), s.verdict.clone())
}

/// `Called.value` holds a string: the user function ran.
fn called(pf: &ProgramFacts) -> bool {
    let k = &field(tmpl(pf, "Called"), "value").kind;
    k.top || k.mask() & (1 << 1) != 0
}

const USER_NEXT: &str =
    "function () { new Called(String(Math.random())); return { value: new Alt(), done: false }; }";

/// U1. node: prints `4`. An own `next` on the generator object is what
/// GetIterator reads: the loop's `v` is what it returns, and it is called.
#[test]
fn gen_next_u1_own_next_on_the_generator_object() {
    let pf = facts_of(&format!(
        "const it: any = gen();
         it.next = {USER_NEXT};
         for (const s of it) {{ console.log(g(s)); break; }}"
    ));
    let (r, v) = at_site(&pf);
    assert!(r.iter().any(|r| r.contains("Alt")), "{r:?} {v:?}");
    assert!(
        called(&pf),
        "user next not seen as called\n{}",
        pf.dump_text()
    );
}

/// U1b. The own `next` stored through an unknown key.
#[test]
fn gen_next_u1b_own_next_under_an_unknown_key() {
    let pf = facts_of(&format!(
        "const it: any = gen();
         it[process.argv[2] ?? \"next\"] = {USER_NEXT};
         for (const s of it) {{ console.log(g(s)); break; }}"
    ));
    let (r, v) = at_site(&pf);
    assert!(r.iter().any(|r| r.contains("Alt")), "{r:?} {v:?}");
    assert!(
        called(&pf),
        "user next not seen as called\n{}",
        pf.dump_text()
    );
}

/// U2. node: prints `4`. `G.prototype.next`.
#[test]
fn gen_next_u2_next_on_the_function_prototype() {
    let pf = facts_of(&format!(
        "(gen as any).prototype.next = {USER_NEXT};
         for (const s of gen()) {{ console.log(g(s)); break; }}"
    ));
    let (r, v) = at_site(&pf);
    assert!(r.iter().any(|r| r.contains("Alt")), "{r:?} {v:?}");
    assert!(
        called(&pf),
        "user next not seen as called\n{}",
        pf.dump_text()
    );
}

/// U3. %GeneratorPrototype%.next reached through
/// `Object.getPrototypeOf(G.prototype)`. DEC 250: perry's runtime refuses
/// a write of an image member of %GeneratorPrototype% (`next`), so the
/// intrinsic stays: the yielded `Svc` alone reaches the site.
#[test]
fn gen_next_u3_next_on_the_generator_prototype_is_locked() {
    let pf = facts_of(&format!(
        "Object.getPrototypeOf((gen as any).prototype).next = {USER_NEXT};
         for (const s of gen()) {{ console.log(g(s)); break; }}"
    ));
    let (r, v) = at_site(&pf);
    assert!(r.iter().any(|r| r.contains("Svc")), "{r:?} {v:?}");
}

/// U4. node: prints `4`. An own `[Symbol.iterator]` on the generator
/// object returning another iterator.
#[test]
fn gen_next_u4_own_symbol_iterator() {
    let pf = facts_of(&format!(
        "const it: any = gen();
         it[Symbol.iterator] = function () {{ return {{ next: {USER_NEXT} }}; }};
         for (const s of it) {{ console.log(g(s)); break; }}"
    ));
    let (r, v) = at_site(&pf);
    assert!(r.iter().any(|r| r.contains("Alt")), "{r:?} {v:?}");
    assert!(
        called(&pf),
        "user next not seen as called\n{}",
        pf.dump_text()
    );
}

/// U5. The iterable is a generator object or an array.
#[test]
fn gen_next_u5_generator_or_array() {
    let pf = facts_of(
        "const src: any = process.argv.length > 5 ? gen() : [new Alt()];
         for (const s of src) { console.log(g(s)); break; }",
    );
    let (r, v) = at_site(&pf);
    assert!(r.iter().any(|r| r.contains("Alt")), "{r:?} {v:?}");
    assert!(r.iter().any(|r| r.contains("Svc")), "{r:?} {v:?}");
}

/// P1. node: prints `3`. Nothing written.
#[test]
fn gen_next_p1_plain_generator() {
    let pf = facts_of("for (const s of gen()) { console.log(g(s)); }");
    let (r, v) = at_site(&pf);
    assert!(proven(&v), "{r:?} {v:?}");
    assert!(!r.iter().any(|r| r.contains("Alt")), "{r:?}");
}

/// P2. node: prints `3`. A generator consumed by another generator's
/// for-of, which a for-of consumes.
#[test]
fn gen_next_p2_nested_generators() {
    let pf = facts_of(
        "function* outer() { for (const s of gen()) yield s; }
         for (const s of outer()) { console.log(g(s)); }",
    );
    let (r, v) = at_site(&pf);
    assert!(proven(&v), "{r:?} {v:?}");
}

/// P3. node: prints `3`. The generator object stored in a field and
/// passed through a call.
#[test]
fn gen_next_p3_stored_and_passed() {
    let pf = facts_of(
        "class Box { it: any; constructor(i: any) { this.it = i; } }
         const b = new Box(gen());
         function run(x: any) { for (const s of x) { console.log(g(s)); } }
         run(b.it);",
    );
    let (r, v) = at_site(&pf);
    assert!(proven(&v), "{r:?} {v:?}");
}

// The fact (`gen_sources`): P1-P4 hold, U1-U7 are excluded.

#[test]
fn gen_next_fact_u1_own_next() {
    excluded(&format!("const it: any = gen(); it.next = {USER_NEXT}; for (const s of it) {{ console.log(g(s)); break; }}"));
}

#[test]
fn gen_next_fact_u1b_unknown_key() {
    excluded(&format!("const it: any = gen(); it[process.argv[2] ?? \"next\"] = {USER_NEXT}; for (const s of it) {{ console.log(g(s)); break; }}"));
}

#[test]
fn gen_next_fact_u1c_own_return() {
    excluded("const it: any = gen(); it.return = function () { new Called(String(Math.random())); return { value: 0, done: true }; }; for (const s of it) { console.log(g(s)); break; }");
}

#[test]
fn gen_next_fact_u2_proto_next() {
    excluded(&format!("(gen as any).prototype.next = {USER_NEXT}; for (const s of gen()) {{ console.log(g(s)); break; }}"));
}

#[test]
fn gen_next_fact_u2b_replaced_prototype() {
    excluded(&format!("(gen as any).prototype = {{ next: {USER_NEXT} }}; for (const s of gen()) {{ console.log(g(s)); break; }}"));
}

#[test]
fn gen_next_fact_u2c_set_prototype_of() {
    excluded(&format!("Object.setPrototypeOf((gen as any).prototype, {{ next: {USER_NEXT} }}); for (const s of gen()) {{ console.log(g(s)); break; }}"));
}

/// `%IteratorPrototype%[@@iterator]` is not a locked image member of
/// `%GeneratorPrototype%`: a write reached through the chain excludes.
#[test]
fn gen_next_fact_u3_iterator_prototype_symbol_iterator() {
    excluded(&format!("Object.getPrototypeOf(Object.getPrototypeOf((gen as any).prototype))[Symbol.iterator] = function () {{ return {{ next: {USER_NEXT} }}; }}; for (const s of gen()) {{ console.log(g(s)); break; }}"));
}

#[test]
fn gen_next_fact_u4_own_symbol_iterator() {
    excluded(&format!("const it: any = gen(); it[Symbol.iterator] = function () {{ return {{ next: {USER_NEXT} }}; }}; for (const s of it) {{ console.log(g(s)); break; }}"));
}

/// The guarded for-of walks the ordinary Array by index; its iterator arm
/// runs only when `arrayRecordNeedsIterator(t)` holds (rule RP2), so the
/// record there is over the generator alone: a generator source.
#[test]
fn gen_next_fact_u5_generator_or_array() {
    holds("const src: any = process.argv.length > 5 ? gen() : [new Alt()]; for (const s of src) { console.log(g(s)); break; }");
}

#[test]
fn gen_next_fact_u6_escaped_generator_object() {
    excluded("const it: any = gen(); external(it); for (const s of it) { console.log(g(s)); break; }");
}

/// `for await` makes no for-of record: no site the fact could name.
#[test]
fn gen_next_fact_u7_async_generator() {
    let (pf, sources) = facts_and_sources(
        "async function* ag() { yield new Svc(); } async function run() { for await (const s of ag()) { console.log(g(s)); } } run();",
    );
    let mb = pf.bindings_for("t.ts").expect("module bindings");
    assert!(
        mb.locals.iter().all(|(_, b)| !b.generator_source),
        "{sources:?}"
    );
}

#[test]
fn gen_next_fact_p1_plain() {
    holds("for (const s of gen()) { console.log(g(s)); }");
}

#[test]
fn gen_next_fact_p2_nested() {
    let g2 = generator_sources(
        "function* outer() { for (const s of gen()) yield s; }
         for (const s of outer()) { console.log(g(s)); }",
    );
    assert!(g2.len() >= 2 && g2.iter().all(|&b| b), "{g2:?}");
}

#[test]
fn gen_next_fact_p3_stored_and_passed() {
    holds(
        "class Box { it: any; constructor(i: any) { this.it = i; } }
         const b = new Box(gen());
         function run(x: any) { for (const s of x) { console.log(g(s)); } }
         run(b.it);",
    );
}

/// A store into an unknown value under `next` leaves the fact (DEC 250:
/// `%GeneratorPrototype%.next` is locked).
#[test]
fn gen_next_fact_p4_top_store_of_next() {
    holds(
        "const f: any = (globalThis as any)[process.argv[2] ?? \"Object\"];
         const u: any = f(process.argv[3]);
         u.next = (q: any) => q;
         for (const s of gen()) { console.log(g(s)); }",
    );
}

/// An own key of any name moves the instance off its birth layout, where
/// the direct resume reads the record.
#[test]
fn gen_next_fact_u8_own_other_key() {
    excluded(
        "const it: any = gen(); it.tag = 1; for (const s of it) { console.log(g(s), it.tag); }",
    );
}

/// Reading an absent own key stores nothing.
#[test]
fn gen_next_fact_p5_read_absent_key() {
    holds("const it: any = gen(); console.log(it.tag); for (const s of it) { console.log(g(s)); }");
}

/// `generator_source` of the payload local of every protocol for-of step
/// (`iteratorStep(LocalGet(payload), ..)`, what codegen's native step site
/// names), in program order: generator bodies first.
fn step_payloads(src: &str) -> Vec<bool> {
    let ast = perry_parser::parse_typescript(
        &format!(
            r#"
        declare function external(x?: unknown): any;
            class Svc {{ size = 3; }}
            class Alt {{ other = 1; size = 4; }}
            class Called {{ value: any; constructor(v: any) {{ this.value = v; }} }}
            function g(a: any) {{ return a.size; }}
            function* gen() {{ yield new Svc(); }}
            {src}
            "#
        ),
        "t.ts",
    )
    .expect("parses");
    let mut hir = perry_hir::lower_module(&ast, "t", "t.ts").expect("lowers");
    perry_transform::transform_generators(&mut hir);
    let text = format!("{hir:?}");
    let head = "\"iteratorStep\", args: [LocalGet(";
    let ids: Vec<u32> = text
        .match_indices(head)
        .map(|(at, _)| {
            let n: String = text[at + head.len()..]
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            n.parse().expect("local id")
        })
        .collect();
    assert!(!ids.is_empty(), "no protocol for-of step");
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    let pf = compute(&[("t.ts".to_string(), &hir)], &cfg);
    let mb = pf.bindings_for("t.ts").expect("module bindings");
    ids.iter()
        .map(|id| {
            mb.locals
                .iter()
                .find(|(l, _)| l == id)
                .is_some_and(|(_, b)| b.generator_source)
        })
        .collect()
}

/// P6. A for-of inside a generator body over a parameter that holds only
/// generators: its payload is GetIterator(source), which returns the
/// generator itself, so the `[iterator#n]` object is never born.
#[test]
fn gen_next_fact_p6_generator_body_forof_payload() {
    let p = step_payloads(
        "function* range(n: number) { for (let i = 0; i < n; i++) yield new Svc(); }
         function* mapGen(it: any, f: (x: any) => any) { for (const v of it) yield f(v); }
         let t = 0;
         for (const s of mapGen(range(3), (x: any) => x)) { t += g(s); }
         console.log(t);",
    );
    assert!(p.iter().all(|&b| b), "{p:?}");
}

/// U9. The same body fed a generator AND an array: GetIterator over the
/// array makes the iterator object, and array mode puts the array itself
/// in the payload. The step inside `wrap` keeps the runtime step.
#[test]
fn gen_next_fact_u9_generator_body_forof_mixed_source() {
    let p = step_payloads(
        "function* wrap(src: any) { for (const v of src) yield v; }
         for (const s of wrap(gen())) { console.log(g(s)); }
         for (const s of wrap([new Alt()])) { console.log(g(s)); }",
    );
    assert!(!p[0], "{p:?}");
}

/// U10. A Set source: its `@@iterator` returns a fresh Set iterator.
#[test]
fn gen_next_fact_u10_generator_body_forof_set_source() {
    let p = step_payloads(
        "function* wrap(src: any) { for (const v of src) yield v; }
         for (const s of wrap(gen())) { console.log(g(s)); }
         for (const s of wrap(new Set([new Alt()]))) { console.log(g(s)); }",
    );
    assert!(!p[0], "{p:?}");
}

/// U11. A generator with an own key: not the birth layout.
#[test]
fn gen_next_fact_u11_generator_body_forof_own_key() {
    let p = step_payloads(
        "function* wrap(src: any) { for (const v of src) yield v; }
         const it: any = gen(); it.tag = 1;
         for (const s of wrap(it)) { console.log(g(s)); }",
    );
    assert!(!p[0], "{p:?}");
}
