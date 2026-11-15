//! REFOLD1 fixtures: the three.js animation track key `propertyName` of
//! `PropertyBinding` (three.core.js:53003-53033 `_trackRe`, :53210-53245
//! `parseTrackName`, :53334 / :53366 `targetObject[ propertyName ]`) when
//! the track name is a program constant. ECMA-262 22.2.7.2
//! RegExpBuiltinExec on a constant pattern and a constant subject is one
//! match array; the key is its group 5, the read a named load.
//! F1 minimal, F2 the three.js shape (field, static parse, result object,
//! bound getter), F3 KEEP (unknown subject), F4 KEEP (two constants).

use super::*;

fn rows(pf: &ProgramFacts, key: &str) -> Vec<String> {
    pf.sites
        .iter()
        .filter(|s| s.key == key && !s.is_call)
        .map(|s| format!("@{} top={} recv={:?} verdict={:?}", s.byte_offset, s.top, s.receivers, s.verdict))
        .collect()
}

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

fn dump(tag: &str, pf: &ProgramFacts) {
    for s in pf.sites.iter().filter(|s| !s.is_call) {
        eprintln!(
            "REFOLD1 {tag} key={:?} @{} top={} recv={:?} verdict={:?}",
            s.key, s.byte_offset, s.top, s.receivers, s.verdict
        );
    }
}

/// three.core.js:53001-53033, verbatim (types added).
const TRACK_RE: &str = r#"
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
"#;

fn f1(subject: &str) -> ProgramFacts {
    facts(&format!(
        r#"class V {{ x = 1; }}
class U {{ x = 2; }}
class T {{ position: any = new V(); other(): number {{ return 1; }} }}
const re = new RegExp( "^(\\w+)\\.(\\w+)$" );
const m: any = re.exec( {subject} );
const k: any = m[ 2 ];
const t: any = new T();
const v: any = t[ k ];
console.log( v.x );
"#
    ))
}

/// The three.js shape: the name lives in a track's field, a mixer loop
/// reads it, a static method parses it into a result object literal, the
/// binding object keeps it in a field and a bound method reads under it.
fn f2(tracks: &str) -> ProgramFacts {
    f2v(tracks, "track.name", "propertyName: any = null;")
}

fn f2v(tracks: &str, name: &str, pfield: &str) -> ProgramFacts {
    f2r(TRACK_RE, tracks, name, pfield)
}

fn f2r(track_re: &str, tracks: &str, name: &str, pfield: &str) -> ProgramFacts {
    facts(&format!(
        r#"class V {{ x = 1; y = 1; }}
class U {{ x = 2; y = 2; }}
class Obj {{ name: any = ''; position: any = new V(); scale: any = new U(); other(): number {{ return 1; }} }}
{track_re}
class Track {{ name: any; times: any; constructor( name: any, times: any ) {{ this.name = name; this.times = times; }} }}
class Clip {{ tracks: any; constructor( tracks: any ) {{ this.tracks = tracks; }} }}
class Binding {{
  path: any; parsedPath: any; node: any; targetObject: any = null; {pfield}
  constructor( root: any, path: any ) {{
    this.path = path;
    this.parsedPath = Binding.parseTrackName( path );
    this.node = root;
  }}
  static parseTrackName( trackName: any ): any {{
    const matches = _trackRe.exec( trackName );
    if ( matches === null ) throw new Error( 'cannot parse ' + trackName );
    const results = {{ nodeName: matches[ 2 ], objectName: matches[ 3 ], objectIndex: matches[ 4 ],
      propertyName: matches[ 5 ], propertyIndex: matches[ 6 ] }};
    return results;
  }}
  bind(): void {{
    const targetObject = this.node;
    const propertyName = this.parsedPath.propertyName;
    const nodeProperty = targetObject[ propertyName ];
    if ( nodeProperty === undefined ) return;
    console.log( nodeProperty.y );
    this.targetObject = targetObject;
    this.propertyName = propertyName;
  }}
  getValue(): any {{ return this.targetObject[ this.propertyName ]; }}
}}
const o: any = new Obj();
o.name = 'obj';
const clip = new Clip( [ {tracks} ] );
const bindings: any[] = [];
for ( let i = 0; i !== clip.tracks.length; ++ i ) {{
  const track = clip.tracks[ i ];
  const b = new Binding( o, {name} );
  b.bind();
  bindings.push( b );
}}
for ( let i = 0; i < bindings.length; i ++ ) {{
  const v: any = bindings[ i ].getValue();
  console.log( v.x );
}}
"#
    ))
}

