//! A built-in that reads or writes a program object runs the program code
//! that object holds: its [[Get]] and [[Set]] under an index key run the
//! accessors defined under that key, on the object or its chain, with the
//! object as `this` (ECMA-262 23.1.3, the Array.prototype algorithms). The
//! receivers are `any`, so each call stays a call of the native, not an HIR
//! array fold. CXCHK0 gap witnesses test_gap_array_adaptive_sort,
//! test_gap_array_shift_observable, test_gap_array_some_callback_arity.

use super::*;

/// Whether `hit` is an active body in `src`.
fn hit_runs(src: &str) -> bool {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let cfg = FactsConfig {
        max_work: Some(50_000_000),
        ..FactsConfig::default()
    };
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], &cfg);
    assert!(pf.body_reach.complete, "activated");
    let id = hir
        .functions
        .iter()
        .find(|f| f.name == "hit")
        .expect("hit")
        .id;
    pf.body_reach.bodies.iter().any(|(_, i, _)| *i == id)
}

fn runs(name: &str, src: &str) {
    assert!(hit_runs(src), "{name}: hit inactive, Node runs it");
}

fn stays(name: &str, src: &str) {
    assert!(!hit_runs(src), "{name}: hit active, Node never runs it");
}

#[test]
fn an_element_read_of_a_native_runs_an_index_getter() {
    runs(
        "join",
        r#"
function hit() { return 7; }
const a: any = [5, 4, 3];
Object.defineProperty(a, "0", { get() { return hit(); }, configurable: true });
console.log(a.join(","));
"#,
    );
    runs(
        "some",
        r#"
function hit() { return 20; }
const a: any = [1, 2, 3];
Object.defineProperty(a, 1, { get() { return hit(); }, enumerable: true, configurable: true });
console.log(a.some((x: any) => x > 10));
"#,
    );
    runs(
        "shift",
        r#"
function hit() { return 10; }
const a: any = [10, 20, 30];
Object.defineProperty(a, "0", { get() { return hit(); }, configurable: true });
console.log(a.shift());
"#,
    );
}

#[test]
fn an_array_operation_on_a_typed_array_runs_an_index_getter() {
    // A typed receiver: the call is an HIR array operation, not a call of
    // the native.
    runs(
        "join",
        r#"
function hit() { return 7; }
const a = [5, 4, 3];
Object.defineProperty(a, "0", { get() { return hit(); }, configurable: true });
console.log(a.join(","));
"#,
    );
    runs(
        "some",
        r#"
function hit() { return 20; }
const a = [1, 2, 3];
Object.defineProperty(a, 1, { get() { return hit(); }, enumerable: true, configurable: true });
console.log(a.some((x) => x > 10));
"#,
    );
    runs(
        "shift",
        r#"
function hit() { return 10; }
const a = [10, 20, 30];
Object.defineProperty(a, "0", { get() { return hit(); }, configurable: true });
console.log(a.shift());
"#,
    );
    runs(
        "for-of",
        r#"
function hit() { return 10; }
const a = [10, 20, 30];
Object.defineProperty(a, "1", { get() { return hit(); }, configurable: true });
for (const v of a) console.log(v);
"#,
    );
}

#[test]
fn an_element_write_of_a_native_runs_an_index_setter() {
    runs(
        "shift",
        r#"
function hit() { return 1; }
const a: any = [10, 20, 30];
Object.defineProperty(a, "0", { get() { return 10; }, set(v: any) { hit(); }, configurable: true });
console.log(a.shift());
"#,
    );
}

#[test]
fn a_getter_under_a_named_key_stays_inactive() {
    // Controls: an element read names numeric keys only.
    stays(
        "join",
        r#"
function hit() { return 7; }
const a: any = [5, 4, 3];
Object.defineProperty(a, "x", { get() { return hit(); }, configurable: true });
console.log(a.join(","));
"#,
    );
    stays(
        "shift",
        r#"
function hit() { return 7; }
const a: any = [5, 4, 3];
Object.defineProperty(a, "x", { get() { return hit(); }, configurable: true });
console.log(a.shift());
"#,
    );
}
