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

#[test]
fn numeric_literals_are_core_operations_without_dispatch_obligations() {
    assert_eq!(
        crate::member_ops::class(&perry_hir::Expr::Number(1.5)),
        crate::member_ops::Op::Core,
    );
    let (pf, d) = dispatch("console.log(1.5);\n");
    assert!(d.complete(), "{:?}\n{}", d.open, pf.dump_text());
    assert!(pf.member_ops.iter().all(|o| o.op != "Number"));
}

#[test]
fn native_dispatch_direct_this_certificate_requires_the_lowered_route() {
    let (pf, _) = dispatch(
        "class P { m() { return 7; } run() { return this.m(); } }\nconsole.log(new P().run());",
    );
    let s = pf
        .sites
        .iter()
        .find(|s| s.is_call && s.key == "m")
        .expect("live call");
    assert!(matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set));
    assert!(
        !s.dispatch_obligation().unwrap().redispatch,
        "{}",
        pf.dump_text()
    );
    // The same lookup verdict, without the route certificate, is no proof.
    let mut missing = s.clone();
    missing.method_call.as_mut().unwrap().dispatch = None;
    assert!(missing.dispatch_obligation().unwrap().redispatch);
    let (native, _) = dispatch("const s = 'abc'; console.log(s.split(/b/));");
    let s = native
        .sites
        .iter()
        .find(|s| s.is_call && s.key == "split")
        .expect("native call");
    assert_eq!(s.verdict, Verdict::NativeMember);
    assert!(s.dispatch_obligation().unwrap().redispatch);
}

#[test]
fn native_dispatch_named_negative_controls_keep_the_generic_route() {
    for (key, src) in [
        ("m", "const x = (globalThis as any).hostValue; x.m();"),
        ("get", "const x: any = new Map(); x.get = function() { return 7; }; console.log(x.get('a'));"),
        ("get", "(Map.prototype as any).get = function() { return 7; }; const x: any = new Map(); console.log(x.get('a'));"),
        ("call", "const f = 'abc'.charCodeAt; console.log(f.call('abc', 0));"),
        ("apply", "const f = 'abc'.charCodeAt; console.log(f.apply('abc', [0]));"),
        ("charCodeAt", "const x: any = process.argv.length > 9 ? 'abc' : { charCodeAt(i: number) { return 7; } }; console.log(x.charCodeAt(0));"),
        ("log", "console.log = function(x: any) {}; console.log(7);"),
    ] {
        let (pf, _) = dispatch(src);
        let calls: Vec<_> = pf.sites.iter().filter(|s| s.is_call && s.key == key).collect();
        assert!(!calls.is_empty(), "missing {key}: {src}\n{}", pf.dump_text());
        for s in calls {
            assert!(s.dispatch_obligation().unwrap().redispatch, "{src}\n{}", pf.dump_text());
        }
    }
}

#[test]
fn native_dispatch_computed_and_spread_calls_have_no_direct_this_certificate() {
    for src in [
        "class P { m() { return 7; } run(k: string) { return (this as any)[k](); } } console.log(new P().run(process.argv[2]));",
        "class P { m(...a: number[]) { return a.length; } run() { return this.m(...[1, 2]); } } console.log(new P().run());",
    ] {
        let (pf, _) = dispatch(src);
        assert!(pf.sites.iter().filter(|s| s.is_call).all(|s| s.dispatch_obligation().unwrap().redispatch), "{}", pf.dump_text());
    }
}

#[test]
fn native_dispatch_push_has_one_record_and_the_js_key() {
    let (pf, _) = dispatch("class Bag { items: number[] = []; add() { return this.items.push(7); } } const b = new Bag(); console.log(b.add());");
    let pushes: Vec<_> = pf.member_ops.iter().filter(|o| o.key == "push").collect();
    assert_eq!(pushes.len(), 1, "{}", pf.dump_text());
    assert_eq!(pushes[0].op, "NativeMethodCall");
    assert!(!pushes[0].redispatch, "{}", pf.dump_text());
    assert!(!pf
        .member_ops
        .iter()
        .any(|o| o.key == "push_single" || o.key == "push_field_single"));
}

#[test]
fn native_dispatch_push_receiver_and_override_controls_remain_open() {
    for update in [
        "b.items = { push(x: number) { return 9; } } as any;",
        "(b.items as any).push = function(x: number) { return 9; };",
        "Object.setPrototypeOf(b.items, { push(x: number) { return 9; } });",
    ] {
        let (pf, _) = dispatch(&format!("class Bag {{ items: number[] = []; add() {{ return this.items.push(7); }} }} const b = new Bag(); {update} console.log(b.add());"));
        let pushes: Vec<_> = pf.member_ops.iter().filter(|o| o.key == "push").collect();
        assert_eq!(pushes.len(), 1, "{update}\n{}", pf.dump_text());
        assert!(pushes[0].redispatch, "{update}\n{}", pf.dump_text());
    }
}

#[test]
fn native_dispatch_fill_push_uses_the_fill_certificate() {
    let (pf, _) = dispatch("class T { private tab: number[] = []; constructor() { for (let i = 0; i < 16; i++) this.tab.push(i | 0); } get(k: number) { return this.tab[k & 15] | 0; } } console.log(new T().get(7));");
    let bindings = pf.bindings_for("main.ts").unwrap();
    assert!(
        !bindings.rest_pushes.is_empty(),
        "fill route must be live\n{}",
        pf.dump_text()
    );
    let pushes: Vec<_> = pf.member_ops.iter().filter(|o| o.key == "push").collect();
    assert_eq!(pushes.len(), 1, "{}", pf.dump_text());
    assert!(!pushes[0].redispatch, "{}", pf.dump_text());
}