// F1: constant subject, constant pattern: `t[k]` is the named load of
// `position`, `v.x` reads V only.
#[test]
fn refold1_f1_constant_exec_key_is_named() {
    let pf = f1("\"obj.position\"");
    dump("F1", &pf);
    let v = recv(&pf, "x");
    assert_eq!(v, vec!["V".to_string()], "F1 x rows: {:?}", rows(&pf, "x"));
}

// F2: the three.js shape, one constant track name.
#[test]
fn refold1_f2_three_shape_key_is_named() {
    let pf = f2("new Track( 'obj.position', [ 0, 1 ] )");
    dump("F2", &pf);
    let v = recv(&pf, "x");
    // `targetObject` is born `null` (an honest TypeError arm of the
    // flow-insensitive solve); the key must name `position` only.
    assert_eq!(v, vec!["V".to_string(), "undefined (TypeError)".to_string()], "F2 x rows: {:?}", rows(&pf, "x"));
}

// F3 KEEP: an unknown subject: the key may name `other` (a method) or an
// absent key (`undefined`): `v.x` must not be narrowed to V.
#[test]
fn refold1_f3_unknown_subject_stays_unknown() {
    let pf = f1("String( process.argv[ 2 ] )");
    dump("F3", &pf);
    let v = recv(&pf, "x");
    assert_ne!(v, vec!["V".to_string()], "F3 x rows: {:?}", rows(&pf, "x"));
}

// F4 KEEP: two constant track names: the key set is both names; `x` reads
// V and U and nothing else.
#[test]
fn refold1_f4_two_constants_two_loads() {
    let pf = f2("new Track( 'obj.position', [ 0, 1 ] ), new Track( 'obj.scale', [ 0, 1 ] )");
    dump("F4", &pf);
    let v = recv(&pf, "x");
    assert_eq!(
        v,
        vec!["U".to_string(), "V".to_string(), "undefined (TypeError)".to_string()],
        "F4 x rows: {:?}",
        rows(&pf, "x")
    );
}

// Probes (where the constant is lost): `y` is the read inside `bind`
// (local key), `x` the bound getter's (field key).
#[test]
fn refold1_probe_hops() {
    for (tag, name, pfield) in [
        ("P0 field+getter", "track.name", "propertyName: any = null;"),
        ("P1 direct const", "'obj.position'", "propertyName: any = null;"),
        ("P2 no null init", "track.name", "propertyName: any;"),
        ("P3 const+no init", "'obj.position'", "propertyName: any;"),
    ] {
        let pf = f2v("new Track( 'obj.position', [ 0, 1 ] )", name, pfield);
        eprintln!("REFOLD1 {tag}: y={:?} x={:?}", recv(&pf, "y"), recv(&pf, "x"));
    }
}

fn q(body: &str) -> ProgramFacts {
    facts(&format!(
        r#"class V {{ x = 1; y = 1; }}
class U {{ x = 2; y = 2; }}
class Obj {{ name: any = ''; position: any = new V(); scale: any = new U(); other(): number {{ return 1; }} }}
{TRACK_RE}
function parseF( trackName: any ): any {{
  const matches = _trackRe.exec( trackName );
  if ( matches === null ) throw new Error( 'cannot parse ' + trackName );
  return {{ nodeName: matches[ 2 ], propertyName: matches[ 5 ] }};
}}
class P {{ static parseS( trackName: any ): any {{ return parseF( trackName ); }} }}
class K {{ pp: any; constructor( path: any ) {{ this.pp = parseF( path ); }} }}
const o: any = new Obj();
{body}
"#
    ))
}

#[test]
fn refold1_probe_calls() {
    for (tag, body) in [
        ("Q0 top fn", "const p = parseF( 'obj.position' ); console.log( o[ p.propertyName ].y );"),
        ("Q1 static", "const p = P.parseS( 'obj.position' ); console.log( o[ p.propertyName ].y );"),
        ("Q2 ctor field", "const k = new K( 'obj.position' ); console.log( o[ k.pp.propertyName ].y );"),
        ("Q3 inline exec", "const m: any = _trackRe.exec( 'obj.position' ); console.log( o[ m[ 5 ] ].y );"),
        ("Q5 excl head", "const m: any = _trackRe.exec( String( process.argv[ 2 ] ).replace( _reservedRe, '' ) + '.position' ); console.log( o[ m[ 5 ] ].y );"),
        ("Q6 simple re", "const m: any = new RegExp( '^(\\\\w+)\\\\.(\\\\w+)$' ).exec( 'obj.position' ); console.log( o[ m[ 2 ] ].y );"),
        ("Q7 track re const ns", "const m: any = _trackRe.exec( '.position' ); console.log( o[ m[ 5 ] ].y );"),
        ("Q4 fn in method", "class M { run( s: any ): void { const p = parseF( s ); console.log( o[ p.propertyName ].y ); } } new M().run( 'obj.position' );"),
    ] {
        let pf = q(body);
        eprintln!("REFOLD1 {tag}: y={:?}", recv(&pf, "y"));
    }
}

