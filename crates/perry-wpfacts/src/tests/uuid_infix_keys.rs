//! K7 fixture (three.js `AnimationMixer._bindAction`, three.core.js:55213:
//! `bindingsByRoot[ rootUuid ]` with `rootUuid = root.uuid` from
//! `generateUUID()`): a string built by `+` with a constant `'-'` between
//! its parts (ECMA-262 13.15.3: the constant is part of the result) and then
//! flattened by `String.prototype.toLowerCase` (22.1.3.28: the `-` maps to
//! itself) contains a `-`, so it never names an Object.prototype member (no
//! built-in member name contains `-`). On this head the read reads every
//! Object.prototype method (builtin-chain-any), the binding found there is a
//! built-in method object, and its unlocked fields carry the glTF JSON into
//! `Interpolant::copySampleValue_` (K7). Controls: a key from
//! `process.argv[2]` and `process.argv[2].toLowerCase()` (may be
//! `constructor`), `'to' + process.argv[2]` (may be `toString`).
use super::*;

/// The sorted class names (or other tokens) the `.p` reads see.
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

fn prog(key: &str) -> String {
    format!(
        "class A {{ p = 1; }}
         const lut: string[] = [];
         for (let i = 0; i < 256; i++) lut[i] = (i < 16 ? '0' : '') + i.toString(16);
         function generateUUID(): string {{
           const d0 = Math.random() * 0xffffffff | 0;
           const d1 = Math.random() * 0xffffffff | 0;
           const uuid = lut[d0 & 0xff] + lut[d0 >> 8 & 0xff] + '-' + lut[d1 & 0xff] + lut[d1 >> 8 & 0xff];
           return uuid.toLowerCase();
         }}
         class Root {{ uuid: string; constructor() {{ this.uuid = generateUUID(); }} }}
         const root = new Root();
         const byRoot: any = {{}};
         const rootUuid = {key};
         let m: any = byRoot[rootUuid];
         if (m === undefined) {{ m = new A(); byRoot[rootUuid] = m; }}
         const x: any = byRoot[rootUuid];
         console.log(m.p, x.p);"
    )
}

#[test]
fn dash_infix_key_reads_no_builtin_member() {
    let v = recv(&facts(&prog("root.uuid")));
    assert_eq!(v, vec!["A"], "{v:?}");
}

#[test]
fn argv_key_reads_builtin_members() {
    let v = recv(&facts(&prog("process.argv[2]")));
    assert!(v.len() > 1, "{v:?}");
}

#[test]
fn argv_lowercase_key_reads_builtin_members() {
    let v = recv(&facts(&prog("String(process.argv[2]).toLowerCase()")));
    assert!(v.len() > 1, "{v:?}");
}

#[test]
fn to_prefix_argv_key_reads_builtin_members() {
    let v = recv(&facts(&prog("'to' + process.argv[2]")));
    assert!(v.len() > 1, "{v:?}");
}

#[test]
fn argv_then_const_suffix_key_reads_builtin_members() {
    // `x + 'a'`: no character outside IdentifierPart is sure.
    let v = recv(&facts(&prog("process.argv[2] + 'a'")));
    assert!(v.len() > 1, "{v:?}");
}

#[test]
fn replace_drops_infix() {
    let v = recv(&facts(&prog("('x-' + process.argv[2]).replace('-', '')")));
    assert!(v.len() > 1, "{v:?}");
}

#[test]
fn slice_drops_infix() {
    let v = recv(&facts(&prog("root.uuid.slice(1)")));
    assert!(v.len() > 1, "{v:?}");
}

#[test]
fn field_joined_with_argv_drops_infix() {
    let src = prog("root.uuid").replace(
        "this.uuid = generateUUID();",
        "this.uuid = generateUUID(); if (process.argv.length > 5) this.uuid = process.argv[2];",
    );
    let v = recv(&facts(&src));
    assert!(v.len() > 1, "{v:?}");
}

