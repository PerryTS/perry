//! CHB6: the result of a builtin string method is the value its algorithm
//! returns (ECMA-262 22.1.3, 22.2.6), not a container of primitives: `trim`,
//! `slice`, `toLowerCase`, `replace` return a String, `split` an Array of
//! Strings, RegExp `@@split` an Array. A member read or call on that result
//! is a String (or Array) member. A program that writes the method onto the
//! prototype runs what it stored, and a receiver that may be another value
//! runs that value's method: both stay unknown.
use super::*;

fn facts_of(src: &str, per_unit: bool) -> ProgramFacts {
    let path = "main.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "main", &path).expect("lowers");
    let list: Vec<(String, &perry_hir::Module)> = vec![(path, &hir)];
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    match per_unit {
        true => crate::compute_units(&list, &cfg),
        false => crate::compute(&list, &cfg),
    }
}

/// The verdict of the one site of `key` that follows `marker` in `src`.
fn verdict_at(pf: &ProgramFacts, src: &str, marker: &str, key: &str) -> Verdict {
    let at = src.find(marker).expect("marker") as u32;
    pf.sites
        .iter()
        .filter(|s| s.key == key && s.byte_offset >= at)
        .min_by_key(|s| s.byte_offset)
        .unwrap_or_else(|| panic!("site .{key} after {marker:?} in\n{}", pf.dump_text()))
        .verdict
        .clone()
}

fn proven(v: &Verdict) -> bool {
    !matches!(v, Verdict::No(_))
}

/// `body` defines `f`; the site of `key` after `marker` is proven in both
/// modes (per unit never stronger than whole).
fn proves(body: &str, marker: &str, key: &str) {
    let src = format!("{body}\nconsole.log(f(\" a,b \"), f(\"c\"));\n");
    for per_unit in [false, true] {
        let v = verdict_at(&facts_of(&src, per_unit), &src, marker, key);
        assert!(proven(&v), "per_unit={per_unit} {marker} .{key}: {v:?}");
    }
}

fn proven_src(src: &str, marker: &str, key: &str) {
    for per_unit in [false, true] {
        let v = verdict_at(&facts_of(src, per_unit), src, marker, key);
        assert!(proven(&v), "per_unit={per_unit} {marker} .{key}: {v:?}");
    }
}

fn unproven(src: &str, marker: &str, key: &str) {
    for per_unit in [false, true] {
        let v = verdict_at(&facts_of(src, per_unit), src, marker, key);
        assert!(!proven(&v), "per_unit={per_unit} {marker} .{key}: {v:?}");
    }
}

#[test]
fn trim_result_length() {
    proves(
        "function f(s: string) { return s.trim().length; }",
        "s.trim()",
        "length",
    );
}

#[test]
fn trim_result_method_call() {
    proves(
        "function f(s: string) { return s.trim().slice(1); }",
        "s.trim()",
        "slice",
    );
}

#[test]
fn to_lower_case_result_length() {
    proves(
        "function f(s: string) { return s.toLowerCase().length; }",
        "s.toLowerCase()",
        "length",
    );
}

#[test]
fn string_split_result_length() {
    proves(
        "function f(s: string) { return s.split(\",\").length; }",
        "s.split",
        "length",
    );
}

#[test]
fn regexp_split_result_length() {
    proves(
        "function f(s: string) { return s.split(/\\s+/).length; }",
        "s.split",
        "length",
    );
}

#[test]
fn regexp_replace_result_length() {
    proves(
        "function f(s: string) { return s.replace(/a/, \"\").length; }",
        "s.replace",
        "length",
    );
}

#[test]
fn string_replace_result_length() {
    proves(
        "function f(s: string) { return s.replace(\"a\", \"\").length; }",
        "s.replace",
        "length",
    );
}

/// Control: `Array.prototype.slice` already births an exact Array (whole
/// program; per unit the literal argument is another unit's value: CHB2's
/// cause C).
#[test]
fn array_slice_result_length() {
    let src =
        "function f(a: number[]) { return a.slice(1).length; }\nconsole.log(f([1, 2]), f([3]));\n";
    let v = verdict_at(&facts_of(src, false), src, "a.slice", "length");
    assert!(proven(&v), "{v:?}");
}

/// DEC 250: replacing `String.prototype.trim` is a TypeError, so the call
/// runs the builtin and its result is a string.
#[test]
fn a_replaced_locked_prototype_method_keeps_its_result() {
    proven_src(
        "(String.prototype as any).trim = function () { return { length: 9 }; };\n\
         function f(s: string) { return s.trim().length; }\n\
         console.log(f(\" a \"));\n",
        "s.trim()",
        "length",
    );
}

/// A receiver that may be a host value runs that value's `trim`.
#[test]
fn receiver_that_may_be_a_host_value_is_unknown() {
    unproven(
        "import \"unseen-host\"; function f(v: any) { return v.trim().length; }\n\
         console.log(f(\" a \"), f((globalThis as any).hostValue));\n",
        "v.trim()",
        "length",
    );
}

/// A receiver that may be a number: `toFixed` is a Number method, `trim`
/// of the number is absent (TypeError), so `.length` sees only the String.
/// The control for the negative above: the string result is not widened.
#[test]
fn number_or_string_receiver_result_is_a_string() {
    let src = "function f(v: string | number) { return (typeof v === \"number\" ? v.toFixed(1) : v.trim()).length; }\n\
               console.log(f(1), f(\" a \"));\n";
    for per_unit in [false, true] {
        let v = verdict_at(&facts_of(src, per_unit), src, "(typeof", "length");
        assert!(proven(&v), "per_unit={per_unit}: {v:?}");
    }
}
