//! R9STORE2 fixtures: the console formatter and `toJSON`.
//!
//! Node lib/internal/util/inspect.js `formatWithOptions`: only a String
//! FIRST argument is a format string; `%j` in it serializes the argument
//! it consumes (JSON.stringify: 25.5.2.2 SerializeJSONProperty calls
//! `toJSON`); every other argument is shown by `inspect`, which never
//! calls `toJSON`.
use super::*;

/// Whether function `name`'s body can run (the activation pass's reach).
fn runs(src: &str, name: &str) -> bool {
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
        .find(|f| f.name == name)
        .expect(name)
        .id;
    pf.body_reach.bodies.iter().any(|(_, i, _)| *i == id)
}

const CLS: &str = "function hit( o: any ) { return { k: o.q }; }
class A { q: any; constructor() { this.q = Number( process.argv.length ); } toJSON() { return hit( this ); } }
const a = new A();
";

/// The three.js `log` shape: a rest logger, the front shifted off and
/// prefixed (a constant without `%`), the rest spread after it.
fn logger(call: &str) -> String {
    format!(
        "{CLS}function log( ...params: any[] ) {{ const message = 'T.' + params.shift(); console.log( message, ...params ); }}
{call}
"
    )
}

// C1 RED: `log( 'x', a )`: the format string is 'T.x' (no `%`): `a` is
// shown by inspect, `toJSON` never runs.
#[test]
fn r9store2_c1_rest_logger_const_front() {
    let on = runs(&logger("log( 'x', a ); log( 'y' );"), "hit");
    eprintln!("R9STORE2 C1 hit runs={on}");
    assert!(!on, "C1 toJSON body active");
}

// C2 KEEP: `console.log( '%j', a )`: `%j` serializes `a`: reached.
#[test]
fn r9store2_c2_pct_j_reaches() {
    let on = runs(&format!("{CLS}console.log( '%j', a );\n"), "hit");
    assert!(on, "C2 %j toJSON not reached");
}

// C3 KEEP: an unknown first argument may hold `%j`: reached.
#[test]
fn r9store2_c3_unknown_first_reaches() {
    let on = runs(
        &format!("{CLS}console.log( String( process.argv[2] ), a );\n"),
        "hit",
    );
    assert!(on, "C3 unknown format string: toJSON not reached");
}

// C4 KEEP: `console.log( a )`: inspect only: not reached.
#[test]
fn r9store2_c4_lone_object_not_reached() {
    let on = runs(&format!("{CLS}console.log( a );\n"), "hit");
    assert!(!on, "C4 lone argument reached toJSON");
}

// C6 RED: the rest logger with a later unknown String: only the FIRST
// argument is a format string (`formatWithOptions`), here the constant
// front `T.x`: the later String never makes `a` serialized. The call is
// fixed when it is made (`Gen::fixed_spreads`), before the console model
// reads it: before, the spread elements counted as a possible first.
#[test]
fn r9store2_c6_later_unknown_string_is_no_format() {
    let on = runs(
        &logger("log( \x27x\x27, a, String( process.argv[2] ) );"),
        "hit",
    );
    eprintln!("R9STORE2 C6 hit runs={on}");
    assert!(!on, "C6 toJSON body active");
}
