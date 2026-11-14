//! SKINROOT10: GLTFLoader's registry (GLTFLoader.js:585, `GLTFRegistry`,
//! `objects = {}`): `get(key) { return objects[key]; }` with every key a
//! concatenation around a literal part (`type + ':' + index` :2928,
//! `'light:' + i` :698, `'PointsMaterial:' + uuid` :3504, `+=` :3546).
//! ECMA-262 13.15.3 ApplyStringOrNumericBinaryOperator: when either operand
//! is a String the result is the concatenation of both ToStrings (or the
//! evaluation throws), so it contains the literal; a template is the same
//! concatenation (13.2.8); ToPropertyKey of a String is itself (7.1.19). The
//! key names only members whose name contains the literal: no
//! Object.prototype member (`constructor`, the methods) is read.
use super::*;

fn whole(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

/// The sorted receivers of the `.p` reads of the registry's results.
fn recv(pf: &ProgramFacts) -> Vec<String> {
    let mut r: Vec<String> = pf
        .sites
        .iter()
        .filter(|s| s.key == "p" && !s.is_call)
        .flat_map(|s| s.receivers.iter())
        .map(|r| r.rsplit(':').next().unwrap_or(r).to_string())
        // `undefined`: a key the registry never stored.
        .filter(|r| !r.starts_with("undefined"))
        .collect();
    r.sort();
    r.dedup();
    r
}

fn prog(pre: &str, key: &str, calls: &str) -> String {
    format!(
        "class A {{ p = 1; }}
         {pre}
         function GLTFRegistry(): any {{
             let objects: any = {{}};
             return {{
                 get: function (key: any) {{ return objects[key]; }},
                 add: function (key: any, o: any) {{ objects[key] = o; }},
             }};
         }}
         const j: any = JSON.parse(String(process.argv[2]));
         class Parser {{
             cache: any = GLTFRegistry();
             getDependency(type: any, index: any): any {{
                 const cacheKey = {key};
                 let d = this.cache.get(cacheKey);
                 if (!d) {{ d = new A(); this.cache.add(cacheKey, d); }}
                 return d;
             }}
         }}
         const parser = new Parser();
         {calls}
         const d: any = parser.getDependency(j.t, j.i);
         console.log(d.p);"
    )
}

fn gltf(key: &str) -> Vec<String> {
    recv(&whole(&prog("", key, "")))
}

#[test]
fn red_skinroot10_infix_key_reads_no_prototype_member() {
    // The GLTFLoader key forms, all through one registry.
    let calls = "parser.getDependency('mesh', 0);
         parser.getDependency('node', j.n);
         const lk = 'light:' + j.l;
         let ld: any = parser.cache.get(lk);
         if (!ld) { ld = new A(); parser.cache.add(lk, ld); }
         console.log(ld.p);
         let ck = 'ClonedMaterial:' + j.u + ':';
         if (j.x) ck += 'flat-shading:';
         const cd: any = parser.cache.get(ck);
         console.log(cd.p);";
    let v = recv(&whole(&prog("", "type + ':' + index", calls)));
    assert_eq!(v, vec!["A"], "{v:?}");
}

#[test]
fn red_skinroot10_unknown_parts_colon_key() {
    assert_eq!(gltf("type + ':' + index"), vec!["A"]);
}

#[test]
fn keep_skinroot10_unknown_key_reads_prototype_members() {
    // k1: TRUE: an unknown key may be "constructor" or any method name.
    let v = gltf("j.k");
    assert!(v.len() > 1, "{v:?}");
}

#[test]
fn keep_skinroot10_to_prefix_reads_to_members() {
    // k2: `'to' + x` may be `toString` / `toLocaleString`.
    let v = gltf("'to' + index");
    assert!(v.len() > 1, "{v:?}");
}

#[test]
fn keep_skinroot10_join_with_unknown_is_wide() {
    // k3: a join with a value lacking the infix has no infix.
    let v = gltf("(j.c ? ('a:' + index) : type)");
    assert!(v.len() > 1, "{v:?}");
}

#[test]
fn keep_skinroot10_program_proto_member_with_infix_is_read() {
    // k4: a program-written Object.prototype member whose name holds ':'.
    let v = recv(&whole(&prog(
        "class F { p = 3; } (Object.prototype as any)['x:y'] = new F();",
        "'x:' + index",
        "",
    )));
    assert!(v.contains(&"F".to_string()), "{v:?}");
}

#[test]
fn red_skinroot10_string_and_template_forms_keep_infix() {
    // k5: `String(x)` / template / `+=` forms are concatenations too.
    assert_eq!(gltf("String(type) + ':' + String(index)"), vec!["A"]);
    assert_eq!(gltf("`${type}:${index}`"), vec!["A"]);
    assert_eq!(gltf("(function () { let k = type + ''; k += ':'; k += index; return k; })()"), vec!["A"]);
}

/// The registry read of GLTFParser::getDependency on skin: `type` one of
/// the literal kinds, `index` a Number (glTF indices): each key is
/// `kind + ':' + ToString(number)` (13.15.3, 6.1.6.1.20), held as an
/// exclusion entry (its characters: the kind's, ':' and a Number's).
fn numeric_index_prog(calls: &str) -> String {
    prog("", "type + ':' + index", calls).replace(
        "const d: any = parser.getDependency(j.t, j.i);",
        "const d: any = parser.getDependency('scene', 0);",
    )
}

#[test]
fn red_skinroot10_kind_and_number_key_reads_no_prototype_member() {
    for c in [
        "parser.getDependency('camera', Math.floor(Math.random() * 9));",
        "parser.getDependency('camera', Number(j.i));",
        "for (const t of ['mesh', 'node']) parser.getDependency(t, Number(j.i));",
        "(function (ty: any) { return parser.getDependency(ty, Number(j.i)); })(Math.random() < 0.5 ? 'skin' : 'mesh');",
    ] {
        let v = recv(&whole(&numeric_index_prog(c)));
        assert_eq!(v, vec!["A"], "{c}: {v:?}");
    }
}

#[test]
fn keep_skinroot10_number_then_member_suffix_reads_member() {
    // An exclusion entry keeps the names it may spell: `X + 'toString'`
    // with X empty or a Number's text may be `toString`.
    let v = gltf("(j.c ? '' : String(Number(j.i))) + 'toString'");
    assert!(v.len() > 1, "{v:?}");
}
