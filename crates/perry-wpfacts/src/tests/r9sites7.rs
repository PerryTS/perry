//! R9SITES7 fixtures: rule NUM-KEY-PRIM.
//!
//! `p[k]` of a primitive `p` (ECMA-262 7.3.2 GetV) under a key that holds
//! only Numbers is `p[ToString(k)]` (7.1.19 ToPropertyKey, 6.1.6.1.20
//! Number::toString): a numeric name, which no member of an intrinsic
//! prototype has. A String's index reads one of its characters or
//! `undefined`; only a number key a store wrote on the prototype is more.
//! The three.js shape: `_getValue_array`'s `source[ i ]` with `source` a
//! property that may be a String.
use super::*;

/// The receivers of the reads of `key`, short names.
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

fn program(key: &str, pre: &str) -> String {
    format!(
        "class V {{ x: number; constructor() {{ this.x = Number( process.argv.length ); }} }}
{pre}
const src: any = process.argv[ 3 ] === 'v' ? new V() : String( Math.random() );
const out: any[] = [];
for ( let i = 0, n = src.length; i !== n; ++ i ) {{ out.push( src[ {key} ] ); }}
const r: any = out[ 0 ];
console.log( r.zz );
"
    )
}

fn builtin_fns(r: &[String]) -> Vec<&String> {
    r.iter().filter(|x| x.contains(".prototype.")).collect()
}

// N1 RED: the key is a Number: a String's characters or `undefined`, never
// a String.prototype member.
#[test]
fn r9sites7_n1_number_key_on_string_reads_no_member() {
    let pf = facts(&program("i", ""));
    let r = recv(&pf, "zz");
    eprintln!("R9SITES7 N1 zz recv={r:?}");
    assert!(builtin_fns(&r).is_empty(), "N1 member read under a Number key: {r:?}");
}

// N2 KEEP: the key is unknown (`process.argv[2]`): it may spell `at`.
#[test]
fn r9sites7_n2_keep_string_key() {
    let pf = facts(&program("process.argv[ 2 ]", ""));
    let r = recv(&pf, "zz");
    eprintln!("R9SITES7 N2 zz recv={r:?}");
    assert!(r.iter().any(|x| x == "String.prototype.at"), "N2 lost a member: {r:?}");
}

// N3 KEEP: a Number key the program wrote on String.prototype reads it.
#[test]
fn r9sites7_n3_keep_written_number_key() {
    let pre = "function hook() { return 1; }\n( String.prototype as any )[ 7 ] = hook;";
    let pf = facts(&program("i", pre));
    let r = recv(&pf, "zz");
    eprintln!("R9SITES7 N3 zz recv={r:?}");
    assert!(r.iter().any(|x| x.contains("hook")), "N3 lost the written member: {r:?}");
}

// N4 KEEP: a key that is a Number or a String reads the members.
#[test]
fn r9sites7_n4_keep_mixed_key() {
    let pf = facts(&program("( process.argv.length > 9 ? i : process.argv[ 2 ] )", ""));
    let r = recv(&pf, "zz");
    eprintln!("R9SITES7 N4 zz recv={r:?}");
    assert!(r.iter().any(|x| x == "String.prototype.at"), "N4 lost a member: {r:?}");
}
