//! R9SITES3 fixtures.
//!
//! FK: a for-in over a parameter (`set( values ) { for ( const key in values ) … }`)
//! iterated with object literals only binds their enumerable String keys
//! (ECMA-262 14.7.5.6 ForIn/OfBodyEvaluation, 14.7.5.9
//! EnumerateObjectProperties); `undefined` runs no iteration (14.7.5.6 step
//! 6.a). A message `'x ' + key` is then a finite set of constants without
//! `%`, so the formatter (Node util.format) consumes no later argument and
//! `%j` never serializes it: `toJSON` never runs.
use super::*;

/// The receivers of the reads of `key` (no calls), `proto:` kept.
fn recv_p(pf: &ProgramFacts, key: &str) -> Vec<String> {
    let mut r: Vec<String> = pf
        .sites
        .iter()
        .filter(|s| s.key == key && !s.is_call && !s.access.is_store())
        .flat_map(|s| s.receivers.iter())
        .map(|r| {
            let last = r.rsplit(':').next().unwrap_or(r).to_string();
            if r.starts_with("proto:") {
                format!("proto:{last}")
            } else {
                last
            }
        })
        .collect();
    for s in pf
        .sites
        .iter()
        .filter(|s| s.key == key && !s.access.is_store())
    {
        eprintln!(
            "R9SITES3 site {} call={} verdict={:?} recv={:?}",
            s.byte_offset, s.is_call, s.verdict, s.receivers
        );
    }
    r.sort();
    r.dedup();
    r
}

const FK: &str = "class A { tj: any; constructor() { this.tj = { q: Number( process.argv.length ) }; } toJSON() { return this.tj; } }
const a = new A();
class M { a: any; b: any; type: string; constructor( p?: any ) { this.a = 1; this.b = 2; this.type = 'M'; this.set( p ); }
  set( values: any ) { for ( const key in values ) { const nv = values[ key ]; if ( nv === undefined ) { console.warn( 'u ' + key, a ); continue; } const cur = this[ key ]; if ( cur === undefined ) { console.warn( `x '${ key }' of ${ this.type }`, a ); continue; } this[ key ] = nv; } } }
new M( { a: 3 } );
new M( { b: 4, c: 5 } );
new M();
";

// FK1 RED: the keys are the literals' keys: no message holds `%`, toJSON never runs.
#[test]
fn r9sites3_fk1_forin_param_keys_are_literal_keys() {
    let pf = facts(&format!("{FK}console.log( a instanceof A );\n"));
    let r = recv_p(&pf, "tj");
    eprintln!("R9SITES3 FK1 tj recv={r:?}");
    assert!(r.is_empty(), "FK1 toJSON runs: {r:?}");
}

// FK2 KEEP: an object of unknown keys (JSON.parse of the input) binds any
// String key: `'u ' + key` may hold `%j`, so `a` may be serialized.
#[test]
fn r9sites3_fk2_keep_unknown_object_any_key() {
    let pf = facts(&format!("{FK}new M( JSON.parse( process.argv[2] ) );\n"));
    let r = recv_p(&pf, "tj");
    eprintln!("R9SITES3 FK2 tj recv={r:?}");
    assert!(r.contains(&"A".to_string()), "FK2 toJSON lost: {r:?}");
}

// FK3 RED, the shape of a rest-parameter logger that prefixes the first
// argument; every caller passes a constant set without `%`.
#[test]
fn r9sites3_fk3_rest_logger_constant_messages() {
    let src = format!(
        "function w( ...p: any[] ) {{ const m = 'T.' + p.shift(); console.warn( m, ...p ); }}
{FK}
class N {{ a: any; constructor( p?: any ) {{ this.a = 1; this.set( p ); }} set( values: any ) {{ for ( const key in values ) {{ if ( this[ key ] === undefined ) {{ w( `x '${{ key }}'` ); continue; }} this[ key ] = values[ key ]; }} }} }}
new N( {{ a: 3 }} ); new N( {{ z: 1 }} ); new N();
w( 'y', a );
"
    );
    let pf = facts(&src);
    let r = recv_p(&pf, "tj");
    eprintln!("R9SITES3 FK3 tj recv={r:?}");
    assert!(r.is_empty(), "FK3 toJSON runs: {r:?}");
}

// R9SITES4 (the r9b / r9a / r6 totals): with the old any-String template rule
// a message `T.${ x }` could hold `%j`, so the formatter serialized the later
// arguments and every `toJSON` was reached (three.js `warn` / `error`: the
// Object3D / Material / BufferGeometry `toJSON` bodies, ~790 sites of r9b that
// node never runs). The template is the Strings of `"" + x` (13.15.3
// ApplyStringOrNumericBinaryOperator, 7.1.17 ToString, 7.1.1 ToPrimitive):
// it is any String again where user code chooses the text.
const TJ: &str = "class A { tj: any; constructor() { this.tj = { q: Number( process.argv.length ) }; } toJSON() { return this.tj; } }\nconst a = new A();\n";

// FK4 KEEP: the operand is an object whose `toString` returns the input: the
// message may hold `%j` (7.1.1 ToPrimitive hint string calls user code).
#[test]
fn r9sites4_fk4_keep_template_object_tostring() {
    let pf = facts(&format!("{TJ}class S {{ toString() {{ return process.argv[2]; }} }}\nconsole.warn( `T.${{ new S() }}`, a );\n"));
    let r = recv_p(&pf, "tj");
    eprintln!("R9SITES4 FK4 tj recv={r:?}");
    assert!(r.contains(&"A".to_string()), "FK4 toJSON lost: {r:?}");
}

// FK5 KEEP: `Symbol.toPrimitive` returns the input (7.1.1 step 2.b).
#[test]
fn r9sites4_fk5_keep_template_object_to_primitive() {
    let pf = facts(&format!("{TJ}const o = {{ [Symbol.toPrimitive]( h: string ) {{ return process.argv[2]; }} }};\nconsole.warn( `T.${{ o }}`, a );\n"));
    let r = recv_p(&pf, "tj");
    eprintln!("R9SITES4 FK5 tj recv={r:?}");
    assert!(r.contains(&"A".to_string()), "FK5 toJSON lost: {r:?}");
}

// FK6 KEEP: the operand is the input String itself.
#[test]
fn r9sites4_fk6_keep_template_input_string() {
    let pf = facts(&format!("{TJ}const s = String( Math.random() ) + process.argv[2];\nconsole.warn( `T.${{ s }}`, a );\n"));
    let r = recv_p(&pf, "tj");
    eprintln!("R9SITES4 FK6 tj recv={r:?}");
    assert!(r.contains(&"A".to_string()), "FK6 toJSON lost: {r:?}");
}

// FK7: a Number operand: Number::toString (6.1.6.1.20) never yields `%`:
// the formatter consumes no later argument and toJSON never runs.
#[test]
fn r9sites4_fk7_template_number_no_pct() {
    let pf = facts(&format!(
        "{TJ}const n = Number( process.argv[2] );\nconsole.warn( `T.${{ n }}`, a );\n"
    ));
    let r = recv_p(&pf, "tj");
    eprintln!("R9SITES4 FK7 tj recv={r:?}");
    assert!(r.is_empty(), "FK7 toJSON runs: {r:?}");
}
