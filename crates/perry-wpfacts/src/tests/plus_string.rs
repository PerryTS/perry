//! Rule P: `l + r` is a String once either operand surely is one (ECMA-262
//! 13.15.3 ApplyStringOrNumericBinaryOperator: after ToPrimitive a String
//! operand concatenates; only two non-Strings add as numbers). The numeric
//! kinds of one operand reach the result only when the other may be no
//! String, so a member read on a String accumulator (`s += k` with a host
//! or parameter `k`) is a String member. An operand that may be a Number
//! or an object on both sides keeps the result unknown.
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

/// The verdict of the one `.length` site that follows `marker` in `src`.
fn length_at(pf: &ProgramFacts, src: &str, marker: &str) -> Verdict {
    let at = src.find(marker).expect("marker") as u32;
    pf.sites
        .iter()
        .filter(|s| s.key == "length" && s.byte_offset >= at)
        .min_by_key(|s| s.byte_offset)
        .unwrap_or_else(|| panic!("site .length after {marker:?} in\n{}", pf.dump_text()))
        .verdict
        .clone()
}

fn proven(v: &Verdict) -> bool {
    !matches!(v, Verdict::No(_))
}

/// `body` defines what `call` runs; the `.length` read after `marker` is
/// proven in both modes.
fn proves(body: &str, call: &str, marker: &str) {
    let src = format!("{body}\nconsole.log({call});\n");
    for per_unit in [false, true] {
        let v = length_at(&facts_of(&src, per_unit), &src, marker);
        assert!(proven(&v), "per_unit={per_unit} {marker}: {v:?}");
    }
}

/// The `.length` read after `marker` stays unproven in both modes.
fn unproven(body: &str, call: &str, marker: &str) {
    let src = format!("{body}\nconsole.log({call});\n");
    for per_unit in [false, true] {
        let v = length_at(&facts_of(&src, per_unit), &src, marker);
        assert!(!proven(&v), "per_unit={per_unit} {marker}: {v:?}");
    }
}

// A host value (`process.argv[2]`: a String or `undefined`) or an unknown
// value (`JSON.parse`) on the right of a String accumulator.

#[test]
fn string_accumulator_plus_host_string() {
    proves(
        "function f(k: any) { let s = \"\"; s += k; return s.length; }",
        "f(process.argv[2])",
        "return s",
    );
}

#[test]
fn string_accumulator_plus_unknown() {
    proves(
        "function f(k: any) { let s = \"\"; s += k; return s.length; }",
        "f(JSON.parse(process.argv[2]))",
        "return s",
    );
}

#[test]
fn string_literal_plus_object() {
    proves(
        "function f(o: any) { return (\"x\" + o).length; }",
        "f({ valueOf() { return 1; } })",
        "(\"x\"",
    );
}

#[test]
fn unknown_plus_string_literal() {
    proves(
        "function f(k: any) { return (k + \"x\").length; }",
        "f(JSON.parse(process.argv[2]))",
        "(k +",
    );
}

#[test]
fn string_field_accumulator_plus_unknown() {
    proves(
        "class A { input = \"\"; on(key: any) { this.input += key; return this.input.length; } }",
        "new A().on(JSON.parse(process.argv[2]))",
        "return this",
    );
}

// Both operands may be no String: Number + unknown, a String-or-Number
// accumulator, two objects whose ToPrimitive may return Numbers.

#[test]
fn number_plus_unknown_stays_unknown() {
    unproven(
        "function f(n: number, k: any) { return (n + k).length; }",
        "f(1, JSON.parse(process.argv[2]))",
        "(n +",
    );
}

#[test]
fn string_or_number_accumulator_stays_unknown() {
    unproven(
        "function f(c: boolean, k: any) { let s: any = \"\"; if (c) s = 1; s += k; return s.length; }",
        "f(process.argv.length > 3, JSON.parse(process.argv[2]))",
        "return s",
    );
}

#[test]
fn object_plus_object_stays_unknown() {
    unproven(
        "function f(a: any, b: any) { return (a + b).length; }",
        "f({ valueOf() { return 1; } }, { valueOf() { return 2; } })",
        "(a +",
    );
}