// Rule K8 (ECMA-262 7.1.19 ToPropertyKey, 7.1.17 ToString): a key that is a
// dash-infix String or `undefined` / `null` reads the infix names and the
// named key "undefined" / "null"; neither names an Object.prototype member.
// three.js `_bindAction`: `root.uuid` of a root that may lack `uuid`.

#[test]
fn dash_infix_or_absent_uuid_key_reads_no_builtin_member() {
    let v = recv(&facts(&prog(
        "(Math.random() < 0.5 ? root : ({} as any)).uuid",
    )));
    assert_eq!(v, vec!["A"], "{v:?}");
}

#[test]
fn dash_infix_or_null_key_reads_no_builtin_member() {
    let v = recv(&facts(&prog("(Math.random() < 0.5 ? root.uuid : null)")));
    assert_eq!(v, vec!["A"], "{v:?}");
}

#[test]
fn argv_or_undefined_key_reads_builtin_members() {
    let v = recv(&facts(&prog(
        "(Math.random() < 0.5 ? process.argv[2] : undefined)",
    )));
    assert!(v.len() > 1, "{v:?}");
}

/// three.js `_lut` is a literal array of constants (three.core.js:2297):
/// indexed in bounds (`& 3` of four entries, as `& 0xff` of 256 there),
/// `lut[a] + lut[b]` gives known prefixes (rule M3), the `-` after them is
/// not kept beside a prefix, and the key reaches the read through
/// `toLowerCase` of those prefixes (22.1.3.28: an ASCII code point maps on
/// its own, so a known ASCII prefix stays a known prefix).
fn literal_lut_prog(lut: &str, key: &str) -> String {
    let p = prog(key);
    let q = p
        .replace("const lut: string[] = [];", &format!("const lut: string[] = [{lut}];"))
        .replace("for (let i = 0; i < 256; i++) lut[i] = (i < 16 ? '0' : '') + i.toString(16);", "")
        .replace("& 0xff]", "& 3]");
    assert!(!q.contains("i.toString(16)") && !q.contains("0xff]"), "{q}");
    q
}

#[test]
fn literal_lut_prefix_key_reads_no_builtin_member() {
    let v = recv(&facts(&literal_lut_prog("'bc', 'cd', 'db', 'bd'", "root.uuid")));
    assert_eq!(v, vec!["A"], "{v:?}");
}

#[test]
fn literal_lut_prefix_upper_key_reads_no_builtin_member() {
    let v = recv(&facts(&literal_lut_prog("'bc', 'cd', 'db', 'bd'", "root.uuid.toUpperCase()")));
    assert_eq!(v, vec!["A"], "{v:?}");
}

#[test]
fn literal_lut_member_prefix_key_reads_builtin_members() {
    // `'co' + 'ns' + ...` may spell `constructor`.
    let v = recv(&facts(&literal_lut_prog("'co', 'ns', 'co', 'ns'", "root.uuid")));
    assert!(v.len() > 1, "{v:?}");
}

#[test]
fn literal_lut_upper_member_prefix_key_reads_builtin_members() {
    // `'CO' + 'NS' + ...` lower-cased may spell `constructor`.
    let v = recv(&facts(&literal_lut_prog("'CO', 'NS', 'CO', 'NS'", "root.uuid")));
    assert!(v.len() > 1, "{v:?}");
}

/// BATCHB58 (three.core.js:55213, `_lut` of hex pairs `00`..`ff`): a
/// String key of known prefixes reads, per prefix, the names its glob
/// admits, and the elements and numeric keys only when the prefix may begin
/// ToString of a Number (ECMA-262 6.1.6.1.20; `-0`, 7.1.21): `10*` may
/// (`10`, `1e+21`), `0a*`, `00*` (no canonical `00`), `ff*` may not. A
/// prefix that may be numeric never makes the key any key.
fn literal_lut_arr_prog(lut: &str, key: &str) -> String {
    let p = literal_lut_prog(lut, key);
    let q = p.replace(
        "const byRoot: any = {};",
        "class B { p = 2; }\n         const byRoot: any = [];\n         byRoot[Math.random() * 99 | 0] = new B();",
    );
    assert!(q.contains("byRoot[Math.random() * 99 | 0] = new B();"), "{q}");
    q
}

