//! SKINDES3 fixtures (three.js GLTFLoader.js GLTFRegistry, 579-600, with
//! `getDependency`'s `type + ':' + index` (2929) and `getDependencies`'
//! `this.cache.get( type )` (3037)). At run time `get( 'scene' )` returns
//! only what `add( 'scene', ... )` stored, and `t + ':' + i` always holds
//! ':' (13.15.3), so it never equals 'scene'. Each fixture reads `.q` on
//! the value got under one key; the verdict is the receiver classes.
//! r* = the exact answer (RED where the solver merges), k* = KEEP (wide).
use super::*;

/// Receiver classes of every non-call `.q` read, in program order.
fn reads(pf: &ProgramFacts) -> Vec<Vec<String>> {
    let mut s: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == "q" && !s.is_call)
        .collect();
    s.sort_by_key(|s| s.byte_offset);
    s.iter()
        .map(|s| {
            let mut r: Vec<String> = s
                .receivers
                .iter()
                .map(|r| r.rsplit(':').next().unwrap_or(r).to_string())
                .collect();
            // Flow-insensitive: a read of a key may also see the object
            // before its store (absent: undefined); that is not the merge.
            r.retain(|c| !c.starts_with("undefined"));
            r.sort();
            r.dedup();
            r
        })
        .collect()
}

const CLS: &str = "class S { q = 1; } class N { q = 2; } class Z { q = 3; }
                   const n = Number(process.argv[2]);\n";

const REG: &str = "function R() { let o: any = {}; return {
                     get: function (k: string) { return o[k]; },
                     add: function (k: string, v: any) { o[k] = v; } }; }\n";

fn run(tag: &str, body: &str) -> Vec<Vec<String>> {
    let src = format!("{CLS}{REG}{body}");
    let got = reads(&facts(&src));
    eprintln!("SKINROOT39 {tag}={got:?}");
    got
}

fn one(c: &str) -> Vec<String> {
    vec![c.to_string()]
}

/// r1 (M1, K-CAT): `t + ':' + i`, t a parameter of two constants, i a
/// Number: holds ':', never 'scene'.
#[test]
fn r1_cat_number_keeps_colon() {
    let got = run(
        "r1",
        "const m: any = {};
         function put(t: string, i: number) { const k = t + ':' + i; m[k] = new N(); }
         put('node', n); put('mesh', n + 1);
         m['scene'] = new S();
         const g: any = m['scene'];
         console.log(g.q);",
    );
    assert_eq!(got, vec![one("S")]);
}

/// r1 variants: the operands the r8 run's `getDependency` sees: `i` a
/// JSON value (any value: ToPrimitive of an object), `t` any string, `t`
/// an object. 13.15.3: `x + ':'` is ToString(ToPrimitive(x)) + ':'.
fn cat(tag: &str, t: &str, i: &str) -> Vec<Vec<String>> {
    run(
        tag,
        &format!(
            "const m: any = {{}};
             const j: any = JSON.parse(process.argv[3]);
             function put(t: any, i: any) {{ const k = t + ':' + i; m[k] = new N(); }}
             put('node', n); put({t}, {i});
             m['scene'] = new S();
             const g: any = m['scene'];
             console.log(g.q);"
        ),
    )
}

#[test]
fn r1b_cat_json_index() {
    assert_eq!(cat("r1b", "'mesh'", "j.i"), vec![one("S")]);
}

#[test]
fn r1c_cat_any_string_type() {
    assert_eq!(cat("r1c", "String(j.t)", "n"), vec![one("S")]);
}

#[test]
fn r1d_cat_object_type() {
    assert_eq!(cat("r1d", "j", "n"), vec![one("S")]);
}

/// r1e/r1f: which step loses ':': `t + ':'` alone (a suffix entry), and
/// `':' + i` beside an any-string head (`(t + ':') + i` in one operand).
fn key(tag: &str, k: &str) -> Vec<Vec<String>> {
    run(
        tag,
        &format!(
            "const m: any = {{}};
             const j: any = JSON.parse(process.argv[3]);
             const t: string = String(j.t);
             m[{k}] = new N();
             m['scene'] = new S();
             const g: any = m['scene'];
             console.log(g.q);"
        ),
    )
}

#[test]
fn r1e_suffix_colon() {
    assert_eq!(key("r1e", "t + ':'"), vec![one("S")]);
}

