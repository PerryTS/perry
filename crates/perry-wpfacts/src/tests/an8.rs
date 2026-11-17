//! ANIM8 (2026-11-16): F-A on a PRIMITIVE subject. ECMA-262 14.7.5.6
//! ForIn/OfHeadEvaluation step 6 (null / undefined: no iteration; else
//! ToObject), 14.7.5.9 EnumerateObjectProperties (enumerable string keys,
//! own then inherited), 10.4.3 String exotic objects (index keys are
//! enumerable own one-character Strings; `length` is not enumerable).
//! Inside `for (k in s)` the read `s[k]` of a primitive `s` is a String's
//! character or a value the PROGRAM made enumerable on the kind's chain
//! (every built-in member is non-enumerable, clause 18), never a built-in
//! prototype member. RED: fxj (GLTFLoader `addUnknownExtensionsToUserData`
//! with a local subject) within 3x the work of the program without the
//! copy; a for-in over an argv String reads only Strings. KEEP: the
//! program-made enumerable members are read (Object.prototype,
//! String.prototype, a Number subject), a String-or-object subject, the
//! ANIM5 KEEPs. Unknowns from `process.argv`.
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
/// built-in function (the pollution: chain members copied as values).
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
    eprintln!("an8 work: without copy {w0} (held {h0}), with copy {w} (held {h})");
    assert!(!h, "realm held (work {w})");
    assert!(w <= 3 * w0.max(1), "work {w} > 3 x {w0}");
}

#[test]
fn an8_red_fxj_no_hold() {
    red(ADD_J);
}

const BOX: &str = "const h: any = { box };\nh.m = dst[process.argv[4]];\nh.m(\"_\");\nconsole.log(box.witness);\n";

/// KEEP: `delete o[other]` in the body with an inherited non-enumerable
/// member of the same name on a program prototype: the read sees it.
#[test]
fn an8_keep_delete_reads_inherited() {
    let body = format!("const P: any = {{ m(this: any) {{ ext(this); }} }};\nObject.defineProperty(P, \"m\", {{ enumerable: false }});\nconst holder: any = {{ o: Object.create(P) }};\nholder.o.m = 1; holder.o.z = 2;\nconst dst: any = {{}};\nfor (const k in holder.o) {{ delete holder.o[\"m\"]; dst[k] = holder.o[k]; }}\n{BOX}");
    assert!(!witness(SETUP_ARGV, &body).0);
}

/// KEEP: `dst === o` chosen by argv; and a dst that may be the holder,
/// with a store replacing the link.
#[test]
fn an8_keep_dst_is_subject() {
    let body = format!("const holder: any = {{ o: {{ a: 1 }} }};\nconst dst: any = process.argv[3] ? holder.o : {{}};\nfor (const k in holder.o) dst[k] = holder.o[k];\n{BOX}");
    let (ok, d) = witness(SETUP_ARGV, &body);
    assert!(ok, "{d}");
    let body2 = format!("const X: any = {{ f: function (this: any) {{ ext(this); }} }};\nconst holder: any = {{ o: {{ o: X, f: 0 }} }};\nconst dst: any = process.argv[3] ? holder : {{}};\nfor (const k in holder.o) dst[k] = holder.o[k];\n{BOX}");
    assert!(
        !witness(SETUP_ARGV, &body2).0,
        "a store replacing the link stays watched"
    );
}

/// KEEP: dst is the subject's prototype (`Object.create( dst )`).
#[test]
fn an8_keep_dst_is_prototype() {
    let body = format!("const dst: any = {{}};\nconst holder: any = {{ o: Object.create(dst) }};\nholder.o.a = 1;\nfor (const k in holder.o) dst[k] = holder.o[k];\ndst.f = function (this: any) {{ ext(this); }};\n{BOX}");
    assert!(!witness(SETUP_ARGV, &body).0);
}

