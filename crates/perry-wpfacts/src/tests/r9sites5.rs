//! R9SITES5 fixtures: RULE RP (`gen/ret_param.rs`, `Model::ret_param_into`).
//!
//! A function whose every completion returns one unreassigned plain
//! parameter returns, at each direct call, the argument that call passes
//! (ECMA-262 10.2.1.3, 10.2.11): two loggers that pass their own rest
//! arrays through one such helper each keep their own array, so each
//! `shift()` reads its own front and the messages stay constants without
//! `%` (Node util.format consumes no later argument): `toJSON` never runs.
use super::*;

/// The receivers of the reads of `key` (no calls, no stores), last name part.
fn recv(pf: &ProgramFacts, key: &str) -> Vec<String> {
    let mut r: Vec<String> = pf
        .sites
        .iter()
        .filter(|s| s.key == key && !s.is_call && !s.access.is_store())
        .flat_map(|s| s.receivers.iter())
        .map(|r| r.rsplit(':').next().unwrap_or(r).to_string())
        .collect();
    r.sort();
    r.dedup();
    r
}

const HEAD: &str = "class A { tj: any; constructor() { this.tj = { q: Number( process.argv.length ) }; } toJSON() { return this.tj; } }
const a = new A();
";

/// The two loggers through helper `enh` (defined by the caller).
fn loggers(enh: &str) -> String {
    format!(
        "{HEAD}{enh}
function w( ...ps: any[] ) {{ ps = enh( ps ); const m = 'T.' + ps.shift(); console.warn( m, ...ps ); }}
function e( ...ps: any[] ) {{ ps = enh( ps ); const m = 'T.' + ps.shift(); console.error( m, ...ps ); }}
w( 'x', a ); e( 'y', [ a ] ); w( 'z' );
"
    )
}

const ENH: &str = "function enh( p: any ) { const m = p[ 0 ]; if ( typeof m === 'string' && m.startsWith( 'TSL:' ) ) { p[ 1 ] = 'S'; } return p; }";

// RP1 RED: each logger's array comes back to it: the fronts are its own
// constants, no message holds `%`, toJSON never runs.
#[test]
fn r9sites5_rp1_identity_helper_keeps_each_rest_array() {
    let pf = facts(&loggers(ENH));
    let r = recv(&pf, "tj");
    eprintln!("R9SITES5 RP1 tj recv={r:?}");
    assert!(r.is_empty(), "RP1 toJSON runs: {r:?}");
}

// RP2 guard: the same loggers with a helper that reassigns its parameter
// (`if ( ! p ) p = [ a ]`): its result is the shared return, each shift
// may read the other array's object, `%j` is possible: toJSON stays reached.
#[test]
fn r9sites5_rp2_keep_reassigned_param() {
    let pf = facts(&loggers(
        "function enh( p: any ) { if ( ! p ) p = [ a ]; return p; }",
    ));
    let r = recv(&pf, "tj");
    eprintln!("R9SITES5 RP2 tj recv={r:?}");
    assert!(
        r.contains(&"A".to_string()),
        "RP2 lost the shared return: {r:?}"
    );
}

// RP3 KEEP: a helper that uses `arguments` (10.4.4: a mapped object aliases
// the parameters): out, shared return.
#[test]
fn r9sites5_rp3_keep_arguments_object() {
    let pf = facts(&loggers(
        "function enh( p: any ) { if ( arguments.length > 3 ) console.log( 'n' ); return p; }",
    ));
    let r = recv(&pf, "tj");
    eprintln!("R9SITES5 RP3 tj recv={r:?}");
    assert!(
        r.contains(&"A".to_string()),
        "RP3 lost the shared return: {r:?}"
    );
}

const BC: &str = "class B { tj: any; constructor() { this.tj = 1; } }
class C { tj: any; constructor() { this.tj = 2; } }
const b = new B(); const c = new C();
";

// RP4 KEEP: a defaulted parameter is out: `undefined` there binds the default.
#[test]
fn r9sites5_rp4_keep_default_param() {
    let pf = facts(&format!(
        "{HEAD}{BC}function g( p: any = c ) {{ return p; }}
console.log( g( undefined ).tj, g( b ) === b );
"
    ));
    let r = recv(&pf, "tj");
    eprintln!("R9SITES5 RP4 tj recv={r:?}");
    assert!(r.contains(&"C".to_string()), "RP4 lost the default: {r:?}");
}

// RP5 RED + KEEP: fewer arguments than the position: the result is
// `undefined` (10.2.11 step 26), not the other call's argument.
#[test]
fn r9sites5_rp5_missing_argument_is_undefined() {
    let pf = facts(&format!(
        "{HEAD}{BC}function g( p: any, q?: any ) {{ return q; }}
const r1 = g( a, b ); const r2 = g( a );
console.log( ( r2 ?? c ).tj, r1 === b );
"
    ));
    let r = recv(&pf, "tj");
    eprintln!("R9SITES5 RP5 tj recv={r:?}");
    assert_eq!(
        r,
        vec!["C".to_string()],
        "RP5: the result holds another call's argument"
    );
}