/// Probe: is the string `e` exactly the constant `want`? `d[e].p` reads
/// A only when it is.
fn r(want: &str, e: &str) -> Vec<String> {
    let pf = facts(&format!(
        r#"class A {{ p = 1; }}
class B {{ p = 2; }}
{TRACK_RE}
const d: any = {{ "{want}": new A(), other: new B() }};
const key: any = {e};
console.log( d[ key ].p );
"#
    ));
    recv(&pf, "p")
}

#[test]
fn refold1_probe_pieces() {
    for (tag, want, e) in [
        ("R0 const", "ab", "'a' + 'b'"),
        ("R1 wordChar", r"[^\\[\\]\\.:\\/]", "_wordChar"),
        ("R2 replace", r"\\[\\]:\\/", "_RESERVED_CHARS_RE.replace( '\\\\.', '' )"),
        ("R3 source", "(WC)", "/(WC)/.source"),
        ("R4 source.replace", "(x)", "/(WC)/.source.replace( 'WC', 'x' )"),
        ("R5 propertyRe", r"\\.([^\\[\\]\\.:\\/]+)(?:\\[(.+)\\])?", "_propertyRe"),
        ("R6 trackRe.source", "x", "_trackRe.source"),
    ] {
        eprintln!("REFOLD1 {tag}: p={:?}", r(want, e));
    }
}

/// The same pipeline with a `_trackRe` written as one constant (groups 2
/// and 5 the node and property words): isolates the field / call hops from
/// rule M8 (`source` of a literal).
const PLAIN_RE: &str = r#"const _trackRe = new RegExp( '^()(\\w+)()()\\.(\\w+)()$' );"#;

#[test]
fn refold1_probe_plain_pipeline() {
    for (tag, tracks) in [
        ("S1 one", "new Track( 'obj.position', [ 0, 1 ] )"),
        ("S2 two", "new Track( 'obj.position', [ 0, 1 ] ), new Track( 'obj.scale', [ 0, 1 ] )"),
    ] {
        let pf = f2r(PLAIN_RE, tracks, "track.name", "propertyName: any = null;");
        eprintln!("REFOLD1 {tag}: y={:?} x={:?} xrows={:?}", recv(&pf, "y"), recv(&pf, "x"), rows(&pf, "x"));
    }
}


/// F1's program with pattern `pat` (a string constant), subject `subject`
/// and the capture index `g` as the key.
fn f1p(pat: &str, subject: &str, g: u32) -> ProgramFacts {
    facts(&format!(
        r#"class V {{ x = 1; }}
class T {{ position: any = new V(); other(): number {{ return 1; }} }}
const re = new RegExp( {pat} );
const m: any = re.exec( {subject} );
const k: any = m[ {g} ];
const t: any = new T();
const v: any = t[ k ];
console.log( v.x );
"#
    ))
}

// FOLDFIX1 KEEP: an optional group that does not take part in the match
// is an own `undefined` (22.2.7.2 step 33): `t[undefined]` reads the absent
// key `"undefined"`, so `v.x` keeps its TypeError arm.
#[test]
fn foldfix1_optional_group_unmatched_is_undefined() {
    let pf = f1p(r#""^(\\w+)?\\.(\\w+)$""#, r#"".position""#, 1);
    let v = recv(&pf, "x");
    // `m[ 1 ]` is exactly `undefined`: `v` is only `undefined` and `v.x` throws.
    let r = rows(&pf, "x");
    assert!(v.is_empty() && r.iter().all(|x| x.ends_with("verdict=Throws")) && !r.is_empty(), "{r:?}");
}

// FOLDFIX1 KEEP: a constant subject the pattern cannot match: `exec` is
// `null` (22.2.7.2 step 12.a), `m[ 2 ]` only throws, and no key reaches
// `t[ k ]`.
#[test]
fn foldfix1_no_match_is_null_only() {
    let pf = f1p(r#""^(\\w+)\\.(\\w+)$""#, r#""nodots""#, 2);
    let v = recv(&pf, "x");
    assert!(v.is_empty(), "{:?}", rows(&pf, "x"));
}
