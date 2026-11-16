//! SKINROOT19/20: the enumerable-key read of `"__proto__"` of a for-in
//! same-key copy (`for (k in s) d[k] = s[k]`, `Cons::EnumLoad`) over a
//! typed array reads only a `"__proto__"` property the program made
//! enumerable on the array or on its chain (ECMA-262 14.7.5.9: for-in yields
//! enumerable String keys; 10.4.5.6 a typed array's own keys are its indices
//! and what the program added; clause 18 and 20.1.3.8: no built-in property of
//! `%TypedArray%.prototype`, its kind prototypes or `Object.prototype`, the
//! `__proto__` accessor among them, is enumerable). It never runs the
//! `Object.prototype.__proto__` accessor, so `d` never gets
//! `%TypedArray%.prototype` as a prototype.
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

fn m_mono(pf: &ProgramFacts) -> Vec<String> {
    let mut s: Vec<_> = pf.sites.iter().filter(|s| s.key == "m" && s.is_call).collect();
    s.sort_by_key(|s| s.byte_offset);
    let v: Vec<String> = s.iter().map(|s| format!("{:?}", s.verdict)).collect();
    assert!(!v.is_empty(), "no call site of m");
    v
}

const OBJ: &str = "const o = { m() { return 1; } };\n";

/// The copy from a typed array (or a parsed object) into a parsed object:
/// `d.indexOf` is no `%TypedArray%` member, the realm is not held, and
/// `o.m()` stays the program body.
#[test]
fn skinroot19_typed_array_enum_proto_not_accessor() {
    let src = format!(
        "{OBJ}function copy(def: any, d: any) {{
           for (const k in def.ext) d[k] = def.ext[k];
         }}
         const def: any = JSON.parse(process.argv[2]);
         if (process.argv.length > 4) def.ext = new Float32Array(4);
         const d: any = JSON.parse(process.argv[3]);
         copy(def, d);
         const r: any = {{ keptv: 0 }};
         r.keptv = d.subarray;
         d.indexOf(1);
         const x = o.m();
         console.log(String(r.keptv), x);"
    );
    let pf = whole(&src);
    let k = kept(&pf);
    eprintln!("SKINROOT19 keptv {k:?}");
    // Without the rule `d` gets `%TypedArray%.prototype` and, through its
    // members, ⊤ under `d.subarray`.
    assert!(!k.top, "RED: d.subarray reaches ⊤ through %TypedArray%.prototype: {k:?}");
    let v = m_mono(&pf);
    assert!(
        v.iter().any(|x| x == "T0Mono" || x.starts_with("T0Mono(")),
        "RED: realm held through d's typed-array prototype: {v:?} keptv {k:?}"
    );
}

/// (k1) An own enumerable `"__proto__"` the program defined on the typed
/// array: the copy gives its value P.
#[test]
fn keep_skinroot19_defined_enumerable_proto_on_typed_array() {
    let src = "const P: any = { tag: function fa() {} };
         const s: any = new Float32Array(4);
         Object.defineProperty(s, \"__proto__\", { value: P, enumerable: true });
         const d: any = {};
         for (const k in s) d[k] = s[k];
         const r: any = { keptv: 0 };
         r.keptv = d.tag;
         console.log(String(r.keptv));";
    let pf = whole(src);
    let k = kept(&pf);
    assert!(k.top || k.funcs >= 1, "{k:?}\n{}", pf.dump_text());
}

/// (k2) A parsed document with an own `"__proto__"` key: its JSON value is
/// given to `d` (CreateDataProperty, then the setter of `d["__proto__"]`).
#[test]
fn keep_skinroot19_json_proto_key_given() {
    let src = "const s: any = JSON.parse(process.argv[2]);
         const d: any = {};
         for (const k in s) d[k] = s[k];
         const r: any = { keptv: 0 };
         r.keptv = d.tag;
         console.log(String(r.keptv));";
    let pf = whole(src);
    let k = kept(&pf);
    assert!(
        k.top || !k.objs.is_empty() || k.prims & !crate::model::prim::UNDEF != 0,
        "{k:?}\n{}",
        pf.dump_text()
    );
}

/// (k3) A plain `ta.__proto__` read (not an enumerable-key read) still runs
/// the accessor: the typed array's prototype.
#[test]
fn keep_skinroot19_plain_proto_read_typed_array() {
    let src = "const ta: any = new Float32Array(4);
         const r: any = { keptv: 0 };
         r.keptv = ta.__proto__;
         console.log(String(r.keptv));";
    let pf = whole(src);
    let k = kept(&pf);
    assert!(k.top || k.builtin_objs >= 1, "{k:?}\n{}", pf.dump_text());
}

/// (k4) A program getter made enumerable under `"__proto__"` on
/// `Float32Array.prototype`: for-in yields the key and `s[k]` calls the
/// program getter, whose value P becomes d's prototype.
#[test]
fn keep_skinroot19_program_enumerable_getter_on_kind_prototype() {
    let src = "const P: any = { tag: function fa() {} };
         Object.defineProperty(Float32Array.prototype, \"__proto__\", { enumerable: true, get() { return P; } });
         const s: any = new Float32Array(4);
         const d: any = {};
         for (const k in s) d[k] = s[k];
         const r: any = { keptv: 0 };
         r.keptv = d.tag;
         console.log(String(r.keptv));";
    let pf = whole(src);
    let k = kept(&pf);
    assert!(k.top || k.funcs >= 1, "{k:?}\n{}", pf.dump_text());
}
