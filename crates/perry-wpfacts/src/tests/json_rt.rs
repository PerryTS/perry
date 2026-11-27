//! JSON-RT: `JSON.parse(JSON.stringify(x))` with no replacer function and
//! no reviver is a fresh tree mirroring `x` (ECMA-262 25.5.2.2-25.5.2.5
//! SerializeJSONProperty / SerializeJSONObject / SerializeJSONArray: own
//! enumerable String keys, values after `toJSON`, `undefined`, functions
//! and symbols omitted in objects and `null` in arrays; 25.5.1 ParseJSON
//! creates exactly those properties). A key the source cannot hold is
//! absent from the copy and reads `undefined`; the source's objects never
//! reach the copy. three.js `Texture.copy` (three.core.js:7778
//! `this.userData = JSON.parse(JSON.stringify(source.userData))`): the
//! clone of `{}` has no keys, so `texture.isHTMLTexture` (three.module.js
//! 12275) is never true for it (DEADBR Route B).
//! Controls: a parse of a text of any other origin, and the round trip of
//! an unknown value, keep the any-JSON value.
use super::*;
use crate::model::prim;

const PRE: &str = "class M { go() { return 1; } }\n";

/// The values of `expr` at the end of `src`: receivers, primitives, ⊤.
fn probe(src: &str, expr: &str) -> (Vec<String>, u16, bool) {
    let pf = facts(&format!("{PRE}{src}\nconst __p: any = {expr};\n__p.probe();\n"));
    let v: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == "probe" && s.is_call)
        .collect();
    assert!(!v.is_empty(), "no .probe call\n{}", pf.dump_text());
    let mut r = Vec::new();
    let (mut p, mut top) = (0u16, false);
    for s in v {
        // Object receivers only (a primitive shows as `primitive:..`).
        r.extend(s.receivers.iter().filter(|x| !x.starts_with("primitive:")).cloned());
        p |= s.receiver_prims;
        top |= s.top;
    }
    eprintln!("JSONRT {expr}: recv {r:?} prims {} top {top}", prim::name(p));
    (r, p, top)
}

fn has_m(r: &[String]) -> bool {
    r.iter().any(|x| x.ends_with(":M"))
}

/// DEADBR fixture fx/deadbr1.ts: the userData clone reaches `up` as `t`.
#[test]
fn json_rt_userdata_clone_keeps_the_arm_dead() {
    let src = "class Img { go() { return 1; } }
class Tex {
  image = new Img();
  userData: any = {};
  clone(): Tex { const c = new Tex(); c.userData = JSON.parse(JSON.stringify(this.userData)); return c; }
}
function up(t: any) {
  const img = t.image;
  if (t.isHTMLTexture) {
    img.go();
  }
}
const a = new Tex();
const b = a.clone();
const all: any[] = [a, b, b.userData];
for (const t of all) up(t);
";
    let pf = facts(src);
    let v: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == "go" && s.is_call)
        .collect();
    assert!(!v.is_empty(), "no .go call\n{}", pf.dump_text());
    for s in v {
        eprintln!("JSONRT go: recv {:?} top {}", s.receivers, s.top);
        let img = s.receivers.iter().any(|r| r.ends_with(":Img"));
        assert!(!img && !s.top, "arm live: {:?}", s.receivers);
    }
}

#[test]
fn json_rt_absent_key_reads_only_undefined() {
    let (r, p, top) = probe(
        "const x = { a: { m: 1 }, n: \"s\" };\nconst c: any = JSON.parse(JSON.stringify(x));",
        "c.zz",
    );
    assert!(r.is_empty() && !top && p == prim::UNDEF, "{r:?} {}", prim::name(p));
}

#[test]
fn json_rt_nested_object_mirrors_keys() {
    let src = "const x = { a: { m: 1, k: new M() }, n: \"s\" };\nconst c: any = JSON.parse(JSON.stringify(x));";
    let (_, p, _) = probe(src, "c.a.m");
    assert!(p & prim::NUM != 0 && p & prim::STR == 0, "{}", prim::name(p));
    let (_, p, _) = probe(src, "c.n");
    assert!(p & prim::STR != 0 && p & prim::NUM == 0, "{}", prim::name(p));
    // The source's objects never reach the copy: `c.a.k` is a copy of an M.
    let (r, _, top) = probe(src, "c.a.k");
    assert!(!has_m(&r) && !top && !r.is_empty(), "{r:?}");
    // That copy has none of M's keys (M has no own key): `go` is absent.
    let (r, p, top) = probe(src, "c.a.k.zz");
    assert!(r.is_empty() && !top && p == prim::UNDEF, "{r:?} {}", prim::name(p));
}

#[test]
fn json_rt_function_undefined_symbol_values_are_omitted() {
    let src = "const x: any = { f: () => 1, u: undefined, s: Symbol(\"q\"), m: new M() };\nconst c: any = JSON.parse(JSON.stringify(x));";
    for k in ["c.f", "c.u", "c.s"] {
        let (r, p, top) = probe(src, k);
        assert!(r.is_empty() && !top && p == prim::UNDEF, "{k}: {r:?} {}", prim::name(p));
    }
    // In an array they are `null`.
    let src = "const x: any[] = [() => 1, undefined];\nconst c: any = JSON.parse(JSON.stringify(x));";
    let (r, p, top) = probe(src, "c[0]");
    assert!(r.is_empty() && !top && p & prim::NULL != 0 && p & prim::STR == 0, "{r:?} {}", prim::name(p));
}

#[test]
fn json_rt_to_json_result_is_mirrored() {
    let src = "class D { toJSON() { return { k: 5 }; } }\nconst x = { d: new D() };\nconst c: any = JSON.parse(JSON.stringify(x));";
    let (_, p, _) = probe(src, "c.d.k");
    assert!(p & prim::NUM != 0 && p & prim::STR == 0, "{}", prim::name(p));
    let (r, p, top) = probe(src, "c.d.zz");
    assert!(r.is_empty() && !top && p == prim::UNDEF, "{r:?} {}", prim::name(p));
}

#[test]
fn json_rt_any_key_store_keeps_field_any() {
    let src = "const x: any = {};\nx[process.argv[2]] = 7;\nconst c: any = JSON.parse(JSON.stringify(x));";
    let (r, p, top) = probe(src, "c.q");
    assert!(p & prim::NUM != 0 && p & prim::STR == 0 && r.is_empty() && !top, "{r:?} {}", prim::name(p));
}

#[test]
fn json_rt_late_key_replays() {
    let src = "const x: any = {};\nconst c: any = JSON.parse(JSON.stringify(x));\nx.late = \"v\";";
    let (_, p, _) = probe(src, "c.late");
    assert!(p & prim::STR != 0 && p & prim::NUM == 0, "{}", prim::name(p));
}

#[test]
fn json_rt_control_parse_of_text_is_any_json() {
    let (r, p, _) = probe("const c: any = JSON.parse(process.argv[2]);", "c.q");
    assert!(p & prim::STR != 0 && p & prim::NUM != 0 && !r.is_empty(), "{r:?} {}", prim::name(p));
}

#[test]
fn json_rt_control_unknown_value_is_any_json() {
    let src = "const x: any = JSON.parse(process.argv[2]);\nconst c: any = JSON.parse(JSON.stringify(x));";
    let (r, p, _) = probe(src, "c.q");
    assert!(p & prim::STR != 0 && p & prim::NUM != 0 && !r.is_empty(), "{r:?} {}", prim::name(p));
}
