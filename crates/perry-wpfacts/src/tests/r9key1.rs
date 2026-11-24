//! R9KEY1 fixtures: the losing hop of the real r9 solve (three tracks
//! "bone1.position", "bone2.quaternion", "skinned.material.opacity"). The
//! activation pass runs `parseTrackName` (three.core.js:53210-53250) once
//! for all three names, and its `results` object is one allocation whose
//! `nodeName` field holds the matched names and the rewritten
//! `results.nodeName.substring( 0, lastDot )` (:53240). `lastIndexOf( '.' )`
//! (:53228) of {"bone1", "bone2", "skinned.material", ...} is the interval
//! [-1, 7], `lastDot !== -1` (:53230) leaves [0, 7], so the rewrite adds
//! "" (`substring( 0, 0 )`) to `nodeName`; `"" && ...` (:53228) delivers ""
//! itself to `lastDot`, and `results.nodeName.substring( lastDot + 1 )`
//! (:53232) / `substring( 0, lastDot )` (:53240) got a String operand:
//! any String (KEY2b took only Numbers and `undefined`). `objectName` was
//! any, `bind` (:53606) read `targetObject[ objectName ]` under any key
//! (the uuid String), `_getValue_direct` (:53334) read the String chain
//! under any key: `Builtin(String)`, the realm hold. The "any string from
//! mergeUniforms" in the hold's chain is only the shared
//! `builtin-chain-any` node, first minted there.
//! Rule: 22.1.3.25 step 4/5 ToIntegerOrInfinity (7.1.5) of a String
//! operand is ToNumber (7.1.4.1.1 StringToNumber) of each constant.
//! F1 three tracks through one binding class: activation configuration
//! not held; whole pipeline `.y` reads V, U, W only. F2 the folds of
//! String operands. K1 KEEP unknown String operand stays any. K2 KEEP a
//! set of two keys, one absent: the present member only.

use super::key2::{recv, v, PARSE_BIND, TRACK_RE};
use super::*;

fn held_unsummarized(src: &str) -> bool {
    let ast = Box::leak(Box::new(
        perry_parser::parse_typescript(src, "t.ts").unwrap(),
    ));
    let hir = Box::leak(Box::new(perry_hir::lower_module(ast, "t", "t.ts").unwrap()));
    let mut g = crate::gen::Gen::new(vec![("t.ts", &*hir)], false, false);
    g.m.max_work = 50_000_000;
    g.register_all();
    g.link_exports();
    g.walk_all();
    g.feed_parent_values();
    for f in g.m.funcs.iter_mut() {
        f.no_summary = true;
    }
    g.m.summarize_and_replay();
    g.m.solve();
    g.m.realm_held
}

/// r9's three tracks through one binding class (three.core.js
/// PropertyBinding: constructor :53140, bind :53489-53627,
/// `_getValue_direct` :53334), on an object whose fields hold a String.
fn tracks(names: &[&str]) -> String {
    let news: Vec<String> = names
        .iter()
        .map(|n| format!("new Binding( '{n}' )"))
        .collect();
    format!(
        r#"class V {{ x = 1; y = 1; }}
class U {{ x = 2; y = 2; }}
class W {{ x = 3; y = 3; }}
class Mat {{ opacity: any = new W(); }}
class Obj {{ name: any = 'obj'; uuid: any = String( Math.random() ); position: any = new V(); quaternion: any = new U(); material: any = new Mat(); other(): number {{ return 1; }} }}
{TRACK_RE}
{PARSE_BIND}
const o: any = new Obj();
class Binding {{
	path: any; targetObject: any; propertyName: any;
	constructor( n: any ) {{ this.path = parseTrackName( n ); this.targetObject = bindT( o, this.path ); this.propertyName = this.path.propertyName; }}
	getValue( buffer: any, offset: number ) {{ buffer[ offset ] = this.targetObject[ this.propertyName ]; }}
}}
const bs = [ {} ];
const buf: any[] = [ null ];
let s = 0;
for ( const b of bs ) {{ b.getValue( buf, 0 ); const x: any = buf[ 0 ]; if ( x !== undefined ) {{ s += x.y; x.toString(); }} }}
console.log( s );
"#,
        news.join(", ")
    )
}

