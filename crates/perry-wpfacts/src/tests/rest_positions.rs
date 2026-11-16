//! A rest parameter's array holds the call's arguments in order (ECMA-262
//! 10.2.11 FunctionDeclarationInstantiation with a BindingRestElement,
//! 8.6.3 IteratorBindingInitialization: CreateArrayFromList of the
//! remaining arguments): element k is argument n+k when no spread argument
//! comes before it. So `params[1]` reads that one argument only. (three.js
//! `error(msg, this, targetObject)` -> `enhanceLogMessage(params)` read every
//! argument of every call at `const stackTrace = params[1]`.) With a spread
//! argument the positions depend on the spread's length: those elements go
//! to every index. A write that does not name one fixed index (push, a
//! computed key, an escape) merges every position.

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

/// The three.js shape: `enhance(params)` receives the rest array itself,
/// reads `params[1]` and writes indices 0 and 1. `A` escapes iff it reaches
/// `st` (its `getLocation` leaks `this`; its guard field is unknown).
const HEAD: &str = "const unknown: any = new Function(process.argv[2])();
     class A { a = 1; isStackTrace: any = unknown.flag; getLocation() { unknown[1] = this; return 'a'; } }
     class S { s = 1; isStackTrace = true; getLocation() { return 's'; } }
     function enhance(params: any[]) {
       const message = params[0];
       if (typeof message === 'string' && message.startsWith('TSL:')) {
         const st = params[1];
         if (st && st.isStackTrace) { params[0] += ' ' + st.getLocation(); }
         else { params[1] = 'Stack trace not available.'; }
       }
       return params;
     }
     function error(...params: any[]) { enhance(params); }
     function log(msg: string, ...params: any[]) {
       const s = params[1];
       if (s && s.isStackTrace) s.getLocation();
     }
";

fn run(body: &str) -> ProgramFacts {
    whole(&format!("{HEAD}\n{body}"))
}

#[test]
fn rest_index_reads_only_its_argument() {
    let pf = run("error('TSL: m', new S(), new A()); error('TSL: n', 1);");
    assert_eq!(escape_of(&pf, "a"), None, "{}", pf.dump_text());
}

#[test]
fn rest_after_params_reads_only_its_argument() {
    let pf = run("log('m', new A(), new S());");
    assert_eq!(escape_of(&pf, "a"), None, "{}", pf.dump_text());
}

#[test]
fn rest_index_zero_reads_only_first() {
    let pf = run(
        "function z(...p: any[]) { const s = p[0]; if (s && s.isStackTrace) s.getLocation(); }
         z(new S(), new A());",
    );
    assert_eq!(escape_of(&pf, "a"), None, "{}", pf.dump_text());
}

#[test]
fn rest_fewer_arguments_read_nothing() {
    let pf = run("log('m', new A()); log('m', 1, new S());");
    assert_eq!(escape_of(&pf, "a"), None, "{}", pf.dump_text());
}

#[test]
fn rest_method_call_is_exact() {
    let pf = run(
        "class L { log(...p: any[]) { enhance(p); } }
         new L().log('TSL: m', new S(), new A());",
    );
    assert_eq!(escape_of(&pf, "a"), None, "{}", pf.dump_text());
}

#[test]
fn rest_construct_is_exact() {
    let pf = run(
        "class K { constructor(...p: any[]) { enhance(p); } }
         new K('TSL: m', new S(), new A());",
    );
    assert_eq!(escape_of(&pf, "a"), None, "{}", pf.dump_text());
}

#[test]
fn rest_in_summarized_caller_is_exact() {
    let pf = run(
        "function g(x: any) { error('TSL: m', new S(), x); return 0; }
         g(new A()); g(2);",
    );
    assert_eq!(escape_of(&pf, "a"), None, "{}", pf.dump_text());
}