/// KEEP: a setter on dst that deletes from o.
#[test]
fn an8_keep_setter_deletes() {
    let body = format!("const p: any = {{ b(this: any) {{ ext(this); }} }};\nconst holder: any = {{ o: Object.create(p) }};\nholder.o.a = 1; holder.o.b = 2;\nObject.defineProperty(p, \"b\", {{ enumerable: false }});\nconst dst: any = {{ set a(v: any) {{ delete holder.o.b; }} }};\nfor (const k in holder.o) dst[k] = holder.o[k];\n{BOX}");
    assert!(!witness(SETUP_ARGV, &body).0);
}

/// KEEP: the body calls an unknown function.
#[test]
fn an8_keep_body_calls_unknown() {
    let body = format!("const holder: any = {{ o: {{ a: 1 }} }};\nconst u: any = new Function(process.argv[5]);\nconst dst: any = {{}};\nfor (const k in holder.o) {{ u(holder); dst[k] = holder.o[k]; }}\n{BOX}");
    assert!(!witness(SETUP_ARGV, &body).0);
}

/// KEEP: o is a Proxy.
#[test]
fn an8_keep_proxy_subject() {
    let body = format!("const holder: any = {{ o: new Proxy({{ a: 1 }}, {{ get(t: any, k: any) {{ return function (this: any) {{ ext(this); }}; }} }}) }};\nconst dst: any = {{}};\nfor (const k in holder.o) dst[k] = holder.o[k];\n{BOX}");
    assert!(!witness(SETUP_ARGV, &body).0);
}

/// KEEP: o is an Array (index keys + a program-added enumerable property).
#[test]
fn an8_keep_array_subject() {
    let body = format!("const holder: any = {{ o: [1, 2] }};\nholder.o.f = function (this: any) {{ ext(this); }};\nconst dst: any = {{}};\nfor (const k in holder.o) dst[k] = holder.o[k];\n{BOX}");
    assert!(!witness(SETUP_ARGV, &body).0);
}

/// KEEP: an enumerable key the program added to `Object.prototype`: the
/// for-in visits it and the read sees its value.
#[test]
fn an8_keep_object_prototype_enumerable() {
    let body = format!("(Object.prototype as any).f = function (this: any) {{ ext(this); }};\nconst holder: any = {{ o: {{ a: 1 }} }};\nconst dst: any = {{}};\nfor (const k in holder.o) dst[k] = holder.o[k];\n{BOX}");
    assert!(!witness(SETUP_ARGV, &body).0);
}

/// AN7 measurement: sharper KEEP for the holder meet: the replaced link
/// reaches a NON-enumerable inherited accessor (missed by an enumerable read).
#[test]
fn an8_keep_sharp_holder() {
    let body = format!("const P: any = {{}};\nObject.defineProperty(P, \"f\", {{ value: function (this: any) {{ ext(this); }}, enumerable: false }});\nconst X: any = Object.create(P);\nconst holder: any = {{ o: {{ o: X, f: 0 }} }};\nconst dst: any = process.argv[3] ? holder : {{}};\nfor (const k in holder.o) dst[k] = holder.o[k];\n{BOX}");
    assert!(
        !witness(SETUP_ARGV, &body).0,
        "the getter reached through the replaced link stays seen"
    );
}

/// The values of the any-key field of every program object (and its
/// primitive mask): `o[q] = s[q]` stores the for-in read there.
fn copied(src: &str) -> (usize, u16) {
    let mut g = solved(src);
    let m = &mut g.m;
    let objs: Vec<u32> = (0..m.toks.len() as u32)
        .filter(|&t| {
            matches!(&m.toks[t as usize], TokKind::Obj(tm, _)
                if matches!(m.tmpls[*tm as usize].kind, TmplKind::Lit { .. }))
        })
        .collect();
    let (mut n, mut p) = (0, 0u16);
    for t in objs {
        let fa = m.field_any(t);
        n += m.nodes[fa as usize].pts.to_sorted().len();
        p |= m.nodes[fa as usize].prim;
    }
    // A member of a primitive kind's prototype (the read rule's question).
    let kinds = [
        "builtin-fn:String.",
        "builtin-fn:Number.",
        "builtin-fn:Boolean.",
        "builtin-fn:BigInt.",
        "builtin-fn:Symbol.",
    ];
    let bf = (0..m.toks.len() as u32)
        .filter(|&t| matches!(m.toks[t as usize], TokKind::BuiltinFn(..)))
        .filter(|&t| {
            let s = m.tok_name(t);
            kinds.iter().any(|k| s.starts_with(k))
        })
        .count();
    eprintln!("an8 copied: values {n} prim {p:#x} builtin fns {bf}");
    (n + bf, p)
}

