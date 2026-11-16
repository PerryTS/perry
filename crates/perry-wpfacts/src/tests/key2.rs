//! KEY2 fixtures: the losing step of the three.js track key in the real
//! r9b solve. `parseTrackName` (three.core.js:53210-53250) runs the
//! `lastDot` block on every track: `results.nodeName.lastIndexOf( '.' )`
//! (:53228) was any Number, so `lastDot !== -1` (:53230) never excluded
//! the block, `results.nodeName.substring( lastDot + 1 )` (:53232) was any
//! String, and `results.objectName = objectName` (:53241) stored it. `bind`
//! (:53492, :53516) then read `targetObject[ objectName ]` (:53599, :53606)
//! under an any key, `this.targetObject` (:53642) held every field value
//! of an Object3D (the uuid String among them) and `_getValue_direct`
//! (:53334) read `this.targetObject[ this.propertyName ]` on a String:
//! the String prototype chain, `Builtin(String)`, the realm hold.
//! Rules: 22.1.3.9 indexOf / 22.1.3.12 lastIndexOf and 22.1.3.25
//! substring on constant operands (a Number interval, a String set), and
//! `x !== k` / `x === k` false excluding `k` from an integer interval's
//! end (7.2.15 IsStrictlyEqual of Numbers).
//! F1 smallest form of the losing step, F2 the folds, F3 three.js
//! :53228-53250 verbatim on "skinned.material.opacity", K1 KEEP unknown
//! subject, K2 KEEP sets give every combination.

use super::*;

pub(super) fn recv(pf: &ProgramFacts, key: &str) -> Vec<String> {
    let mut r: Vec<String> = pf
        .sites
        .iter()
        .filter(|s| s.key == key && !s.is_call)
        .flat_map(|s| s.receivers.iter())
        .map(|r| r.rsplit(':').next().unwrap_or(r).to_string())
        .collect();
    r.sort();
    r.dedup();
    r
}

fn dump(tag: &str, pf: &ProgramFacts) {
    for s in pf.sites.iter().filter(|s| !s.is_call) {
        eprintln!(
            "KEY2 {tag} key={:?} @{} top={} recv={:?} verdict={:?}",
            s.key, s.byte_offset, s.top, s.receivers, s.verdict
        );
    }
}

pub(super) fn v(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| s.to_string()).collect()
}

/// three.core.js:53001-53033, verbatim (types added).
pub(super) const TRACK_RE: &str = r#"
const _RESERVED_CHARS_RE = '\\[\\]\\.:\\/';
const _reservedRe = new RegExp( '[' + _RESERVED_CHARS_RE + ']', 'g' );
const _wordChar = '[^' + _RESERVED_CHARS_RE + ']';
const _wordCharOrDot = '[^' + _RESERVED_CHARS_RE.replace( '\\.', '' ) + ']';
const _directoryRe = /((?:WC+[\/:])*)/.source.replace( 'WC', _wordChar );
const _nodeRe = /(WCOD+)?/.source.replace( 'WCOD', _wordCharOrDot );
const _objectRe = /(?:\.(WC+)(?:\[(.+)\])?)?/.source.replace( 'WC', _wordChar );
const _propertyRe = /\.(WC+)(?:\[(.+)\])?/.source.replace( 'WC', _wordChar );
const _trackRe = new RegExp( ''
	+ '^'
	+ _directoryRe
	+ _nodeRe
	+ _objectRe
	+ _propertyRe
	+ '$'
);
const _supportedObjectNames = [ 'material', 'materials', 'bones', 'map' ];
"#;