#[test]
fn glob_number_prefix_is_exact() {
    let numeric = |p: &str| {
        crate::model::KeyShape {
            globs: vec![vec![p.to_string(), String::new()]],
        }
        .may_be_numeric()
    };
    for p in ["", "1", "10", "1e", "1e+", "1.5e-", "0", "0.", "0.00", "-0", "-", "-1.", "N", "NaN", "-I", "Inf", "9e+2"] {
        assert!(numeric(p), "{p} may begin a Number string");
    }
    for p in ["00", "0a", "ff", "a", "1a", "0e", "10e", "1.e", "1.50e", "1e0", "1e+0", "inf", "nan", "-N", "--", "1-"] {
        assert!(!numeric(p), "{p} begins no Number string");
    }
}

#[test]
fn hex_prefix_key_reads_no_builtin_member() {
    let v = recv(&facts(&literal_lut_prog("'0a', 'ff', '00', '09'", "root.uuid")));
    assert_eq!(v, vec!["A"], "{v:?}");
}

#[test]
fn digit_prefix_key_reads_no_builtin_member() {
    // `10*`.. may spell an index, never an Object.prototype member.
    let v = recv(&facts(&literal_lut_prog("'10', '12', '13', '1e'", "root.uuid")));
    assert_eq!(v, vec!["A"], "{v:?}");
}

/// A plain object with a store under an unknown Number key (`numeric_field`). An Array
/// keeps its elements in `field_any`, which every computed-key read of it
/// reads, so the numeric part is told apart on an ordinary object.
fn literal_lut_num_prog(lut: &str, key: &str) -> String {
    let p = literal_lut_prog(lut, key);
    let q = p.replace(
        "const byRoot: any = {};",
        "class B { p = 2; }\n         const byRoot: any = {};\n         byRoot[Math.random() * 99 | 0] = new B();",
    );
    assert!(q.contains("byRoot[Math.random() * 99 | 0] = new B();"), "{q}");
    q
}

#[test]
fn hex_prefix_key_reads_no_numeric_key() {
    // `0a*`, `ff*`, `00*`, `09*` begin no ToString of a Number.
    let v = recv(&facts(&literal_lut_num_prog("'0a', 'ff', '00', '09'", "root.uuid")));
    assert_eq!(v, vec!["A"], "{v:?}");
}

#[test]
fn digit_prefix_key_reads_numeric_key() {
    // `10*` may spell a Number key: the numeric keys (`numeric_field`, one
    // field for every Number key, `byRoot[Math.random() * 99 | 0]` among them) are read.
    let v = recv(&facts(&literal_lut_num_prog("'10', '12', '13', '14'", "root.uuid")));
    assert!(v.contains(&"B".to_string()), "{v:?}");
}

#[test]
fn digit_prefix_key_reads_elements() {
    // `10*` may spell an index: the element `byRoot[0]` wrote is read.
    let v = recv(&facts(&literal_lut_arr_prog("'10', '12', '13', '14'", "root.uuid")));
    assert!(v.contains(&"B".to_string()), "{v:?}");
}

#[test]
fn literal_lut_to_va_prefix_key_reads_builtin_members() {
    // `'to' + 'St'` may spell `toString`, `'va' + 'lu'` `valueOf`.
    let v = recv(&facts(&literal_lut_prog("'to', 'St', 'va', 'lu'", "root.uuid")));
    assert!(v.len() > 1, "{v:?}");
}

#[test]
fn dash_infix_key_reads_numeric_key() {
    // `*-*` may spell `-1` (ToString(-1)): a shaped read of a shape that may
    // spell a Number key reads the store under an unknown Number key.
    let p = prog("root.uuid").replace(
        "const byRoot: any = {};",
        "class B { p = 2; }\n         const byRoot: any = {};\n         byRoot[Math.random() * 99 - 50 | 0] = new B();",
    );
    assert!(p.contains("new B();"), "{p}");
    let v = recv(&facts(&p));
    assert!(v.contains(&"B".to_string()), "{v:?}");
}
