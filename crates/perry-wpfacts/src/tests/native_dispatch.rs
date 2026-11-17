//! DESIGN-native-dispatch PR 1: the dispatcher fact. A closed program whose
//! every route into the runtime's generic method dispatcher is a named
//! member call is complete, and its call sites state their obligations;
//! each route no site states keeps the fact open (every stage live). The
//! programs print the same with facts on and off (the lane's gate runs
//! them against node).
use super::*;
use crate::NativeDispatch;

fn dispatch(src: &str) -> (ProgramFacts, NativeDispatch) {
    let path = "main.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "main", &path).expect("lowers");
    let list: Vec<(String, &perry_hir::Module)> = vec![(path, &hir)];
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    let pf = crate::compute(&list, &cfg);
    let d = pf
        .native_dispatch
        .clone()
        .expect("the fact is computed for a whole program");
    (pf, d)
}

/// node: prints `107`.
const CLOSED: &str = "class P {\n\
       x: number;\n\
       constructor(x: number) { this.x = x; }\n\
       twice() { return this.x * 2; }\n\
     }\n\
     const p = new P(3);\n\
     const s = 'abc';\n\
     const a = [1, 2, 3];\n\
     const n = s.charCodeAt(1) + a.indexOf(2) + p.twice() + Math.max(1, 2);\n\
     console.log(n);\n";

#[test]
fn a_closed_program_is_complete_and_lists_its_obligations() {
    let (pf, d) = dispatch(CLOSED);
    assert!(d.complete(), "{:?}\n{}", d.open, pf.dump_text());
    let obligations: Vec<_> = pf.dispatch_obligations().collect();
    let of = |key: &str| {
        obligations
            .iter()
            .find(|o| o.key == key)
            .copied()
            .unwrap_or_else(|| panic!("no obligation for .{key}\n{}", pf.dump_text()))
    };
    // A String method on a primitive receiver, at its named site.
    let cc = of("charCodeAt");
    assert_eq!((cc.prims, cc.op), (crate::model::prim::STR, None));
    // The class method on P.
    let twice = of("twice");
    assert_eq!(twice.prims, 0);
    assert!(
        twice.receivers.iter().any(|r| r.display.contains('P')),
        "{twice:?}"
    );
    // HIR-specialized member ops state their own obligation: the folded
    // `a.indexOf(2)` on the program's array (an exact Array: its fallback
    // edge cannot be taken) and `Math.max` on its namespace (a direct
    // call).
    let index_of = of("indexOf");
    assert_eq!(index_of.op, Some("ArrayIndexOf"), "{index_of:?}");
    assert!(
        !index_of.receivers.is_empty() && !index_of.redispatch,
        "{index_of:?}"
    );
    let max = of("max");
    assert_eq!((max.op, max.namespace), (Some("MathMax"), Some("Math")));
    // `console.log`: a named call site on the host console (or, lowered to a
    // `NativeMethodCall`, that op on the `console` namespace).
    assert!(
        obligations.iter().any(|o| o.key == "log"),
        "{obligations:?}"
    );
    assert!(
        pf.member_ops.iter().all(|o| o.stated),
        "{:?}",
        pf.member_ops
    );
    let dump = pf.dump_text();
    assert!(dump
        .lines()
        .any(|l| l.starts_with("# native_dispatch complete obligations=")));
    assert!(
        dump.lines()
            .any(|l| l.starts_with("# member_op ") && l.contains("ArrayIndexOf key=indexOf")),
        "{dump}"
    );
}

/// KEEP: a specialized op whose receiver may be no Array. The HIR folds
/// `a.indexOf(2)` by the annotation; the value may be an object with its
/// own `indexOf`, which the fold's guard calls through the generic path.
/// The op must list that fallback obligation (or the fact is open).
/// node: prints `1` (no argument) / `7` (an argument).
#[test]
fn a_specialized_op_on_a_maybe_non_array_lists_its_fallback() {
    let (pf, d) = dispatch(
        "class Fake { indexOf(_x: number) { return 7; } }\n\
         const a: number[] = process.argv.length > 9 ? (new Fake() as any) : [1, 2, 3];\n\
         console.log(a.indexOf(2));\n",
    );
    if d.complete() {
        let fallback = pf
            .dispatch_obligations()
            .find(|o| o.key == "indexOf" && o.op.is_some())
            .unwrap_or_else(|| panic!("no indexOf op\n{}", pf.dump_text()));
        assert!(fallback.redispatch, "{fallback:?}");
        assert!(
            fallback
                .receivers
                .iter()
                .any(|r| r.display.contains("Fake")),
            "{fallback:?}"
        );
    }
}