/// three.core.js:53210-53250 (`parseTrackName`, verbatim but the throws'
/// messages) and the `objectName` part of `bind` (:53489-53627).
pub(super) const PARSE_BIND: &str = r#"
function parseTrackName( trackName: any ): any {
	const matches = _trackRe.exec( trackName );
	if ( matches === null ) {
		throw new Error( 'THREE.PropertyBinding: Cannot parse trackName: ' + trackName );
	}
	const results: any = {
		nodeName: matches[ 2 ],
		objectName: matches[ 3 ],
		objectIndex: matches[ 4 ],
		propertyName: matches[ 5 ],
		propertyIndex: matches[ 6 ]
	};
	const lastDot = results.nodeName && results.nodeName.lastIndexOf( '.' );
	if ( lastDot !== undefined && lastDot !== -1 ) {
		const objectName = results.nodeName.substring( lastDot + 1 );
		if ( _supportedObjectNames.indexOf( objectName ) !== -1 ) {
			results.nodeName = results.nodeName.substring( 0, lastDot );
			results.objectName = objectName;
		}
	}
	if ( results.propertyName === null || results.propertyName.length === 0 ) {
		throw new Error( 'THREE.PropertyBinding: can not parse propertyName from trackName: ' + trackName );
	}
	return results;
}
function bindT( root: any, parsedPath: any ): any {
	let targetObject = root;
	const objectName = parsedPath.objectName;
	if ( objectName ) {
		let objectIndex = parsedPath.objectIndex;
		switch ( objectName ) {
			case 'materials':
				targetObject = targetObject.material.materials;
				break;
			default:
				if ( targetObject[ objectName ] === undefined ) {
					throw new Error( 'no objectName' );
				}
				targetObject = targetObject[ objectName ];
		}
		if ( objectIndex !== undefined ) {
			targetObject = targetObject[ objectIndex ];
		}
	}
	return targetObject;
}
"#;

fn shape(body: &str) -> ProgramFacts {
    facts(&format!(
        r#"class V {{ x = 1; y = 1; }}
class U {{ x = 2; y = 2; }}
class W {{ x = 3; y = 3; }}
class Mat {{ opacity: any = new W(); }}
class Obj {{ name: any = 'obj'; uuid: any = String( Math.random() ); position: any = new V(); scale: any = new U(); material: any = new Mat(); other(): number {{ return 1; }} }}
{TRACK_RE}
{PARSE_BIND}
const o: any = new Obj();
{body}
"#
    ))
}

// F1: the losing step, smallest form: the track "obj.position" has no
// object part; the lastDot block is dead (lastIndexOf is -1), so
// `objectName` is `undefined` only, `bindT` returns `o` and the key names
// `position`: `.y` reads V only. Today: lastIndexOf any Number,
// substring any String, `objectName` any, `o[ objectName ]` any key.
#[test]
fn key2_f1_dead_lastdot_block_keeps_target() {
    let pf = shape(
        "const p = parseTrackName( 'obj.position' ); const t = bindT( o, p ); console.log( t[ p.propertyName ].y );",
    );
    dump("F1", &pf);
    assert_eq!(recv(&pf, "y"), v(&["V"]));
}

/// Is the String `e` exactly the constant `want`? `d[ key ].p` reads A
/// only when it is.
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

// F2: the folds on constants (22.1.3.12 lastIndexOf, 22.1.3.25 substring:
// ToIntegerOrInfinity, clamp to [0, len], swap when start > end;
// undefined start is 0, undefined end is len). NaN start is 0 too, but the
// interval domain has no NaN point (`model::range`: NaN is ⊤), so
// `substring( NaN, i )` is any String: not asserted.
#[test]
fn key2_f2_lastindexof_substring_fold() {
    let mut bad = Vec::new();
    for (want, e) in [
        ("obj", "'obj'.substring( 'obj'.lastIndexOf( '.' ) + 1 )"),
        ("a.b", "'a.b.c'.substring( 0, 'a.b.c'.lastIndexOf( '.' ) )"),
        ("c", "'a.b.c'.substring( 'a.b.c'.lastIndexOf( '.' ) + 1 )"),
        ("b.c", "'a.b.c'.substring( 'a.b.c'.indexOf( '.' ) + 1 )"),
        ("bc", "'abc'.substring( 5, 1 )"),
        ("abc", "'abc'.substring( -2 )"),
        ("abc", "'abc'.substring( undefined )"),
    ] {
        let r = is_const(want, e);
        eprintln!("KEY2 F2 {e} -> {r:?}");
        if r != v(&["A"]) {
            bad.push(format!("{e}: {r:?}"));
        }
    }
    assert!(bad.is_empty(), "not folded: {bad:?}");
}

