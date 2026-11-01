//! ECMA-262 23.2: numeric inputs, array-like reads, callbacks and species.
use super::*;
fn check(body: &str, yes: bool) {
    let src = format!(r#"
        (globalThis as any).hostTake = (_x: any) => {{}};
        class Svc {{ marker = 3; length = 1; leak() {{ (globalThis as any).hostTake(this); }} }}
        function g(a: any) {{ return a.marker; }}
        const o: any = new Svc(); const a: any = new Uint8Array([1, 2]);
        {body}
        console.log(g(o));
    "#);
    let pf = facts(&src); let v = &site(&pf, "marker").verdict;
    assert_eq!(!matches!(v, Verdict::No(_)), yes, "{body}: {v:?}");
}
#[test]
fn typed_numeric_methods_read_only() {
    for op in ["a.at(o)", "a.copyWithin(o, o, o)", "a.fill(o, o, o)", "a.includes(o, o)", "a.indexOf(o, o)", "a.lastIndexOf(o, o)", "a.join(o)", "a.with(o, o)"] { check(op, true); }
}
#[test]
fn typed_set_reads_array_like() { check("a.set(o, 0)", true); }
#[test]
fn typed_constructors_read_source() {
    for op in ["new Uint8Array(o)", "new Int32Array(o)", "Uint8Array.from(o)"] { check(op, true); }
}
#[test]
fn typed_callback_result_is_only_coerced() {
    for op in ["a.map(() => o)", "a.forEach(() => o)", "a.some(() => o)", "a.every(() => o)", "a.find(() => o)", "a.findIndex(() => o)"] { check(op, true); }
}
#[test]
fn typed_callback_this_arg_is_passed() { check("a.forEach(function() { this.leak(); }, o)", false); }
#[test]
fn typed_map_coerces_callback_result() {
    check("o.valueOf = function() { this.leak(); return 1; }; a.map(() => o)", false);
}
#[test]
fn typed_sort_coerces_comparator_result() {
    check("o.valueOf = function() { this.leak(); return 1; }; a.sort(() => o)", false);
}
#[test]
fn typed_set_runs_length_getter() {
    check("Object.defineProperty(o, 'length', {get() { o.leak(); return 1; }}); a.set(o)", false);
}
#[test]
fn typed_constructor_runs_iterator() {
    check("o[Symbol.iterator] = function*() { this.leak(); yield 1; }; new Uint8Array(o)", false);
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
    check("const x: any = (globalThis as any).unknown; x.set(o)", false);
}