/// A receiver op the generation lowers through a shared arm states no
/// receiver: the fact is open (O4).
/// node: prints `[ 'a', 'b' ]`.
#[test]
fn a_specialized_op_with_no_receiver_record_keeps_the_fact_open() {
    let why = open_reason("const s = 'a,b';\nconsole.log(s.split(','));\n");
    assert!(why.starts_with("O4:") || why.starts_with("O3:"), "{why}");
}

/// Each twin adds one route no named member-call site states.
fn open_reason(src: &str) -> String {
    let (pf, d) = dispatch(src);
    d.open
        .unwrap_or_else(|| panic!("complete, expected open:\n{}", pf.dump_text()))
}

#[test]
fn a_borrowed_method_keeps_the_fact_open() {
    // node: prints `98`.
    let why =
        open_reason("const s = 'abc';\nconst f = s.charCodeAt;\nconsole.log(f.call(s, 1));\n");
    assert!(why.starts_with("O3:"), "{why}");
}

#[test]
fn a_computed_key_call_keeps_the_fact_open() {
    // node: prints `97`.
    let why = open_reason(
        "const s = 'abc';\n\
         const k = process.argv.length > 9 ? 'charAt' : 'charCodeAt';\n\
         console.log((s as any)[k](0));\n",
    );
    assert!(why.starts_with("O3:"), "{why}");
}

#[test]
fn an_implicit_protocol_on_an_object_keeps_the_fact_open() {
    // node: prints `v[object Object]`.
    let why = open_reason("class P { x = 1; }\nconst p = new P();\nconsole.log('v' + p);\n");
    assert!(why.starts_with("O3:"), "{why}");
}

#[test]
fn an_unknown_value_keeps_the_fact_open() {
    // node: prints `undefined`.
    let why = open_reason("const h = (process as any).noSuchMember;\nconsole.log(typeof h);\n");
    assert!(why.starts_with("O1:L1:"), "{why}");
}

#[test]
fn a_per_unit_run_has_no_fact() {
    let path = "main.ts".to_string();
    let module = perry_parser::parse_typescript(CLOSED, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "main", &path).expect("lowers");
    let list: Vec<(String, &perry_hir::Module)> = vec![(path, &hir)];
    let cfg = FactsConfig {
        unit: true,
        ..FactsConfig::default()
    };
    assert!(crate::compute(&list, &cfg).native_dispatch.is_none());
}

/// The Static keys are the real JS member names; a sub-object member has the
/// sub-object as its namespace (the names runtime method names match).
#[test]
fn static_member_op_keys_are_the_real_js_names() {
    let (pf, _d) = dispatch(
        "import * as os from \"os\";\n\
         const c = process.stdout.columns;\n\
         const j = JSON.parse(\"[1]\");\n\
         const u = process.getuid();\n\
         console.log(os.EOL.length, c, j, u);\n",
    );
    let has = |op: &str, ns: &str, key: &str| {
        pf.member_ops
            .iter()
            .any(|o| o.op == op && o.namespace.as_deref() == Some(ns) && o.key == key && o.stated)
    };
    assert!(has("OsEOL", "os", "EOL"), "{}", pf.dump_text());
    assert!(
        has("ProcessStdoutColumns", "process.stdout", "columns"),
        "{}",
        pf.dump_text()
    );
    assert!(has("JsonParse", "JSON", "parse"), "{}", pf.dump_text());
    assert!(
        has("ProcessPosixCredential", "process", "getuid"),
        "{}",
        pf.dump_text()
    );
}
