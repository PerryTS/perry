//! A rest parameter is a fresh array each call creates, holding that
//! call's arguments after the parameters (ECMA-262 10.2.11
//! FunctionDeclarationInstantiation with a BindingRestElement, 8.6.3
//! IteratorBindingInitialization: ArrayCreate on every [[Call]]). So
//! `p[0]` of one call never reads another call's argument. (three.js
//! `error(...params)`: one rest array for its 34 call sites made
//! `params[0]` every argument of every call.) A function with a rest
//! parameter is summarized like any other: each run births its own array.

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
     class A { a = 1; }
     class B { b = 1; }
     class C { c = 1; }
     function f(...p: any[]) { return p[0]; }
     function s(...p: any[]) { p[0].z = new C(); return 0; }
";

#[test]
fn each_call_site_reads_only_its_own_rest_argument() {
    let pf = whole(&format!(
        "{HEAD}
         const x = f(new A());
         const y = f(new B());
         unknown[0] = x;
         console.log(y.b);"
    ));
    assert!(escape_of(&pf, "a").is_some(), "{}", pf.dump_text());
    assert_eq!(escape_of(&pf, "b"), None, "{}", pf.dump_text());
}

/// The calls sit in summarized callers: composition gives each caller's
/// run its own rest array too.
#[test]
fn composed_calls_read_only_their_own_rest_argument() {
    let pf = whole(&format!(
        "{HEAD}
         function g1() {{ return f(new A()); }}
         function g2() {{ return f(new B()); }}
         unknown[0] = g1();
         console.log(g2().b);"
    ));
    assert!(escape_of(&pf, "a").is_some(), "{}", pf.dump_text());
    assert_eq!(escape_of(&pf, "b"), None, "{}", pf.dump_text());
}

/// Control: no program call stores into `p[0]` here; unknown code calls
/// `s` with its own arguments, so `p[0]` is unknown and `C` escapes.
#[test]
fn a_function_handed_to_unknown_code_takes_unknown_rest_elements() {
    let pf = whole(&format!("{HEAD}\n unknown(s); console.log(new A().a, new B().b);"));
    assert!(escape_of(&pf, "c").is_some(), "{}", pf.dump_text());
}

/// Control: spread elements of an unknown array are the rest elements.
#[test]
fn an_unknown_spread_fills_the_rest_array() {
    let pf = whole(&format!(
        "{HEAD}\n const l: any[] = unknown.list; s(...l); console.log(new A().a, new B().b);"
    ));
    assert!(escape_of(&pf, "c").is_some(), "{}", pf.dump_text());
}
