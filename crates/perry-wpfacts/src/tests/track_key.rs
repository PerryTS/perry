//! KEY3 fixture: the three.js animation track key, reduced from
//! GLTFLoader.js:4579-4664 (`targetName + '.' + PATH_PROPERTIES[path]`),
//! three.core.js:53185 (`PropertyBinding.sanitizeNodeName`) and
//! three.core.js:53003-53033 / 53200-53245 (`_trackRe`, `parseTrackName`).
//! The node part passes `replace(/\s/g, '_').replace(_reservedRe, '')`,
//! so it holds none of `[ ] . : /` (ECMA-262 22.1.3.19, 22.2.6.11). The
//! property part is ToString of an ordinary object's read under any key:
//! its four strings, `undefined`, a builtin method of Object.prototype
//! (Function.prototype.toString, 20.2.3.5: perry's NativeFunction text) or
//! Object.prototype itself through `__proto__` (`[object Object]`,
//! 20.1.3.6). So the capture `propertyName` is one of the four strings and
//! `objectName` is undefined; a store under `propertyName` never writes a
//! method. Controls: without the sanitizer, with a raw key in place of the
//! table read, or with a program `Object.prototype.toString`, the key is
//! any word.
use super::*;

fn recv(pf: &ProgramFacts, key: &str) -> Vec<String> {
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

/// `node` is the node-name expression, `prop` the property-name
/// expression, `pre` extra program text, `tail` the use of `p` (the
/// parsed track) and `h`.
fn prog(node: &str, prop: &str, pre: &str, tail: &str) -> ProgramFacts {
    let src = format!(
        r#"class A {{ p = 1; q = 1; }}
class S {{ p = 2; q = 2; }}
class Q {{ p = 3; q = 3; }}
class W {{ p = 4; q = 4; }}
class B {{ p = 5; q = 5; }}
class H {{
  position: any = new A(); scale: any = new S(); quaternion: any = new Q();
  morphTargetInfluences: any = new W(); ctl: any = new B();
  m(): number {{ return 1; }}
}}
const h: any = new H();
const n: any = process.argv[2];
const t: any = process.argv[3];
{pre}
const _RESERVED_CHARS_RE = '\\[\\]\\.:\\/';
const _reservedRe = new RegExp( '[' + _RESERVED_CHARS_RE + ']', 'g' );
function sanitize( name: any ): any {{ return name.replace( /\s/g, '_' ).replace( _reservedRe, '' ); }}
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
function parse( trackName: any ): any {{
  const matches = _trackRe.exec( trackName );
  if ( matches === null ) throw new Error( 'cannot parse ' + trackName );
  return {{ nodeName: matches[ 2 ], objectName: matches[ 3 ], objectIndex: matches[ 4 ],
    propertyName: matches[ 5 ], propertyIndex: matches[ 6 ] }};
}}
const PATH: any = {{ translation: 'position', rotation: 'quaternion', scale: 'scale', weights: 'morphTargetInfluences' }};
const track = {node} + '.' + {prop};
const p = parse( track );
{tail}
"#
    );
    facts(&src)
}

const SAN: &str = "sanitize( String( n ) )";
const TAB: &str = "PATH[ String( t ) ]";
const READ: &str = "const x: any = h[ p.propertyName ]; console.log( x.p );";

fn four() -> Vec<String> {
    vec!["A".into(), "Q".into(), "S".into(), "W".into()]
}

#[test]
fn property_name_is_one_of_the_four_paths() {
    let v = recv(&prog(SAN, TAB, "", READ), "p");
    assert_eq!(v, four(), "{v:?}");
}

#[test]
fn object_name_is_undefined() {
    let v = recv(&prog(SAN, TAB, "", "const y: any = h[ p.objectName ]; console.log( y.q );"), "q");
    assert!(v.is_empty(), "{v:?}");
}

// The store under the parsed property writes no method: `h.m()` stays a
// call of `H.prototype.m` alone.
#[test]
fn store_under_property_name_keeps_the_method() {
    let pf = prog(SAN, TAB, "", "h[ p.propertyName ] = 1; console.log( h.m() );");
    let s = site(&pf, "m");
    assert!(!matches!(s.verdict, Verdict::No(_)), "{:?}", s.verdict);
}

// Control: a raw host string as the node part may hold `.` and `[`:
// `objectName` may be any word. `propertyName` still is one of the four:
// the track ends with `.` and a known text, so group 5 is that text after
// the last `.` (the `[native code] }` form has no way: exec is null).
#[test]
fn raw_node_part_is_any_key() {
    let v = recv(&prog("String( n )", TAB, "", "const y: any = h[ p.objectName ]; console.log( y.q );"), "q");
    assert!(v.iter().any(|x| x == "B"), "{v:?}");
    let v = recv(&prog("String( n )", TAB, "", READ), "p");
    assert_eq!(v, four(), "{v:?}");
}

// Control: a raw host string in place of the table read: any word.
#[test]
fn raw_property_part_is_any_word() {
    let v = recv(&prog(SAN, "String( t )", "", READ), "p");
    assert!(v.iter().any(|x| x == "B"), "{v:?}");
}

// Control: a program `Object.prototype.toString` makes the `__proto__`
// form `ToString(Object.prototype)` unknown.
#[test]
fn program_object_to_string_is_unknown() {
    let pre = "(Object.prototype as any).toString = function () { return String( n ); };";
    let v = recv(&prog(SAN, TAB, pre, READ), "p");
    assert!(v.iter().any(|x| x == "B"), "{v:?}");
}

// KEY3B pieces 1 and 3: a constant property part. The node part holds
// none of `[ ] . : /` (rule M9 through rule M8), so the only way is the
// node part as group 2 and the constant after the last `.` as group 5
// (rule RH): `objectName` undefined, `propertyName` the constant.
#[test]
fn constant_property_name_is_exact() {
    let v = recv(&prog(SAN, "'position'", "", READ), "p");
    assert_eq!(v, vec!["A".to_string()], "{v:?}");
}

#[test]
fn constant_property_object_name_is_undefined() {
    let v = recv(&prog(SAN, "'position'", "", "const y: any = h[ p.objectName ]; console.log( y.q );"), "q");
    assert!(v.is_empty(), "{v:?}");
}

// Control: a raw node part may hold `.`: `objectName` may be any word.
#[test]
fn constant_property_raw_node_object_name_is_any() {
    let v = recv(&prog("String( n )", "'position'", "", "const y: any = h[ p.objectName ]; console.log( y.q );"), "q");
    assert!(v.iter().any(|x| x == "B"), "{v:?}");
}

// The property part a constant or the `Object` constructor itself: its
// text `function Object() { [native code] }` has no way through the
// pattern, so `propertyName` is the constant alone.
const OBJ: &str = "( String( t ) === 'a' ? 'position' : Object )";
// The same part after a suffix entry (`String( n ) + '.'`, rule M4): the
// key ends with `.position` or `[native code] }`, no field of `h`.
const SUFKEY: &str = "const k2: any = String( n ) + '.' + ( String( t ) === 'a' ? 'position' : Object ); const z: any = h[ k2 ]; console.log( z.q );";

#[test]
fn builtin_function_text_has_no_way() {
    let v = recv(&prog(SAN, OBJ, "", READ), "p");
    assert_eq!(v, vec!["A".to_string()], "{v:?}");
    let v = recv(&prog(SAN, OBJ, "", SUFKEY), "q");
    assert!(v.is_empty(), "{v:?}");
}

// Unknown code (an undeclared host function's call) may store `toString`
// on any builtin: the builtin-function form widens to any string
// (`realm_str_dsts`), so the key `k2` may be any key. The call is
// reached only through `propertyName` itself, so the realm is held after
// the concatenation used the form (the widening, not the gate, decides).
#[test]
fn unknown_code_widens_the_native_form() {
    let tail = format!(
        "const g: any = {{ position: () => (globalThis as any).hostTake( 1 ) }}; g[ p.propertyName ](); {SUFKEY}"
    );
    let v = recv(&prog(SAN, OBJ, "", &tail), "q");
    assert!(v.iter().any(|x| x == "B"), "{v:?}");
}

// A program `Object.prototype.toString` returning a text with `.`: the
// object forms are not used, `propertyName` may be any word.
#[test]
fn program_object_to_string_constant_is_unknown() {
    let pre = "(Object.prototype as any).toString = function () { return 'x.y'; };";
    let v = recv(&prog(SAN, TAB, pre, READ), "p");
    assert!(v.iter().any(|x| x == "B"), "{v:?}");
}