#[test]
fn r1f_colon_then_number() {
    assert_eq!(key("r1f", "t + ':' + n"), vec![one("S")]);
}

#[test]
fn r1g_infix_colon_string() {
    assert_eq!(key("r1g", "t + ':' + t"), vec![one("S")]);
}

/// r1h: the object operand at top level; r1i: one call of `put` with an
/// any-string type (no second caller).
#[test]
fn r1h_object_operand_top_level() {
    assert_eq!(key("r1h", "j + ':' + n"), vec![one("S")]);
}

#[test]
fn r1i_cat_one_caller_any_string() {
    let got = run(
        "r1i",
        "const m: any = {};
         const j: any = JSON.parse(process.argv[3]);
         function put(t: any, i: any) { const k = t + ':' + i; m[k] = new N(); }
         put(String(j.t), n);
         m['scene'] = new S();
         const g: any = m['scene'];
         console.log(g.q);",
    );
    assert_eq!(got, vec![one("S")]);
}

/// r2: the registry with a constant key and a ':' concat key.
#[test]
fn r2_registry_scene_vs_concat() {
    let got = run(
        "r2",
        "const c = R();
         c.add('scene', new S());
         for (let i = 0; i < n; i++) c.add('node:' + i, new N());
         const g: any = c.get('scene');
         console.log(g.q);
         const h: any = c.get('node:' + n);
         console.log(h.q);",
    );
    assert_eq!(got[0], one("S"));
}

/// r3: two constant keys only (M2 without M1).
#[test]
fn r3_registry_two_constants() {
    let got = run(
        "r3",
        "const c = R();
         c.add('a', new S());
         c.add('b', new N());
         const g: any = c.get('a');
         console.log(g.q);
         const h: any = c.get('b');
         console.log(h.q);",
    );
    assert_eq!(got, vec![one("S"), one("N")]);
}

/// r4a: the same through class methods on `this.o`.
#[test]
fn r4a_class_method_registry() {
    let got = run(
        "r4a",
        "class C { o: any = {}; get(k: string) { return this.o[k]; } add(k: string, v: any) { this.o[k] = v; } }
         const c = new C();
         c.add('a', new S());
         c.add('b', new N());
         const g: any = c.get('a');
         console.log(g.q);
         const h: any = c.get('b');
         console.log(h.q);",
    );
    assert_eq!(got, vec![one("S"), one("N")]);
}

/// r4b: a wrapper passes the key on (`getDependencies( type )`).
#[test]
fn r4b_wrapper_passes_key() {
    let got = run(
        "r4b",
        "class P { cache = R(); dep(t: string) { return this.cache.get(t); } }
         const p = new P();
         p.cache.add('a', new S());
         p.cache.add('b', new N());
         const g: any = p.dep('a');
         console.log(g.q);
         const h: any = p.dep('b');
         console.log(h.q);",
    );
    assert_eq!(got, vec![one("S"), one("N")]);
}

/// r5: the built-in Map.
#[test]
#[ignore = "Map per-key values: own lane"]
fn r5_map_two_constants() {
    let got = run(
        "r5",
        "const m = new Map<string, any>();
         m.set('a', new S());
         m.set('b', new N());
         const g: any = m.get('a');
         console.log(g.q);
         const h: any = m.get('b');
         console.log(h.q);",
    );
    assert_eq!(got, vec![one("S"), one("N")]);
}

/// k1 KEEP: a key from JSON content: everything stored, and more (the
/// Object.prototype members a read of any string may name).
#[test]
fn k1_json_key_reads_everything() {
    let got = run(
        "k1",
        "const c = R();
         c.add('a', new S());
         c.add('b', new N());
         const g: any = c.get(JSON.parse(process.argv[3]).k);
         console.log(g.q);",
    );
    assert!(
        got[0].contains(&"S".to_string()) && got[0].contains(&"N".to_string()),
        "{got:?}"
    );
}

/// k2 KEEP: a store under any string may be read under 'a'.
#[test]
fn k2_any_store_reaches_constant_read() {
    let got = run(
        "k2",
        "const c = R();
         c.add('a', new S());
         c.add(JSON.parse(process.argv[3]).k, new Z());
         c.add('b', new N());
         const g: any = c.get('a');
         console.log(g.q);",
    );
    assert!(
        got[0].contains(&"S".to_string()) && got[0].contains(&"Z".to_string()),
        "{got:?}"
    );
}
