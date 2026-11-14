//! DEADFN14 rule 2 (NOT BUILT): `shift()` on a function\x27s own rest array
//! returns what was stored directly at index 0 (the argument binding and
//! literal-0 stores), plus `undefined` when the length can be 0; any
//! element whenever the array may have been reordered, written at an
//! unknown index or under `length`, shifted by a second template site, by
//! one site more than once (a loop, another activation\x27s array), or
//! handed to a heap field or unknown code first. `A` escapes iff it reaches
//! a `getLocation` call (its `getLocation` leaks `this`).
use super::*;

fn whole(src: &str) -> ProgramFacts {
    let path = "t.cts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

fn escape_of(pf: &ProgramFacts, key: &str) -> Option<String> {
    pf.templates
        .iter()
        .filter(|t| t.display.starts_with("t.cts"))
        .find(|t| t.fields.iter().any(|f| f.key == key))
        .unwrap_or_else(|| panic!("no t.cts template with field {key}\n{}", pf.dump_text()))
        .escape
        .clone()
}

const HEAD: &str = "const unknown: any = new Function(process.argv[2])();
     class A { a = 1; isStackTrace: any = unknown.flag; getLocation() { unknown[1] = this; return \x27a\x27; } }
     const holder: any = { x: 0 };
     function id(q: any) { return q; }
     function use(m: any) { if (m && m.isStackTrace) m.getLocation(); }
";

fn run(body: &str) -> ProgramFacts {
    whole(&format!("{HEAD}\n{body}"))
}

fn exact(body: &str) {
    let pf = run(body);
    assert_eq!(escape_of(&pf, "a"), None, "{}", pf.dump_text());
}

fn any_element(body: &str) {
    let pf = run(body);
    assert!(escape_of(&pf, "a").is_some(), "{}", pf.dump_text());
}

/// (a) The shifted value is the first argument only.
#[test]
fn a_shift_returns_index_zero() {
    exact("function f(...p: any[]) { const m = p.shift(); use(m); return p; } f(\x27lit\x27, new A());");
}

/// (a) After the shift, index 0 holds the second argument.
#[test]
fn a_after_shift_index_zero_holds_the_next() {
    any_element("function f(...p: any[]) { p.shift(); use(p[0]); } f(\x27lit\x27, new A());");
}

/// (b) An unknown-index store before the shift.
#[test]
fn b_unknown_index_store_falls_back() {
    any_element("function f(...p: any[]) { p[process.argv.length] = new A(); use(p.shift()); } f(\x27lit\x27);");
}

/// (c) A reordering, a second shift site, a shift in a loop.
#[test]
fn c_reverse_falls_back() {
    any_element("function f(...p: any[]) { p.reverse(); use(p.shift()); } f(\x27lit\x27, new A());");
}
#[test]
fn c_sort_falls_back() {
    any_element("function f(...p: any[]) { p.sort(); use(p.shift()); } f(\x27lit\x27, new A());");
}
#[test]
fn c_second_shift_site_falls_back() {
    any_element("function f(...p: any[]) { p.shift(); use(p.shift()); } f(\x27lit\x27, new A());");
}
#[test]
fn c_shift_in_loop_falls_back() {
    any_element("function f(...p: any[]) { for (let i = 0; i < 2; i++) use(p.shift()); } f(\x27lit\x27, new A());");
}

/// (d) The array reached a heap field or unknown code first.
#[test]
fn d_heap_field_falls_back() {
    any_element("function f(...p: any[]) { holder.x = p; use(p.shift()); } f(\x27lit\x27, new A());");
}
#[test]
fn d_unknown_code_falls_back() {
    any_element("function f(...p: any[]) { unknown(p); use(p.shift()); } f(\x27lit\x27, new A());");
}

/// (e) No arguments: undefined (and the other call\x27s literal).
#[test]
fn e_no_arguments_is_undefined() {
    exact("function f(...p: any[]) { use(p.shift()); return p; } f(); f(\x27lit\x27, new A());");
}

/// (f) A literal-0 store before the shift: its value.
#[test]
fn f_index_zero_store_is_returned() {
    any_element("function f(...p: any[]) { p[0] = new A(); use(p.shift()); } f(\x27lit\x27);");
}

/// (g) Through an identity function and back: still the own array.
#[test]
fn g_identity_round_trip_is_exact() {
    exact("function f(...p: any[]) { p = id(p); use(p.shift()); return p; } f(\x27lit\x27, new A());");
}

/// (h) The binding reassigned with another activation\x27s array: that array
/// is shifted twice.
#[test]
fn h_reassigned_binding_recursion_falls_back() {
    any_element(
        "function h(x: any, ...p: any[]) { if (x) p = x; use(p.shift()); if (!x) h(p); } h(0, \x27lit\x27, new A());",
    );
}

/// (j) The array captured by a closure that shifts it later.
#[test]
fn j_closure_shift_falls_back() {
    any_element("function f(...p: any[]) { const g = () => p.shift(); use(g()); return p; } f('lit', new A());");
}

/// (k) A callee that shifts the array first: two sites.
#[test]
fn k_callee_shift_falls_back() {
    any_element(
        "function g(a: any) { return a.shift(); } function f(...p: any[]) { g(p); use(p.shift()); return p; } f('lit', new A());",
    );
}

/// (l) Returned from a summarized callee into the own local: confined.
#[test]
fn l_returned_into_own_local_is_exact() {
    exact("function f(...p: any[]) { p = id(p); const m = p.shift(); use(m); return p; } f('lit', new A());");
}

/// (m) An escaped function (unknown callers) plus one direct call: no
/// crash, and the first arguments only (the direct call's `'lit'`), never
/// its second.
#[test]
fn m_template_body_with_unknown_caller() {
    exact("function f(...p: any[]) { use(p.shift()); } unknown(f); f('lit', new A());");
}

/// (m) What an unknown caller passes first reaches the template body's
/// shift: the value is any value, so a store into it is a store into
/// whatever unknown code holds.
#[test]
fn m_unknown_caller_value_reaches_shift() {
    let pf = run("function f(...p: any[]) { const m = p.shift(); use(m); return p; } unknown(f); unknown(new A()); f('lit');");
    assert!(escape_of(&pf, "a").is_some(), "{}", pf.dump_text());
}
