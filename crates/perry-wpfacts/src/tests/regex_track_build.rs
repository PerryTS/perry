//! Rules M5-M8 fixture: three.js `_trackRe` built exactly as
//! three.core.js:53003-53033 (`PropertyBinding`): constant + constant is a
//! constant (ECMA-262 13.15.3), `String.prototype.replace(string, string)`
//! of constants is the constant result (22.1.3.19), `source` of a regex
//! literal is its pattern text (22.2.6.13.1, EscapeRegExpPattern), and
//! `new RegExp(constant)` holds that pattern as a literal does (22.2.4.1).
//! So `_trackRe.exec(String(n) + '.quaternion')[5]` is `quaternion` and
//! `h[m[5]]` reads only `Q` (rule RT). Control: one piece of the pattern
//! that is no constant gives any key.
use super::*;

fn recv(pf: &ProgramFacts) -> Vec<String> {
    let mut r: Vec<String> = pf
        .sites
        .iter()
        .filter(|s| s.key == "p" && !s.is_call)
        .flat_map(|s| s.receivers.iter())
        .map(|r| r.rsplit(':').next().unwrap_or(r).to_string())
        .collect();
    r.sort();
    r.dedup();
    r
}

/// three.core.js:53003-53033 verbatim, with `WORD` the `_wordChar` the
/// property part uses.
fn track(word: &str, idx: u32) -> Vec<String> {
    let src = format!(
        r#"class A {{ p = 1; }}
class S {{ p = 2; }}
class Q {{ p = 3; }}
class B {{ p = 4; }}
class H {{
  position: any = new A(); scale: any = new S(); quaternion: any = new Q(); ctl: any = new B();
  m(): number {{ return 1; }}
}}
const h: any = new H();
const n: any = process.argv[2];
const _RESERVED_CHARS_RE = '\\[\\]\\.:\\/';
const _wordChar = '[^' + _RESERVED_CHARS_RE + ']';
const _wordCharOrDot = '[^' + _RESERVED_CHARS_RE.replace( '\\.', '' ) + ']';
const _directoryRe = /((?:WC+[\/:])*)/.source.replace( 'WC', _wordChar );
const _nodeRe = /(WCOD+)?/.source.replace( 'WCOD', _wordCharOrDot );
const _objectRe = /(?:\.(WC+)(?:\[(.+)\])?)?/.source.replace( 'WC', _wordChar );
const _propertyRe = /\.(WC+)(?:\[(.+)\])?/.source.replace( 'WC', {word} );
const _trackRe = new RegExp( ''
	+ '^'
	+ _directoryRe
	+ _nodeRe
	+ _objectRe
	+ _propertyRe
	+ '$'
);
const m = _trackRe.exec( String(n) + '.quaternion' );
if (m !== null) {{ const x: any = h[m[{idx}]]; console.log(x.p); }}
"#
    );
    recv(&facts(&src))
}

/// `constructor` (class `H`) and the method `m` of `H.prototype`: the
/// program-prototype read takes no key entries yet.
const PROGRAM_PROTO: &[&str] = &["H", "m"];

#[test]
fn built_track_re_property_is_quaternion() {
    let v = track("_wordChar", 5);
    assert!(v.iter().any(|x| x == "Q"), "{v:?}");
    for x in &v {
        assert!(x == "Q" || PROGRAM_PROTO.contains(&x.as_str()), "{x} {v:?}");
    }
}

#[test]
fn built_track_re_property_index_is_undefined() {
    let v = track("_wordChar", 6);
    assert!(v.is_empty(), "{v:?}");
}

// Control: a pattern piece that is no constant: the pattern is unknown, the
// capture any string.
#[test]
fn unknown_piece_is_any_key() {
    let v = track("n", 5);
    assert!(v.iter().any(|x| x == "A") && v.iter().any(|x| x == "B"), "{v:?}");
}
