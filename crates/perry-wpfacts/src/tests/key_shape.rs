//! A computed key built from literal parts (`model/key_shape.rs`) names only
//! members whose name matches them: `json[type + "s"]` is never
//! `constructor`, so it never reads the Object constructor, whose absent
//! `map` would give its arguments to unknown code; a store
//! `o[prefix + "_x"] = v` never writes `o.name`. A key whose shape admits
//! `constructor`, may be a number, has no literal part or reaches the
//! access through a variable stays any key.

use super::*;

/// The verdicts of the reads of `.yy` (in the callback, or on `o.name`).
fn yy_reads(pf: &ProgramFacts) -> Vec<Verdict> {
    pf.sites
        .iter()
        .filter(|s| s.key == "yy" && !s.is_call)
        .map(|s| s.verdict.clone())
        .collect()
}

fn yy_proven(pf: &ProgramFacts) -> bool {
    let v = yy_reads(pf);
    !v.is_empty()
        && v.iter()
            .all(|v| matches!(v, Verdict::T0Mono | Verdict::T0Set))
}

/// GLTFLoader's `this.json[type + "s"]`: the callback's argument is a `D`.
const JSON_MAP: &str = r#"
    class D { yy = 2; }
    const d = new D();
    const json: any = { meshes: [d], nodes: [d] };
    const type = String(Math.random());
    const a = String(Math.random());
    const b = String(Math.random());
    PRE
    json[KEY].map((x: any) => console.log(x.yy));
"#;

/// GLTFRegistry's `objects[type + ":" + index]`.
const REGISTRY: &str = r#"
    class D { yy = 2; }
    const d = new D();
    const reg: any = {};
    reg["mesh:0"] = [d];
    const type = String(Math.random());
    const i = Math.floor(Math.random() * 3);
    const a = String(Math.random());
    PRE
    reg[KEY].forEach((x: any) => console.log(x.yy));
"#;

/// A shaped store next to a named field.
const STORE: &str = r#"
    class D { yy = 2; }
    const d = new D();
    const o: any = { name: d };
    const prefix = String(Math.random());
    const a = String(Math.random());
    const v = JSON.parse(String(Math.random()));
    PRE
    o[KEY] = v;
    console.log(o.name.yy);
"#;

fn run(p: &str, pre: &str, key: &str) -> ProgramFacts {
    facts(&p.replace("PRE", pre).replace("KEY", key))
}

#[test]
fn a_literal_suffix_key_reads_no_constructor() {
    for key in [r#"type + "s""#, "`${type}s`", r#""m" + type + "s""#] {
        let pf = run(JSON_MAP, "", key);
        assert!(
            yy_proven(&pf),
            "{key}: {:?}\n{}",
            yy_reads(&pf),
            pf.dump_text()
        );
    }
}

#[test]
fn a_registry_key_with_a_separator_reads_no_constructor() {
    for key in [r#"type + ":" + i"#, "`${type}:${i}`"] {
        let pf = run(REGISTRY, "", key);
        assert!(
            yy_proven(&pf),
            "{key}: {:?}\n{}",
            yy_reads(&pf),
            pf.dump_text()
        );
    }
}

#[test]
fn a_shaped_store_writes_no_other_name() {
    for key in [r#"prefix + "_x""#, "`${prefix}_x`"] {
        let pf = run(STORE, "", key);
        assert!(
            yy_proven(&pf),
            "{key}: {:?}\n{}",
            yy_reads(&pf),
            pf.dump_text()
        );
    }
}

/// Sabotage: shapes that admit `constructor`, a number, no literal part,
/// two non-literals, or a key held in a variable stay any key.
#[test]
fn keys_that_may_be_constructor_stay_any_key() {
    let any = [
        ("", r#""con" + type"#),
        ("", r#"type + "r""#),
        // `*s*`: "constructor" holds an "s".
        ("", r#"type + "s" + a"#),
        ("", r#"type + """#),
        ("", "`${type}${a}`"),
        ("", "type + a"),
    ];
    for (pre, key) in any {
        let pf = run(JSON_MAP, pre, key);
        // A missing Object.map no longer escapes the callback. Inspect
        // the receiver identity to test that the key still admits it.
        let calls: Vec<_> = pf
            .sites
            .iter()
            .filter(|s| s.is_call && s.key == "map")
            .collect();
        assert!(!calls.is_empty(), "missing map site");
        assert!(
            calls
                .iter()
                .any(|s| s.receivers.iter().any(|r| r == "builtin:Object")),
            "{key}: {}",
            pf.dump_text()
        );
    }
}

/// A key `"1" + type` has the exact shape `1*` (a string that begins with
/// `1`, any rest: "1NaN", "1Infinity", "1-5", "11e+21" included): no
/// Object.prototype member name begins with a digit, so it never reads the
/// Object constructor. The same shape through a variable (`const k = type +
/// "s"`, shape `*s`) holds, `constructor` ends with `r`.
#[test]
fn a_key_with_a_literal_edge_through_a_variable_reads_no_constructor() {
    use crate::model::KeyShape;
    let one = KeyShape {
        globs: vec![vec!["1".to_string(), String::new()]],
        free: Vec::new(),
    };
    for s in ["1NaN", "1Infinity", "1-5", "11e+21", "10.5"] {
        assert!(one.admits(s), "{s}");
    }
    assert!(!one.admits("constructor"));
    for (pre, key) in [("", r#""1" + type"#), ("const k = type + \"s\";", "k")] {
        let pf = run(JSON_MAP, pre, key);
        let objs = pf
            .sites
            .iter()
            .filter(|s| s.is_call && s.key == "map")
            .any(|s| s.receivers.iter().any(|r| r == "builtin:Object"));
        assert!(!objs, "{key}: {}", pf.dump_text());
    }
}

/// Sabotage: a store whose shape admits `name` (or any key) reaches it.
#[test]
fn a_store_whose_shape_admits_the_name_reaches_it() {
    let any = [
        ("", r#""na" + prefix"#),
        ("", r#"prefix + "me""#),
        ("", "`${prefix}${a}`"),
    ];
    for (pre, key) in any {
        let pf = run(STORE, pre, key);
        assert!(!yy_proven(&pf), "{key}: {:?}", yy_reads(&pf));
    }
}

/// `prefix + "_x"` (held in a variable too) ends with `_x`: it never writes
/// `name`.
#[test]
fn a_variable_key_ending_in_a_literal_writes_no_other_name() {
    let pf = run(STORE, "const k = prefix + \"_x\";", "k");
    assert!(yy_proven(&pf), "{:?}", yy_reads(&pf));
}

#[test]
fn shape_matching_is_exact() {
    use crate::model::KeyShape;
    let s = |p: &[&str]| KeyShape {
        globs: vec![p.iter().map(|s| s.to_string()).collect()], free: Vec::new()
    };
    assert!(s(&["", "s"]).admits("meshes"));
    assert!(!s(&["", "s"]).admits("constructor"));
    assert!(s(&["con", ""]).admits("constructor"));
    assert!(s(&["", ":", ""]).admits("mesh:0"));
    assert!(!s(&["", ":", ""]).admits("constructor"));
    assert!(s(&["a", "b", "c"]).admits("abc"));
    assert!(!s(&["ab", "b"]).admits("ab"));
    assert!(s(&["", "o", "o", ""]).admits("foo"));
    assert!(!s(&["", "o", "o", ""]).admits("fo"));
}
