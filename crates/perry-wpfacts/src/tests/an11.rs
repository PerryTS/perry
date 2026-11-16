//! ANIM11 (2026-11-16): the for-in holder meet on `JSON.parse` trees.
//! F-A (`Gen::path_watch`) keeps `P[k]` of `for (k in P)`, `P = r.f1..fn`,
//! the enumerable read of the enumerated object only while no body store
//! under the loop key may hit a holder `r.f1..f(i-1)` (`Cons::ForInMeet`).
//! ECMA-262 25.5.1 ParseJSON makes every object of the tree fresh and
//! returns only the result, so the result object is no value inside its
//! own tree (`Model::json_inner`): a store into a nested parsed object
//! (GLTFLoader `addUnknownExtensionsToUserData`, fixture `fxi`) never meets
//! the holder `def` that is the parse result. RED: fxi / fxj without a
//! realm hold and within 1.5x the solve work of the program without the
//! copy. KEEP: a program store into a parsed tree, two parses whose
//! objects meet in one array, an `Object.assign` getter, the replaced
//! link to an Array (`holder.o = { o: [], push: 0 }`). Unknowns from
//! `process.argv`.
use super::box_computed_read::{run as witness, SETUP_ARGV};
use crate::model::{TmplKind, TokKind};

fn solved(src: &str) -> crate::gen::Gen<'static> {
    let src: &'static str = Box::leak(src.to_string().into_boxed_str());
    let ast = Box::leak(Box::new(
        perry_parser::parse_typescript(src, "t.ts").unwrap(),
    ));
    let hir = Box::leak(Box::new(perry_hir::lower_module(ast, "t", "t.ts").unwrap()));
    let mut g = crate::gen::Gen::new(vec![("t.ts", &*hir)], false, false);
    g.m.max_work = 5_000_000;
    g.register_all();
    g.link_exports();
    g.walk_all();
    g.feed_parent_values();
    g.m.summarize_and_replay();
    g.m.solve();
    g
}

/// (realm held, solve work)
fn held_work(src: &str) -> (bool, u64) {
    let g = solved(src);
    (g.m.realm_held, g.m.work.get())
}

/// Whether the any-field of a program literal or a JSON object holds a
/// built-in function (a chain member copied as a value).
fn copies_builtin_fn(src: &str) -> bool {
    let mut g = solved(src);
    let m = &mut g.m;
    let objs: Vec<u32> = (0..m.toks.len() as u32)
        .filter(|&t| match &m.toks[t as usize] {
            TokKind::Obj(tm, _) => matches!(
                m.tmpls[*tm as usize].kind,
                TmplKind::Lit { .. } | TmplKind::BObj { what: "json", .. }
            ),
            _ => false,
        })
        .collect();
    objs.into_iter().any(|t| {
        let fa = m.field_any(t);
        m.nodes[fa as usize]
            .pts
            .to_sorted()
            .iter()
            .any(|&v| matches!(m.toks[v as usize], TokKind::BuiltinFn(..)))
    })
}

/// Whether some value set holds `Array.prototype.push` (read through the
/// replaced link: the program names `push` only as a key of a literal).
fn reaches_push(src: &str) -> bool {
    let g = solved(src);
    let m = &g.m;
    let names: std::collections::BTreeSet<String> = m
        .nodes
        .iter()
        .flat_map(|n| n.pts.to_sorted())
        .filter(|&t| matches!(m.toks[t as usize], TokKind::BuiltinFn(..)))
        .map(|t| m.tok_name(t))
        .collect();
    eprintln!("an11 builtin fns: {names:?}");
    names
        .iter()
        .any(|s| s.contains("push") || s.contains("Array.prototype[*]"))
}

const HEAD: &str = r#"const text: string = process.argv[2] ?? "{\"asset\":{\"version\":\"2.0\"},\"extras\":{\"a\":1},\"extensions\":{\"X\":{\"b\":2}}}";
const json: any = JSON.parse(text);
"#;

const ADD_I: &str = r#"function addUnknown(known: any, object: any, def: any) {
  for (const name in def.extensions) {
    if (known[name] === undefined) {
      object.userData.gltfExtensions = object.userData.gltfExtensions || {};
      object.userData.gltfExtensions[name] = def.extensions[name];
    }
  }
}
"#;

const ADD_J: &str = r#"function addUnknown(known: any, object: any, def: any) {
  const exts = def.extensions;
  for (const name in exts) {
    if (known[name] === undefined) {
      object.userData.gltfExtensions = object.userData.gltfExtensions || {};
      object.userData.gltfExtensions[name] = exts[name];
    }
  }
}
"#;

const ADD_NONE: &str = "function addUnknown(known: any, object: any, def: any) {}\n";

const REST: &str = r#"function assignExtras(object: any, def: any) {
  if (def.extras !== undefined) {
    if (typeof def.extras === "object") Object.assign(object.userData, def.extras);
  }
}
const result: any = { asset: json.asset, userData: {} };
addUnknown({}, result, json);
assignExtras(result, json);
console.log(JSON.stringify(result.userData), result.asset.version);
"#;