/// `copied` of `src` minus that of the same program without the loop: the
/// values the for-in read adds (0: nothing but Strings / undefined).
fn loop_adds(src: &str) -> (usize, u16) {
    let (n, p) = copied(src);
    let (n0, _) = copied(&src.replace("  for (const q in s) o[q] = s[q];\n", ""));
    (n.saturating_sub(n0), p)
}

/// RED: a for-in over an argv String reads its characters only (10.4.3:
/// index keys; `length` and every String.prototype member are not
/// enumerable).
#[test]
fn an8_red_argv_string_reads_strings() {
    let src = "function cp(s: any) {\n  const o: any = {};\n  for (const q in s) o[q] = s[q];\n  return o;\n}\nconst o: any = cp(process.argv[2]);\nconsole.log(o[process.argv[3]]);\n";
    let (n, p) = loop_adds(src);
    assert_eq!(n, 0, "a value other than a String was read");
    let other = p & !(crate::model::prim::STR | crate::model::prim::UNDEF);
    assert_eq!(other, 0, "prim {p:#x}");
}

/// RED: a Number / Boolean subject has no enumerable own key and no
/// enumerable built-in inherited one: nothing is read.
#[test]
fn an8_red_number_subject_reads_nothing() {
    let src = "function cp(s: any) {\n  const o: any = {};\n  for (const q in s) o[q] = s[q];\n  return o;\n}\nconst o: any = cp(process.argv[2] ? 5 : true);\nconsole.log(o[process.argv[3]]);\n";
    let (n, p) = loop_adds(src);
    assert_eq!(n, 0);
    assert_eq!(p & !crate::model::prim::UNDEF, 0, "prim {p:#x}");
}

const F: &str = "function (this: any) { ext(this); }";
const CALL: &str = "const h: any = { box };\nh.m = r;\nh.m(\"_\");\nconsole.log(box.witness);\n";

/// Whether `r = s[q]` inside `for (q in s)` may read the function `F`.
fn reads(pre: &str, subj: &str) -> bool {
    let body =
        format!("{pre}\nconst s: any = {subj};\nlet r: any;\nfor (const q in s) r = s[q];\n{CALL}");
    !witness(SETUP_ARGV, &body).0
}

/// KEEP: an enumerable key the program put on Object.prototype is visited
/// on a String and its value read.
#[test]
fn an8_keep_string_reads_object_prototype_enumerable() {
    assert!(reads(
        &format!("(Object.prototype as any).z = {F};"),
        "\"ab\""
    ));
}

/// KEEP: one put on String.prototype.
#[test]
fn an8_keep_string_reads_string_prototype_enumerable() {
    assert!(reads(
        &format!("(String.prototype as any).q = {F};"),
        "\"ab\""
    ));
}

/// KEEP: a Number subject visits the program-made enumerable keys.
#[test]
fn an8_keep_number_reads_object_prototype_enumerable() {
    assert!(reads(&format!("(Object.prototype as any).z = {F};"), "5"));
}

/// KEEP: a subject that is a String or an object by argv.
#[test]
fn an8_keep_string_or_object_subject() {
    assert!(reads(
        "",
        &format!("process.argv[3] ? \"ab\" : {{ f: {F} }}")
    ));
}

/// A NON-enumerable program-defined prototype member: the String subject
/// reads what an object subject reads (the same inherited-key source).
#[test]
fn an8_keep_non_enumerable_definition_as_object() {
    for proto in ["String", "Object"] {
        let pre = format!(
            "Object.defineProperty({proto}.prototype, \"h\", {{ value: {F}, enumerable: false }});"
        );
        let (rs, ro) = (reads(&pre, "\"ab\""), reads(&pre, "{ a: 1 }"));
        eprintln!("an8 non-enumerable {proto}.prototype.h: string reads {rs}, object reads {ro}");
        assert_eq!(rs, ro);
    }
}
