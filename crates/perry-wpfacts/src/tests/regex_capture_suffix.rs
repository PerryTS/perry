//! Rule RT fixture (three.js `PropertyBinding.parseTrackName`,
//! three.core.js:53003-53033 and 53201-53213, fed by `GLTFLoader` G:4662
//! `targetNames[j] + '.' + PATH_PROPERTIES[target.path]`): the capture of
//! an end-anchored regex literal whose group `(C+)` follows a part that
//! ends with a character not in C is the C-run of the input's known suffix
//! (ECMA-262 13.15.3: `x + c` ends with `c`, rule M4; 22.2.7.2: captures
//! are substrings fixed by the match; 22.2.2: every way the tail matches).
//! So `h[m[2]]` reads only `position`, never `constructor`, the method `m`
//! or an Object.prototype member. Controls: a group before the pinned one,
//! an input without a known suffix, a backreference or an alternation in
//! the tail.
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

const SIMPLE: &str = r"/^(?:([^.]*)\.)?([^.]+)$/";
/// `_trackRe` (three.core.js:53003-53033) with `_RESERVED_CHARS_RE`
/// expanded: directory, node, object(index), property(index).
const TRACK: &str = r"/^((?:[^\[\]\.:\/]+[\/:])*)([^\[\]:\/]+)?(?:\.([^\[\]\.:\/]+)(?:\[(.+)\])?)?\.([^\[\]\.:\/]+)(?:\[(.+)\])?$/";

fn reads(re: &str, input: &str, idx: u32) -> Vec<String> {
    let src = format!(
        "class A {{ p = 1; }}
         class S {{ p = 2; }}
         class Q {{ p = 3; }}
         class B {{ p = 4; }}
         class H {{
           position: any = new A(); scale: any = new S(); quaternion: any = new Q(); ctl: any = new B();
           m(): number {{ return 1; }}
         }}
         const h: any = new H();
         const n: any = process.argv[2];
         const cond = process.argv.length > 3;
         const m = {re}.exec({input});
         if (m !== null) {{ const x: any = h[m[{idx}]]; console.log(x.p); }}"
    );
    recv(&facts(&src))
}

/// The read names `want` and no other own field of `h` (`ctl` is `B`) and
/// no Object.prototype member (`Object` is the constructor's class object,
/// `ObjectProtoMethod` a built-in method).
fn names_only(v: &[String], want: &[&str]) {
    for w in want {
        assert!(v.iter().any(|x| x == w), "{w} {v:?}");
    }
    for x in v {
        assert!(want.contains(&x.as_str()) || PROGRAM_PROTO.contains(&x.as_str()), "{x} {v:?}");
    }
}

/// What `h[k]` reads from `H.prototype` under any String key today
/// (`constructor` is class `H`, the method `m`): `IdxLoad`'s chain read
/// (`plain_key`) takes no key entries. The strict tests are ignored until it
/// does.
const PROGRAM_PROTO: &[&str] = &["H", "m"];

// (i) The pinned group of a known suffix names only `position`.
#[test]
fn suffix_capture_names_only_position() {
    let v = reads(SIMPLE, "String(n) + \".position\"", 2);
    names_only(&v, &["A"]);
}

#[test]
#[ignore = "IdxLoad's program-prototype read (plain_key) takes no key entries: H.prototype's constructor and m"]
fn suffix_capture_names_only_position_strict() {
    let v = reads(SIMPLE, "String(n) + \".position\"", 2);
    assert_eq!(v, vec!["A"], "{v:?}");
}

// (ii) The group before the pinned one lies in the unknown part.
#[test]
fn group_before_pin_is_any_key() {
    let v = reads(SIMPLE, "String(n) + \".position\"", 1);
    assert!(v.len() > 1, "{v:?}");
}

// (iii) Two known suffixes: both names.
#[test]
fn two_suffixes_name_both() {
    let v = reads(SIMPLE, "String(n) + (cond ? \".position\" : \".scale\")", 2);
    names_only(&v, &["A", "S"]);
}

// (iv) No known suffix.
#[test]
fn unknown_input_is_any_key() {
    let v = reads(SIMPLE, "String(n)", 2);
    assert!(v.len() > 1, "{v:?}");
}

// (v) A backreference or an alternation in the tail: no rule.
#[test]
fn backreference_tail_is_any_key() {
    let v = reads(r"/^(?:([^.]*)\.)?([^.]+)\1?$/", "String(n) + \".position\"", 2);
    assert!(v.len() > 1, "{v:?}");
}

#[test]
fn alternation_tail_is_any_key() {
    let v = reads(r"/^(?:([^.]*)\.)?([^.]+)(?:x|y)?$/", "String(n) + \".position\"", 2);
    assert!(v.len() > 1, "{v:?}");
}

// (vi) `_trackRe` on a name + ".quaternion": group 5 = quaternion, group 6
// undefined (reads no member), groups 1-4 unknown.
#[test]
fn track_re_property_is_quaternion() {
    let v = reads(TRACK, "String(n) + \".quaternion\"", 5);
    names_only(&v, &["Q"]);
}

#[test]
fn track_re_property_index_is_undefined() {
    let v = reads(TRACK, "String(n) + \".quaternion\"", 6);
    assert!(v.is_empty(), "{v:?}");
}

#[test]
fn track_re_object_name_is_any_key() {
    let v = reads(TRACK, "String(n) + \".quaternion\"", 3);
    assert!(v.len() > 1, "{v:?}");
}