fn prog(add: &str) -> String {
    format!("{HEAD}{add}{REST}")
}

fn red(add: &str) {
    let (h0, w0) = held_work(&prog(ADD_NONE));
    let (h, w) = held_work(&prog(add));
    eprintln!("an11 work: without copy {w0} (held {h0}), with copy {w} (held {h})");
    assert!(!h, "realm held (work {w})");
    assert!(2 * w <= 3 * w0.max(1), "work {w} > 1.5 x {w0}");
}

#[test]
fn an11_red_fxi_no_hold() {
    red(ADD_I);
}

#[test]
fn an11_red_fxj_no_hold() {
    red(ADD_J);
}

const BOX: &str = "const h: any = { box };\nh.m = dst[process.argv[4]];\nh.m(\"_\");\nconsole.log(box.witness);\n";
const X: &str = "const X: any = { f: function (this: any) { ext(this); } };\n";

/// KEEP: the program stores an object into the parsed tree (`j.a.a = X`)
/// and the body's store may hit the parse result (`dst === j`): the key
/// `a` replaces the link and the next read reaches `X.f`.
#[test]
fn an11_keep_program_store_into_parsed_tree() {
    let body = format!("{X}const j: any = JSON.parse(\"\" + k);\nj.a.a = X;\nconst dst: any = process.argv[3] ? j : {{}};\nfor (const kk in j.a) dst[kk] = j.a[kk];\n{BOX}");
    assert!(!witness(SETUP_ARGV, &body).0);
    // The same with the holder a nested parsed object.
    let body2 = format!("{X}const j: any = JSON.parse(\"\" + k);\nj.b.a.a = X;\nconst dst: any = process.argv[3] ? j.b : {{}};\nconst r: any = j.b;\nfor (const kk in r.a) dst[kk] = r.a[kk];\n{BOX}");
    assert!(!witness(SETUP_ARGV, &body2).0);
}

/// KEEP: objects of two parses meet in one program array; the holder is
/// either one.
#[test]
fn an11_keep_two_parses_meet_in_one_array() {
    let body = format!("{X}const j1: any = JSON.parse(\"\" + k);\nconst j2: any = JSON.parse(process.argv[5] ?? \"{{}}\");\nj2.x.a.a = X;\nconst arr: any[] = [j1, j2.x];\nconst holder: any = arr[process.argv[6] ? 0 : 1];\nconst dst: any = process.argv[3] ? holder : {{}};\nfor (const kk in holder.a) dst[kk] = holder.a[kk];\n{BOX}");
    assert!(!witness(SETUP_ARGV, &body).0);
}

/// KEEP: `Object.assign` with a getter on the source (20.1.2.1 step
/// 3.a.iii.2.a: [[Get]] runs the getter) puts `X` into the parsed tree.
#[test]
fn an11_keep_object_assign_getter_source() {
    let body = format!("{X}const j: any = JSON.parse(\"\" + k);\nObject.assign(j.a, {{ get a() {{ return X; }} }});\nconst dst: any = process.argv[3] ? j : {{}};\nfor (const kk in j.a) dst[kk] = j.a[kk];\n{BOX}");
    assert!(!witness(SETUP_ARGV, &body).0);
}

/// KEEP (sharp): the replaced link is an Array (`holder.o = { o: [],
/// push: 0 }`, `dst === holder` by argv): the next read `holder.o.push`
/// is `Array.prototype.push`, non-enumerable and inherited: the read
/// stays the ordinary one. On a program literal and on a parse result.
#[test]
fn an11_keep_sharp_array_link() {
    let lit = "const k: string = process.argv[2];\nconst holder: any = { o: { o: [], push: 0 } };\nconst dst: any = process.argv[3] ? holder : {};\nfor (const kk in holder.o) dst[kk] = holder.o[kk];\nconsole.log(dst[k]);\n";
    assert!(reaches_push(lit), "{lit}");
    let js = "const k: string = process.argv[2];\nconst j: any = JSON.parse(\"\" + k);\nj.o = { o: [], push: 0 };\nconst dst: any = process.argv[3] ? j : {};\nfor (const kk in j.o) dst[kk] = j.o[kk];\nconsole.log(dst[k]);\n";
    assert!(reaches_push(js), "{js}");
}

/// KEEP: with a reviver the result is the value of the wrapper's `""` key
/// (25.5.1 step 4: `this[""]` in the reviver is the result) and every
/// object of the tree is the reviver's `this`: a copy over a member of the
/// result into the reviver's holder may replace the link.
#[test]
fn an11_keep_reviver_holder() {
    let body = format!("{X}let hold: any = {{}};\nconst j: any = JSON.parse(\"\" + k, function (this: any, key: any, v: any) {{ if (key === \"\") hold = this[\"\"]; return key === \"a\" ? {{ a: X, f: 0 }} : v; }});\nconst dst: any = process.argv[3] ? hold : {{}};\nfor (const kk in j.a) dst[kk] = j.a[kk];\n{BOX}");
    assert!(!witness(SETUP_ARGV, &body).0);
}
