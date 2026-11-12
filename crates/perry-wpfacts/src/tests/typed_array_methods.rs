//! ECMA-262 23.2: numeric inputs, array-like reads, callbacks and species.
use super::*;
fn check(body: &str, yes: bool) {
    let src = format!(
        r#"
        (globalThis as any).hostTake = (_x: any) => {{}};
        class Svc {{ marker = 3; length = 1; leak() {{ (globalThis as any).hostTake(this); }} }}
        function g(a: any) {{ return a.marker; }}
        const o: any = new Svc(); const a: any = new Uint8Array([1, 2]);
        {body}
        console.log(g(o));
    "#
    );
    super::dump_helper_fixture(&src);
    let pf = facts(&src);
    let v = &site(&pf, "marker").verdict;
    assert_eq!(!matches!(v, Verdict::No(_)), yes, "{body}: {v:?}");
}
#[test]
fn typed_numeric_methods_read_only() {
    for op in [
        "a.at(o)",
        "a.copyWithin(o, o, o)",
        "a.fill(o, o, o)",
        "a.includes(o, o)",
        "a.indexOf(o, o)",
        "a.lastIndexOf(o, o)",
        "a.join(o)",
        "a.with(o, o)",
    ] {
        check(op, true);
    }
}
#[test]
fn typed_set_reads_array_like() {
    check("a.set(o, 0)", true);
}
#[test]
fn typed_constructors_read_source() {
    for op in [
        "new Uint8Array(o)",
        "new Int32Array(o)",
        "Uint8Array.from(o)",
    ] {
        check(op, true);
    }
}
#[test]
fn typed_callback_result_is_only_coerced() {
    for op in [
        "a.map(() => o)",
        "a.forEach(() => o)",
        "a.some(() => o)",
        "a.every(() => o)",
        "a.find(() => o)",
        "a.findIndex(() => o)",
    ] {
        check(op, true);
    }
}
#[test]
fn typed_callback_this_arg_is_passed() {
    check("a.forEach(function() { this.leak(); }, o)", false);
}
#[test]
fn typed_map_coerces_callback_result() {
    check(
        "o.valueOf = function() { this.leak(); return 1; }; a.map(() => o)",
        false,
    );
}
#[test]
fn typed_sort_coerces_comparator_result() {
    check(
        "o.valueOf = function() { this.leak(); return 1; }; a.sort(() => o)",
        false,
    );
}
#[test]
fn typed_set_runs_length_getter() {
    check(
        "Object.defineProperty(o, 'length', {get() { o.leak(); return 1; }}); a.set(o)",
        false,
    );
}
#[test]
fn typed_constructor_runs_iterator() {
    check(
        "o[Symbol.iterator] = function*() { this.leak(); yield 1; }; new Uint8Array(o)",
        false,
    );
}
#[test]
fn typed_species_constructor_runs() {
    check("a.constructor = { [Symbol.species]: class C { constructor(n: number) { o.leak(); return new Uint8Array(n); } } }; a.slice(0)", false);
}
#[test]
fn typed_replaced_method_stays_conservative() {
    check("a.set = (x: any) => x.leak(); a.set(o)", false);
}
#[test]
fn typed_unknown_receiver_stays_conservative() {
    check(
        "import \"unseen-host\"; const x: any = (globalThis as any).unknown; x.set(o)",
        false,
    );
}

/// A species-made result fed back to the call that made it settles: the
/// objects it constructs are the call's own templates, one per site, not one
/// per run (a fresh one each time never reaches a fixpoint).
#[test]
fn a_species_result_fed_back_to_its_call_settles() {
    let src = r#"
        function g(a: any) { return a.marker; }
        class Svc { marker = 3; }
        let t: any = new Uint8Array(8);
        for (let i = 0; i < 3; i++) { t = t.slice(1); t = t.subarray(1); t = t.map((x: number) => x + 1); }
        console.log(g(new Svc()), t.length);
    "#;
    let pf = facts(src);
    assert!(!pf.stats.aborted, "{:?}", pf.stats);
    assert!(pf.stats.templates < 60, "templates {}", pf.stats.templates);
}
/// skin (three.js GLTFLoader via skinbench.ts:11): the slice of a Buffer's
/// `.buffer` is a fresh ArrayBuffer (ECMA-262 25.1.6.7); as a Buffer method
/// a view of the same memory. Neither is a primitive box whose computed
/// read is an unknown builtin function, so `.map` of a computed read of it
/// never calls an unknown function with `o`.
#[test]
fn buffer_slice_is_a_buffer_or_an_array_buffer() {
    check(
        "const fs = require(\"fs\"); const b: any = fs.readFileSync(process.argv[1]); \
         const s: any = b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength); \
         const v: any = s[process.argv[2]] || []; v.map(o);",
        true,
    );
}
/// Control: unknown code holding the slice may write any method onto it.
#[test]
fn buffer_slice_held_by_unknown_code_escapes() {
    check(
        "const fs = require(\"fs\"); const b: any = fs.readFileSync(process.argv[1]); \
         const s: any = b.buffer.slice(0, 2); new Function(process.argv[2])(s); s.q(o);",
        false,
    );
}
/// A key neither `%TypedArray%.prototype` nor `Object.prototype` declares:
/// the call throws a TypeError before its argument is used (13.3.6.2).
#[test]
fn typed_absent_method_runs_nothing() {
    check("a.traverse(o)", true);
}
/// Control: unknown code holding the array may write `traverse` onto it.
#[test]
fn typed_absent_method_written_by_unknown_code_escapes() {
    check("new Function(process.argv[2])(a); a.traverse(o)", false);
}