const R9: [&str; 3] = ["obj.position", "obj.quaternion", "obj.material.opacity"];

// F1a (KEEP, green before: this shape has no unknown callee): the activation configuration runs parseTrackName once
// for the three names; the realm stays free.
#[test]
fn r9key1_f1a_three_tracks_activation_not_held() {
    assert!(!held_unsummarized(&tracks(&R9)), "F1a: realm held");
}

// F1b (red before: every builtin chain member): the whole pipeline: `x.y`
// reads the three targets' values only (and the honest `null` of the
// buffer's first value; the `undefined` of an absent key fails the
// `x !== undefined` test that guards the read, top level as in a function).
#[test]
fn r9key1_f1b_three_tracks_read_named_members() {
    let pf = facts(&tracks(&R9));
    let y = recv(&pf, "y");
    eprintln!("R9KEY1 F1b y={y:?}");
    assert_eq!(
        y,
        v(&["U", "V", "W", "null (TypeError)"])
    );
}

fn is_const(want: &str, e: &str) -> Vec<String> {
    let pf = facts(&format!(
        r#"class A {{ p = 1; }}
class B {{ p = 2; }}
const d: any = {{ "{want}": new A(), other: new B() }};
const key: any = {e};
console.log( d[ key ].p );
"#
    ));
    recv(&pf, "p")
}

// F2 (red before): ToIntegerOrInfinity of String operands (7.1.4.1.1
// StringToNumber: whitespace trimmed, "" is 0, hex, Infinity, NaN is 0).
#[test]
fn r9key1_f2_string_operand_fold() {
    let mut bad = Vec::new();
    for (want, e) in [
        ("bc", "'abc'.substring( '1' )"),
        ("abc", "'abc'.substring( '' )"),
        ("ab", "'abc'.substring( 0, ' 2 ' )"),
        ("abc", "'abc'.substring( 'x' )"),
        ("c", "'abc'.substring( '0x2' )"),
        ("bc", "'abc'.substring( '1.9' )"),
        ("abc", "'abc'.substring( '-Infinity', '1e1' )"),
        ("a", "'abc'.substring( 1, '-0' )"),
        ("abc", "'abc'.substring( '1_0', '3' )"),
        ("c", "'abc'.substring( '' + 2 )"),
        (
            "c",
            "'abc'.substring( ( Math.random() < 0.5 ? '' : 0 ) + 2 )",
        ),
        ("ab", "'abc'.substring( 0, 2 + '' )"),
    ] {
        let r = is_const(want, e);
        eprintln!("R9KEY1 F2 {e} -> {r:?}");
        if r != v(&["A"]) {
            bad.push(format!("{e}: {r:?}"));
        }
    }
    assert!(bad.is_empty(), "not folded: {bad:?}");
}

// K1 KEEP: an unknown String operand stays any String.
#[test]
fn r9key1_k1_unknown_string_operand_stays_any() {
    for e in [
        "'abc'.substring( String( process.argv[ 2 ] ) )",
        "'abc'.substring( 0, String( Math.random() ) )",
    ] {
        let r = is_const("ab", e);
        assert!(
            r.contains(&"A".to_string()) && r.contains(&"B".to_string()),
            "K1 {e}: {r:?}"
        );
    }
    let pf = facts(
        &tracks(&["obj.position", "PLACEHOLDER"])
            .replace("'PLACEHOLDER'", "String( process.argv[ 2 ] )"),
    );
    let y = recv(&pf, "y");
    eprintln!("R9KEY1 K1 argv track y={y:?}");
    assert!(
        y.len() > 3 || pf.sites.iter().any(|s| s.key == "y" && s.top),
        "K1 argv track: {y:?}"
    );
}

// K2 KEEP: two constant keys, one absent on the receiver: the read is
// exactly the present member and undefined, never another member; the
// `x !== undefined` test leaves the present member only.
#[test]
fn r9key1_k2_two_keys_one_absent() {
    let pf = facts(
        r#"class A { p = 1; }
class B { p = 2; }
const r: any = { a: new A(), c: new B() };
const k: any = Math.random() < 0.5 ? 'a' : 'b';
const x: any = r[ k ];
if ( x !== undefined ) { console.log( x.p ); }
"#,
    );
    assert_eq!(recv(&pf, "p"), v(&["A"]));
}