#[test]
fn native_dispatch_check_mode_retains_witness_routes() {
    let src =
        "class P { m() { return 7; } run() { return this.m(); } } console.log(new P().run());";
    let path = "main.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).unwrap();
    let hir = perry_hir::lower_module(&module, "main", &path).unwrap();
    let cfg = FactsConfig {
        verify_facts: true,
        ..FactsConfig::default()
    };
    let pf = crate::compute(&[(path, &hir)], &cfg);
    let s = pf
        .sites
        .iter()
        .find(|s| s.is_call && s.key == "m")
        .expect("live call");
    assert!(s.dispatch_obligation().unwrap().redispatch);
}

#[test]
fn native_dispatch_refused_twin_clears_every_certificate() {
    let (pf, _) = dispatch(
        "class P { m() { return 7; } run() { return this.m(); } } console.log(new P().run());",
    );
    let proven = pf.sites.iter().find(|s| s.is_call && s.key == "m").unwrap();
    assert!(!proven.dispatch_obligation().unwrap().redispatch);
    let mut refused = proven.clone();
    refused.method_call = None;
    for mut twins in [
        vec![proven.clone(), refused.clone()],
        vec![refused, proven.clone()],
    ] {
        crate::derive::dispatch::agree(&mut twins);
        assert!(twins.iter().all(|s| s.method_call_certificate().is_none()));
        assert!(twins
            .iter()
            .all(|s| s.dispatch_obligation().unwrap().redispatch));
    }
}

// ERVJ: four routes the runtime runs without its generic dispatcher.

fn closed(src: &str) {
    let (pf, d) = dispatch(src);
    assert!(d.complete(), "{:?}\n{}", d.open, pf.dump_text());
}

#[test]
fn ervj_uncaught_native_error_report_is_no_route() {
    // node: prints `a` to stdout, then reports `Error: boom` and exits 1.
    closed("console.log('a');\nthrow new Error('boom');\n");
}

#[test]
fn ervj_regexp_split_converts_no_separator() {
    // node: prints `3 2`.
    closed(
        "const re = /,/g;\n\
         const r2 = new RegExp(re, 'g');\n\
         console.log('a,b,c'.split(re).length, 'x,y'.split(r2).length);\n",
    );
}

#[test]
fn ervj_performance_now_at_its_named_site_is_no_route() {
    // node: prints `number`.
    closed("const t0 = performance.now();\nconsole.log(typeof t0);\n");
}

#[test]
fn ervj_json_of_a_plain_record_is_no_route() {
    // node: prints `{"a":1,"b":"x","c":[true]}`.
    closed("console.log(JSON.stringify({ a: 1, b: 'x', c: [true] }));\n");
}

#[test]
fn ervj_program_now_and_program_to_json_are_closed() {
    // node: prints `1 {"v":2}`.
    closed(
        "const o = { now() { return 1; } };\n\
         const r = { toJSON() { return { v: 2 }; } };\n\
         console.log(o.now(), JSON.stringify(r));\n",
    );
}

#[test]
fn ervj_negative_controls_keep_the_fact_open() {
    // (what, src, O3 expected): every control stays open; a control that
    // installs a program function on a prototype reports the unknown
    // function value it calls (O1): open, any reason.
    let controls: &[(&str, &str, bool)] = &[
        // node: prints `Error: m`.
        (
            "plain string concat of an Error",
            "const e = new Error('m');\n\
             console.log('' + e);\n",
            true,
        ),
        // node: prints `T`.
        (
            "string concat of an Error with an overridden toString",
            "const e = new Error('m');\n\
             (Error.prototype as any).toString = function () { return 'T'; };\n\
             console.log('' + e);\n",
            false,
        ),
        // node: reports the thrown object, exits 1.
        (
            "uncaught program object with valueOf",
            "throw { valueOf() { return 1; } };\n",
            true,
        ),
        // node: prints `1`.
        (
            "patched RegExp.prototype[@@split] = undefined",
            "(RegExp.prototype as any)[Symbol.split] = undefined;\n\
             console.log('a,b'.split(/,/ as any).length);\n",
            true,
        ),
        // node: prints `x`.
        (
            "patched RegExp.prototype[@@split] = fn",
            "(RegExp.prototype as any)[Symbol.split] = function () { return ['x']; };\n\
             console.log('a,b'.split(/,/ as any)[0]);\n",
            false,
        ),
        // node: prints `2`.
        (
            "split separator object with toString",
            "console.log('a,b'.split({ toString() { return ','; } } as any).length);\n",
            true,
        ),
        // node: prints `1`.
        (
            "split limit object with valueOf",
            "console.log('a,b'.split(',', { valueOf() { return 1; } } as any).length);\n",
            true,
        ),
        // node: prints `1 number`.
        (
            "program now beside a borrowed performance.now",
            "const o = { now() { return 1; } };\n\
             const f = performance.now;\n\
             console.log(o.now(), typeof f.call(performance));\n",
            true,
        ),
        // node: prints `{"d":"1970-01-01T00:00:00.000Z"}`.
        (
            "Date in JSON",
            "console.log(JSON.stringify({ d: new Date(0) }));\n",
            true,
        ),
        // node: prints `{}`.
        (
            "toJSON accessor returning a native",
            "const o = { get toJSON() { return Object.prototype.valueOf; } };\n\
             console.log(JSON.stringify(o));\n",
            true,
        ),
    ];
    for (what, src, o3) in controls {
        let (pf, d) = dispatch(src);
        let why = d
            .open
            .unwrap_or_else(|| panic!("{what}: complete, expected open:\n{}", pf.dump_text()));
        assert!(!*o3 || why.starts_with("O3:"), "{what}: {why}");
        eprintln!("CONTROL {what}: {why}");
    }
}
