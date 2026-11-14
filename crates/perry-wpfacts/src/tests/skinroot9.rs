//! SKINROOT9 (skin GLTFLoader.js:2345, `gltfExtensions[name] =
//! objectDef.extensions[name]` in `for (const name in ...)`): an any-key
//! store whose key may be `"__proto__"` gives its stored value to a JSON
//! receiver as a prototype (B.2.2.1.2). With the same key value in the read
//! and the store, the prototype is set only when the key is `"__proto__"`,
//! and then the value is `src["__proto__"]`, not `src[<any key>]`.
use super::*;

fn whole(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

fn kept(pf: &ProgramFacts) -> crate::derive::KindFact {
    let t = pf
        .templates
        .iter()
        .filter(|t| t.display.starts_with("t.ts"))
        .find(|t| t.fields.iter().any(|f| f.key == "keptv"))
        .unwrap_or_else(|| panic!("no keptv\n{}", pf.dump_text()));
    t.fields.iter().find(|f| f.key == "keptv").unwrap().kind.clone()
}

/// `src` (JSON) inherits `other` = { f: g, c: P }: `src[name]` finds g and
/// P (enumerable, through the chain); `g.tag` is a program function.
/// `body` is the loop body; dst's prototype is read into `r.keptv` (a
/// named read of a JSON object also reads what computed stores wrote under
/// other keys: the prototype is the exact observation).
fn prog(body: &str, pre: &str) -> String {
    format!(
        "const src: any = JSON.parse(process.argv[2]);
         const dst: any = JSON.parse(process.argv[3]);
         const k2: any = process.argv[4];
         class P {{ m() {{}} }}
         function g() {{}}
         (g as any).tag = function h(this: any) {{ (globalThis as any).leak = this; }};
         const other: any = {{ f: g, c: P }};
         Object.setPrototypeOf(src, other);
         {pre}
         for (const name in src) {{ {body} }}
         const r: any = {{ keptv: 0 }};
         r.keptv = Object.getPrototypeOf(dst);
         console.log(String(r.keptv));"
    )
}

/// The same key local in the read and the store: the prototype given
/// to dst is `src["__proto__"]` (a JSON value, or an enumerable member
/// named `__proto__` on the chain; `other` has none): never g or P, so
/// `dst.tag` finds no program function.
#[test]
fn skinroot9_same_key_store_gives_only_proto_named_value() {
    let pf = whole(&prog("dst[name] = src[name];", ""));
    let k = kept(&pf);
    assert!(!k.top && k.funcs == 0, "{k:?}\n{}", pf.dump_text());
}

/// The same pair through a local version (`const v = src[name]`).
#[test]
fn skinroot9_same_key_via_local() {
    let pf = whole(&prog("const v = src[name]; dst[name] = v;", ""));
    let k = kept(&pf);
    assert!(!k.top && k.funcs == 0, "{k:?}\n{}", pf.dump_text());
}

/// Stays: a different key (k2 may be "__proto__" while name is "f"): the
/// value is `src[k2]`, any member of the chain: g may become the
/// prototype, `dst.tag` may be h.
#[test]
fn keep_skinroot9_different_key_stays_wide() {
    let pf = whole(&prog("dst[name] = src[k2];", ""));
    let k = kept(&pf);
    assert!(k.top || k.funcs >= 1, "{k:?}\n{}", pf.dump_text());
}

/// Stays: an explicit `__proto__` store gives g.
#[test]
fn keep_skinroot9_explicit_proto_store() {
    let pf = whole(&prog("", "dst[\"__proto__\"] = g;"));
    let k = kept(&pf);
    assert!(k.top || k.funcs >= 1, "{k:?}\n{}", pf.dump_text());
}

/// Stays: an enumerable own data property named `__proto__` on `other`
/// (defineProperty) is what `src["__proto__"]` finds: g is given.
#[test]
fn keep_skinroot9_enumerable_proto_named_member() {
    let pf = whole(&prog(
        "dst[name] = src[name];",
        "Object.defineProperty(other, \"__proto__\", { value: g, enumerable: true });",
    ));
    let k = kept(&pf);
    assert!(k.top || k.funcs >= 1, "{k:?}\n{}", pf.dump_text());
}

/// Stays (DESIGN §2): `name` is rewritten between the read and the store:
/// two versions of the key, the read's key may be "f" while the store's is
/// "__proto__": g may become dst's prototype.
#[test]
fn keep_skinroot11_key_rewritten_between() {
    let pf = whole(&prog("const v = src[name]; name = k2; dst[name] = v;", "")
        .replace("for (const name in src)", "for (let name in src)"));
    let k = kept(&pf);
    assert!(k.top || k.funcs >= 1, "{k:?}\n{}", pf.dump_text());
}

/// Stays: a key that may be an object (a stateful `toString`): two
/// ToPropertyKey calls may give "f" then "__proto__".
#[test]
fn keep_skinroot11_object_key_stays_wide() {
    let pf = whole(&prog(
        "",
        "let n = 0; const ko: any = { toString() { n++; return n > 1 ? \"__proto__\" : \"f\"; } }; dst[ko] = src[ko];",
    ));
    let k = kept(&pf);
    assert!(k.top || k.funcs >= 1, "{k:?}\n{}", pf.dump_text());
}

/// Stays: `v` has two versions (rewritten to g on one path): the stored
/// value is not only `src[name]`.
#[test]
fn keep_skinroot11_value_two_versions() {
    let pf = whole(&prog("let v = src[name]; if (k2) v = g; dst[name] = v;", ""));
    let k = kept(&pf);
    assert!(k.top || k.funcs >= 1, "{k:?}\n{}", pf.dump_text());
}

fn whole_cfg(src: &str, cfg: &FactsConfig) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], cfg)
}