/// Summary runs (two call sites): each run's rest array holds its own call's
/// arguments, each at its index.
const TWO: &str = "function two(...p: any[]) {
       const s = p[1];
       if (s && s.isStackTrace) s.getLocation();
       return p[0];
     }";

#[test]
fn rest_summary_runs_are_exact() {
    let pf = run(&format!("{TWO}\n unknown[3] = two(1, new S()); console.log(two(new A(), 2).a);"));
    assert_eq!(escape_of(&pf, "a"), None, "{}", pf.dump_text());
}

#[test]
fn control_rest_summary_run_holds_its_argument() {
    let pf = run(&format!("{TWO}\n unknown[3] = two(new A(), 2); two(1, new S());"));
    assert!(escape_of(&pf, "a").is_some(), "{}", pf.dump_text());
}

#[test]
fn control_rest_summary_run_reads_its_index() {
    let pf = run(&format!("{TWO}\n two(1, new A()); two(new S(), 2);"));
    assert!(escape_of(&pf, "a").is_some(), "{}", pf.dump_text());
}

// Controls: the position is not known, or the array was rearranged.

#[test]
fn control_rest_index_holds_its_argument() {
    let pf = run("error('TSL: m', new A());");
    assert!(escape_of(&pf, "a").is_some(), "{}", pf.dump_text());
}

#[test]
fn control_spread_forward_merges_positions() {
    let pf = run(
        "function warn(...p: any[]) { error(...p); }
         warn('TSL: m', 1, new A());",
    );
    assert!(escape_of(&pf, "a").is_some(), "{}", pf.dump_text());
}

#[test]
fn control_push_merges_positions() {
    let pf = run(
        "function e2(...params: any[]) { params.push(new A()); enhance(params); }
         e2('TSL: m');",
    );
    assert!(escape_of(&pf, "a").is_some(), "{}", pf.dump_text());
}

#[test]
fn control_for_of_sees_all() {
    let pf = run(
        "function e3(...params: any[]) { for (const p of params) { if (p && p.isStackTrace) p.getLocation(); } }
         e3('m', 1, new A());",
    );
    assert!(escape_of(&pf, "a").is_some(), "{}", pf.dump_text());
}

#[test]
fn control_escaped_rest_array_merges() {
    // Unknown code may put anything at index 1 of an escaped rest array.
    let pf = run(
        "function e4(...params: any[]) { unknown.keep = params; const x = params[1];
           if (x && x.isStackTrace) x.getLocation(); }
         e4('m', new S(), new A());",
    );
    assert!(escape_of(&pf, "a").is_some(), "{}", pf.dump_text());
}

#[test]
fn control_computed_index_reads_all() {
    let pf = run(
        "function e6(i: number, ...params: any[]) { const s = params[i]; if (s && s.isStackTrace) s.getLocation(); }
         e6(Number(process.argv[3]), 'm', new A());",
    );
    assert!(escape_of(&pf, "a").is_some(), "{}", pf.dump_text());
}

/// The key of `p[k] += v` held by a `const` binding of a numeric literal
/// is that literal: the store writes index 0 only, so `p[1]` holds `S`.
#[test]
fn const_literal_key_compound_writes_its_index() {
    let pf = run(
        "function c(i: number, ...p: any[]) { const k = 0; p[k] += 1; const s = p[1]; if (s && s.isStackTrace) s.getLocation(); }
         c(process.argv.length, new A(), new S());",
    );
    assert_eq!(escape_of(&pf, "a"), None, "{}", pf.dump_text());
}

/// Control: a key binding that is reassigned names no index; its store
/// merges every position, so `p[1]` may hold `A`.
#[test]
fn reassigned_key_compound_merges_positions() {
    let pf = run(
        "function c(i: number, ...p: any[]) { let k = 0; k = i; p[k] += 1; const s = p[1]; if (s && s.isStackTrace) s.getLocation(); }
         c(process.argv.length, new A(), new S());",
    );
    assert!(escape_of(&pf, "a").is_some(), "{}", pf.dump_text());
}
