//! W3HOST3 (2026-11-16). A: a for-of over a spread-free array literal keeps
//! its elements in locals; IteratorClose asks `arrayRecordCloseAbsent` (a
//! shape lookup of `return` on %ArrayIteratorPrototype%'s chain that calls
//! nothing) whether the record must be materialized: its result is a
//! boolean, not an unknown value (ECMA-262 14.7.5.7 ForIn/OfBodyEvaluation,
//! 7.4.11 IteratorClose: GetMethod(iterator, "return")).
use super::*;

fn facts_of(src: &str) -> ProgramFacts {
    let path = "main.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "main", &path).expect("lowers");
    let list: Vec<(String, &perry_hir::Module)> = vec![(path, &hir)];
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    crate::compute(&list, &cfg)
}

/// The verdict of the first site `.key` after `marker`, and the dump.
fn verdict(src: &str, key: &str, marker: &str) -> (Verdict, String) {
    let pf = facts_of(src);
    let at = src.find(marker).expect("marker") as u32;
    let s = pf
        .sites
        .iter()
        .filter(|s| s.key == key && s.byte_offset >= at)
        .min_by_key(|s| s.byte_offset)
        .unwrap_or_else(|| panic!("site .{key} after {marker:?} in\n{}", pf.dump_text()));
    (s.verdict.clone(), pf.dump_text())
}

/// The reasons of the unknown values the solve created (explain on, so
/// each origin is its own node) and whether unknown code holds the realm.
fn top_origins(src: &str) -> (Vec<String>, bool) {
    let ast = Box::leak(Box::new(
        perry_parser::parse_typescript(src, "t.ts").unwrap(),
    ));
    let hir = Box::leak(Box::new(perry_hir::lower_module(ast, "t", "t.ts").unwrap()));
    let mut g = crate::gen::Gen::new(vec![("t.ts", &*hir)], false, true);
    g.m.max_work = 50_000_000;
    g.register_all();
    g.link_exports();
    g.walk_all();
    g.feed_parent_values();
    g.m.summarize_and_replay();
    g.m.solve();
    (g.m.top_vals.keys().cloned().collect(), g.m.realm_held)
}

const LOOP: &str = "const w = process.argv[2] ?? String(Math.random());
const out: Record<string, string> = {};
for (const side of [\"top\", w, \"left\"]) { if (side === w) break; out[\"m-\" + side] = w; }
console.log(Object.keys(out).length);
";

// A-R1 (RED on base): the close test of a literal for-of is a boolean.
#[test]
fn w3j_a_r1_literal_for_of_close_test_is_a_boolean() {
    let (tops, held) = top_origins(LOOP);
    assert!(
        !tops.iter().any(|t| t.contains("arrayRecordCloseAbsent")),
        "no unknown value from the close test: {tops:?}"
    );
    assert!(!held, "realm not held");
}

// A-K1 (KEEP): a program `return` method on %ArrayIteratorPrototype% runs
// at the break (IteratorClose calls it with the iterator as this).
#[test]
fn w3j_a_k1_program_return_runs_at_break() {
    let src = format!(
        "class R {{ v = 1; }}\nconst r = new R();\n\
         const AIP = Object.getPrototypeOf([][Symbol.iterator]());\n\
         AIP.return = function () {{ console.log(\"RET\", r.v); return {{}}; }};\n{LOOP}"
    );
    let pf = facts_of(&src);
    assert!(
        pf.array_iterator_close_written.is_some(),
        "close is observable"
    );
    let (v, dump) = verdict(&src, "v", "\"RET\"");
    assert_ne!(v, Verdict::Dead, "return() body runs\n{dump}");
}

// A-K2 (KEEP): a `return` getter runs at the break (GetMethod is [[Get]]).
#[test]
fn w3j_a_k2_return_getter_runs_at_break() {
    let src = format!(
        "class R {{ v = 1; }}\nconst r = new R();\n\
         const AIP = Object.getPrototypeOf([][Symbol.iterator]());\n\
         Object.defineProperty(AIP, \"return\", {{ get() {{ console.log(\"GET\", r.v); return undefined; }} }});\n{LOOP}"
    );
    let pf = facts_of(&src);
    assert!(
        pf.array_iterator_close_written.is_some(),
        "close is observable"
    );
    let (v, dump) = verdict(&src, "v", "\"GET\"");
    assert_ne!(v, Verdict::Dead, "getter body runs\n{dump}");
}

// A-K3 (KEEP): a replaced Array.prototype[Symbol.iterator] drives the loop
// (the iterator protocol, 7.4.2 GetIterator): its body runs.
#[test]
fn w3j_a_k3_replaced_array_iterator_runs() {
    let src = format!(
        "class R {{ v = 1; }}\nconst r = new R();\n\
         (Array.prototype as any)[Symbol.iterator] = function* (this: any) {{ console.log(\"IT\", r.v); yield this[0]; }};\n{LOOP}"
    );
    let pf = facts_of(&src);
    assert!(
        pf.array_iteration_written.is_some(),
        "iteration is observable"
    );
    let (v, dump) = verdict(&src, "v", "\"IT\"");
    assert_ne!(v, Verdict::Dead, "iterator body runs\n{dump}");
}

// B (S6b): `process.cwd` read as a value and called (`?.()`, an alias):
// the bound method of the process object runs its declared body
// (js_process_cwd: a string, nothing else), as the direct call does
// (ECMA-262 13.3.9 OptionalChain: only a nullish test of the callee value;
// 13.3.6.2 EvaluateCall: `this` is ignored by the bound method).
const CWD: &str = "class M { v = 1; }\nconst m = new M();\n";

// B-R1 (RED on base): the optional call and the alias call do not reach
// unknown code.
#[test]
fn w3j_b_r1_cwd_value_call_is_declared() {
    for call in [
        "const s = process.cwd?.() ?? \"\";",
        "const f = process.cwd; const s = f();",
    ] {
        let src = format!("{CWD}{call}\nconsole.log(s.length, m.v);\n");
        let (tops, held) = top_origins(&src);
        assert!(!held, "{call}: realm not held; unknown values {tops:?}");
        assert!(
            !tops.iter().any(|t| t.contains("Process")),
            "{call}: {tops:?}"
        );
    }
}

// B-K1 (KEEP): a program function stored as `process.cwd` (by argv) is
// what the optional call runs: its body is reached.
#[test]
fn w3j_b_k1_program_cwd_runs() {
    let src = format!(
        "{CWD}if (process.argv[2] === \"x\") (process as any).cwd = () => {{ console.log(\"PC\", m.v); return \"p\"; }};\n\
         const s = process.cwd?.() ?? \"\";\nconsole.log(s.length);\n"
    );
    let (v, dump) = verdict(&src, "v", "\"PC\"");
    assert_ne!(v, Verdict::Dead, "program cwd body runs\n{dump}");
}
