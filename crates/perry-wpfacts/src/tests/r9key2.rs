//! R9KEY2 fixtures: a computed key that is a FINITE set of String constants
//! (r9: three.core.js:53606 `targetObject[ objectName ]` in
//! `PropertyBinding.bind`, objectName = 40 substrings of the three track
//! names in the activation pass) reads each constant by name: ECMA-262
//! 7.1.19 ToPropertyKey of a String is itself, 10.1.8.1 OrdinaryGet walks
//! the chain, so an absent constant gives exactly `undefined`. "" and the
//! canonical numeric "1" are named keys on an ordinary object; on an
//! Array (10.4.2) the numeric one is the element, on a String (10.4.3) a
//! one-character String. An unknown key (`process.argv[2]`) stays any key.
use super::key2::recv;
use super::*;

const OBJ: &str = "class P { p = 1; }
class Q { p = 2; }
class O { position: any = new P(); other: any = new Q(); uuid: any = String( Math.random() ); }
";

/// `k` = one of `keys`, chosen by `Math.random()` (nested conditionals).
fn pick(keys: &[&str]) -> String {
    let n = keys.len();
    let mut k = format!("{:?}", keys[n - 1]);
    for i in (0..n - 1).rev() {
        k = format!(
            "( r < {} ? {:?} : {k} )",
            (i + 1) as f64 / n as f64,
            keys[i]
        );
    }
    k
}

fn read(recv_src: &str, keys: &str) -> Vec<String> {
    let pf = facts(&format!(
        "{OBJ}const r = Math.random();\nconst k: any = {keys};\nconst o: any = {recv_src};\nconst x: any = o[ k ];\nif ( x !== undefined && x !== null ) {{ console.log( x.p ); }}\n"
    ));
    let r = recv(&pf, "p");
    eprintln!(
        "R9KEY2 recv={recv_src} keys={} -> {r:?}",
        &keys[..keys.len().min(80)]
    );
    r
}

fn exact(r: &[String]) -> bool {
    r.contains(&"P".to_string()) && !r.iter().any(|x| x == "Q" || x.contains("String"))
}

// B1: a numeric-looking constant beside a member name on an ordinary object.
#[test]
fn r9key2_b1_numeric_string_key_named() {
    let r = read("new O()", &pick(&["1", "position"]));
    assert!(exact(&r), "B1 {{\"1\", \"position\"}}: {r:?}");
}

// B2: the empty String beside a member name.
#[test]
fn r9key2_b2_empty_string_key_named() {
    let r = read("new O()", &pick(&["", "position"]));
    assert!(exact(&r), "B2 {{\"\", \"position\"}}: {r:?}");
}

/// r9's objectName in the activation pass (R9KEY1): 40 constants, one present.
const R9_KEYS: [&str; 40] = [
    "material",
    "",
    "1",
    "2",
    "e1",
    "e2",
    "i",
    "one1",
    "ne1",
    "one2",
    "ne2",
    "kinned",
    "inned",
    "nned",
    "ned",
    "ed",
    "d",
    "kinned.material",
    "inned.material",
    ".material",
    "k",
    "ki",
    "kin",
    "kinn",
    "kinne",
    "s",
    "b",
    "bo",
    "bon",
    "bone",
    "bone1",
    "bone2",
    "sk",
    "ski",
    "skin",
    "skinn",
    "skinne",
    "skinned",
    "skinned.material",
    "position",
];

// B3: a set of 40 constants is 40 named reads (no size threshold).
#[test]
fn r9key2_b3_forty_constants_named() {
    let r = read("new O()", &pick(&R9_KEYS));
    assert!(exact(&r), "B3 40 constants: {r:?}");
}

// B4 KEEP: on an Array "1" is the element (10.4.2), "position" absent.
#[test]
fn r9key2_b4_array_receiver_element() {
    let r = read("[ new P(), new P() ]", &pick(&["1", "position"]));
    assert!(
        r.contains(&"P".to_string()) && !r.contains(&"Q".to_string()),
        "B4 array: {r:?}"
    );
}

// B5 KEEP: on a String "1" is a one-character String or undefined, "length"
// a Number (10.4.3): never an object.
#[test]
fn r9key2_b5_string_receiver_index() {
    let r = read("String( Math.random() )", &pick(&["1", "length"]));
    assert!(!r.iter().any(|x| x == "P" || x == "Q"), "B5 string: {r:?}");
}

// B6 KEEP: an unknown key reads every member.
#[test]
fn r9key2_b6_unknown_key_stays_any() {
    let r = read("new O()", "String( process.argv[ 2 ] )");
    assert!(
        r.contains(&"P".to_string()) && r.contains(&"Q".to_string()),
        "B6 argv: {r:?}"
    );
}