// F3: three.js :53228-53250 on "skinned.material.opacity": the object
// part `material` is split off the node name, `bindT` goes to the
// material and the key names `opacity`: `.y` reads W only. The guard
// `_supportedObjectNames.indexOf( objectName ) !== -1` is not decided
// (a call, not a local), so the flow-insensitive solve also keeps the
// unsplit fields: nodeName {"skinned", "skinned.material"}, objectName
// {undefined, "material"}: `t` is {o, o.material} and `t.opacity` on `o`
// is the honest `undefined` arm.
#[test]
fn key2_f3_three_object_part_is_split() {
    let pf = shape(
        "const p = parseTrackName( 'skinned.material.opacity' ); const t = bindT( o, p ); console.log( t[ p.propertyName ].y );",
    );
    dump("F3", &pf);
    let y = recv(&pf, "y");
    assert!(y.contains(&"W".to_string()), "F3 y: {y:?}");
    assert!(!y.iter().any(|r| r == "V" || r == "U"), "F3 y: {y:?}");
}

// K1 KEEP: an unknown subject stays any String: both members read.
#[test]
fn key2_k1_unknown_subject_stays_unknown() {
    for e in ["String( process.argv[ 2 ] ).substring( 0, 2 )", "'abc'.substring( 0, Number( process.argv[ 2 ] ) )"] {
        let r = is_const("ab", e);
        assert!(r.contains(&"A".to_string()) && r.contains(&"B".to_string()), "K1 {e}: {r:?}");
    }
}

// K2 KEEP: a set of two constant subjects gives the union of both
// results; an index that is a set of two numbers gives every result.
#[test]
fn key2_k2_sets_give_every_combination() {
    let two = |want1: &str, want2: &str, e: &str| {
        let pf = facts(&format!(
            r#"class A {{ p = 1; }}
class B {{ p = 2; }}
class C {{ p = 3; }}
const d: any = {{ "{want1}": new A(), "{want2}": new C(), other: new B() }};
const s: any = Math.random() < 0.5 ? 'a.b' : 'c.d.e';
const i: any = Math.random() < 0.5 ? 1 : 2;
const key: any = {e};
console.log( d[ key ].p );
"#
        ));
        recv(&pf, "p")
    };
    let r = two("b", "d.e", "s.substring( s.indexOf( '.' ) + 1 )");
    eprintln!("KEY2 K2a {r:?}");
    assert_eq!(r, v(&["A", "C"]));
    let r = two("bc", "c", "'abc'.substring( i )");
    eprintln!("KEY2 K2b {r:?}");
    assert_eq!(r, v(&["A", "C"]));
}

// F4: `d0 !== -1` true excludes -1 from the reads it guards (7.2.15 on
// Numbers; a guard clip is for a function's locals), as `d0 > -1` does;
// `ne0` (the guard on another local) leaves `d0`'s -1: "obj" is a key.
#[test]
fn key2_f4_ne_guard_excludes_the_point() {
    for (tag, test) in [
        ("ne", "d0 !== -1"),
        ("und-and-ne", "d0 !== undefined && d0 !== -1"),
        ("gt", "d0 > -1"),
        ("ne0", "d1 !== 0"),
    ] {
        let pf = facts(&format!(
            r#"class A {{ p = 1; }}
class B {{ p = 2; }}
const d: any = {{ "keep": new A(), other: new B() }};
function f( s: any ): any {{
  const d0 = s && s.lastIndexOf( '.' );
  const d1 = s.indexOf( 'o' );
  let k: any = 'keep';
  if ( {test} ) {{ k = s.substring( d0 + 1 ); }}
  return k;
}}
console.log( d[ f( 'obj' ) ].p );
"#
        ));
        let r = recv(&pf, "p");
        eprintln!("KEY2 F4 {tag}: {r:?}");
        match tag {
            "ne0" => assert_eq!(r, v(&["A", "undefined (TypeError)"]), "F4 {tag}"),
            _ => assert_eq!(r, v(&["A"]), "F4 {tag}"),
        }
    }
}