// RP6 KEEP: a `finally` that returns another value: out (its completion
// replaces the try's return, 14.15.3).
#[test]
fn r9sites5_rp6_keep_finally_return() {
    let pf = facts(&format!(
        "{HEAD}{BC}function g( p: any ) {{ try {{ if ( process.argv.length > 1 ) return p; }} finally {{ if ( process.argv.length > 7 ) return b; }} return p; }}
console.log( g( c ).tj );
"
    ));
    let r = recv(&pf, "tj");
    eprintln!("R9SITES5 RP6 tj recv={r:?}");
    assert!(
        r.contains(&"B".to_string()) && r.contains(&"C".to_string()),
        "RP6: {r:?}"
    );
}

// RP7 KEEP: async and generator functions return their promise / generator,
// never the parameter itself.
#[test]
fn r9sites5_rp7_keep_async_and_generator() {
    let pf = facts(&format!(
        "{HEAD}{BC}async function g( p: any ) {{ return p; }}
function* h( p: any ) {{ return p; }}
function* h2( p: any ) {{ const q = p; return q; }}
const x: any = g( b ); const y: any = h( c ); const z: any = h2( a );
console.log( x.tj, y.tj, z.tj );
"
    ));
    let r = recv(&pf, "tj");
    eprintln!("R9SITES5 RP7 tj recv={r:?}");
    assert!(
        !r.contains(&"B".to_string()),
        "RP7 async result is the parameter: {r:?}"
    );
    // The generator object reads as its return value in the model whether or
    // not the function is RP-shaped (`h2` returns a local): RP adds nothing.
    assert_eq!(
        r.contains(&"C".to_string()),
        r.contains(&"A".to_string()),
        "RP7 generator: {r:?}"
    );
}

// RP8 KEEP: a callee set of two functions: each binding gives its own result.
#[test]
fn r9sites5_rp8_keep_union_callee() {
    let pf = facts(&format!(
        "{HEAD}{BC}function id( p: any ) {{ return p; }}
function other( p: any ) {{ return c; }}
const fs = [ id, other ];
const f = fs[ Number( process.argv.length ) % 2 ];
console.log( f( b ).tj );
"
    ));
    let r = recv(&pf, "tj");
    eprintln!("R9SITES5 RP8 tj recv={r:?}");
    assert!(
        r.contains(&"B".to_string()) && r.contains(&"C".to_string()),
        "RP8: {r:?}"
    );
}

// RP9 KEEP: `new` of such a function: the result is the new object, the
// object argument (10.2.2 step 10) or both: nothing lost.
#[test]
fn r9sites5_rp9_keep_new() {
    let pf = facts(&format!(
        "{HEAD}{BC}function F( this: any, p: any ) {{ return p; }}
const o: any = new ( F as any )( b );
console.log( o.tj );
"
    ));
    let r = recv(&pf, "tj");
    eprintln!("R9SITES5 RP9 tj recv={r:?}");
    assert!(
        r.contains(&"B".to_string()),
        "RP9 lost the returned object: {r:?}"
    );
}

// RP10 KEEP: the parameter written by a closure of the function: out.
#[test]
fn r9sites5_rp10_keep_closure_write() {
    let pf = facts(&format!(
        "{HEAD}{BC}function g( p: any ) {{ const k = () => {{ p = c; }}; if ( process.argv.length > 5 ) k(); return p; }}
console.log( g( b ).tj );
"
    ));
    let r = recv(&pf, "tj");
    eprintln!("R9SITES5 RP10 tj recv={r:?}");
    assert!(
        r.contains(&"B".to_string()) && r.contains(&"C".to_string()),
        "RP10: {r:?}"
    );
}

// RP11 RED, the summarized shape: loggers with a hook branch and a
// stack-trace branch (summarized callers expand the helper's body,
// `summary/compose.rs` `expand`): each still gets its own array back.
#[test]
fn r9sites5_rp11_composed_identity_helper() {
    let pf = facts(&format!(
        "{HEAD}{ENH}
let hook: any = null;
if ( process.argv.length > 9 ) hook = ( ...q: any[] ) => q.length;
function w( ...ps: any[] ) {{ ps = enh( ps ); const m = 'T.' + ps.shift(); if ( hook ) {{ hook( 'warn', m, ...ps ); }} else {{ const st = ps[ 0 ]; if ( st && st.isStackTrace ) {{ console.warn( st.getError( m ) ); }} else {{ console.warn( m, ...ps ); }} }} }}
function e( ...ps: any[] ) {{ ps = enh( ps ); const m = 'T.' + ps.shift(); if ( hook ) {{ hook( 'error', m, ...ps ); }} else {{ const st = ps[ 0 ]; if ( st && st.isStackTrace ) {{ console.error( st.getError( m ) ); }} else {{ console.error( m, ...ps ); }} }} }}
class P {{ bind( t: any ) {{ if ( ! t ) {{ e( 'P: no node', this, [ a ] ); return; }} w( 'P: ok' ); }} }}
new P().bind( process.argv.length > 3 ? null : a ); w( 'x', a ); e( 'y', [ a ] );
"
    ));
    let r = recv(&pf, "tj");
    eprintln!("R9SITES5 RP11 tj recv={r:?}");
    assert!(r.is_empty(), "RP11 toJSON runs: {r:?}");
}