/// SKINROOT13 stays: the copy reads an ARRAY that is not the loop subject
/// (`arr[name]` is an ordinary read): its named read `arr["__proto__"]`
/// finds Object.prototype's accessor, which reads Array.prototype: that
/// may become dst's prototype.
#[test]
fn keep_skinroot13_array_ordinary_read_keeps_accessor() {
    let pf = whole(&prog("dst[name] = arr[name];", "const arr: any = [1, 2];"));
    let k = kept(&pf);
    assert!(
        k.top || k.builtin_objs >= 1 || k.objs.iter().any(|o| o.contains("Array")),
        "{k:?}\n{}",
        pf.dump_text()
    );
}

/// SKINROOT13 stays: with Rule R off (`PERRY_FACTS_FORIN_PATH=0`), a path
/// subject `o.s` is no enumerable read: the named read is the ordinary one,
/// accessor included, so src's prototype `other` may become dst's.
#[test]
fn keep_skinroot13_forin_path_off_is_wide() {
    let body = prog("dst[name] = o.s[name];", "const o: any = { s: src };")
        .replace("for (const name in src)", "for (const name in o.s)");
    let on = kept(&whole(&body));
    let off_pf = whole_cfg(&body, &FactsConfig { no_forin_path: true, ..FactsConfig::default() });
    let off = kept(&off_pf);
    assert!(!on.top && on.funcs == 0, "on {on:?}");
    assert!(
        off.top || off.objs.iter().any(|o| !on.objs.contains(o)),
        "on {on:?} off {off:?}\n{}",
        off_pf.dump_text()
    );
}

/// SKINROOT13 stays: a program object subject with a defineProperty'd
/// enumerable own data property `__proto__` holding g: for-in yields
/// "__proto__", `so["__proto__"]` reads g, and the copy gives g.
#[test]
fn keep_skinroot13_defined_enumerable_proto_is_given() {
    let pf = whole(&prog(
        "",
        "const so: any = { a: 1 }; Object.defineProperty(so, \"__proto__\", { value: g, enumerable: true }); for (const nm in so) { dst[nm] = so[nm]; }",
    ));
    let k = kept(&pf);
    assert!(k.top || k.funcs >= 1, "{k:?}\n{}", pf.dump_text());
}
