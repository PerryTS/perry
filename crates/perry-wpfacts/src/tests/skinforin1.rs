//! SKINFORIN1 (skin GLTFLoader.js:2341/2345 `for (const name in
//! objectDef.extensions) gltfExtensions[name] = objectDef.extensions[name]`):
//! a for-in key names an ENUMERABLE property of the enumerated object or
//! its chain (ECMA-262 14.7.5.9 EnumerateObjectProperties, 14.7.5.10), and
//! no member a realm built-in function or constructor holds is enumerable
//! (clause 18; 10.3.4 CreateBuiltinFunction: `length`, `name` are
//! non-enumerable; 20.2.3 Function.prototype's members likewise). So the
//! same-object read `e[name]` over a JSON object whose [[Prototype]] is a
//! built-in function (skin: `Object.prototype.valueOf` in the JSON protos)
//! never yields `Function.prototype`'s or `Array.prototype`'s methods.
use super::*;

fn whole(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

fn escape_with_field(pf: &ProgramFacts, key: &str) -> Option<String> {
    pf.templates
        .iter()
        .filter(|t| t.display.starts_with("t.ts"))
        .find(|t| t.fields.iter().any(|f| f.key == key))
        .unwrap_or_else(|| panic!("no t.ts template with field {key}\n{}", pf.dump_text()))
        .escape
        .clone()
}

/// `e` = a parsed object whose prototype is a built-in function, `dst` a
/// null-prototype object filled by `lp`; `dst.foo.call(unknown, kept)`:
/// `Function.prototype.call`/`apply` there (members of the built-in link's
/// chain) would call `unknown` with this = `kept`; a JSON value is not
/// callable. `pre` runs before the loop.
fn src(pre: &str, lp: &str) -> String {
    format!(
        "const unknown: any = (globalThis as any)[process.argv[2]];
         const kept = {{ keptmark: 1 }};
         const j: any = JSON.parse(process.argv[3]);
         const e: any = j.ext;
         const other: any = j.other;
         Object.setPrototypeOf(e, Object.prototype.valueOf);
         {pre}
         const dst: any = Object.create(null);
         {lp}
         const f: any = dst.foo;
         try {{ f.call(unknown, kept); }} catch (x) {{ console.log(\"t\"); }}"
    )
}

const SAME_LOCAL: &str = "for (const name in e) { dst[name] = e[name]; }";
const SAME_PATH: &str = "function add(def: any) { for (const name in def.ext) { dst[name] = def.ext[name]; } } add(j);";

/// RED today: the EnumChain link `Object.prototype.valueOf` (a BuiltinFn,
/// not `enum_link`) is read in full (`load_from`, loads.rs family_any).
#[test]
fn skinforin1_builtin_fn_link_local() {
    let pf = whole(&src("", SAME_LOCAL));
    assert_eq!(escape_with_field(&pf, "keptmark"), None, "{}", pf.dump_text());
}

/// RED today, the GLTFLoader shape (subject path re-read, Rule R).
#[test]
fn skinforin1_builtin_fn_link_path() {
    let pf = whole(&src("", SAME_PATH));
    assert_eq!(escape_with_field(&pf, "keptmark"), None, "{}", pf.dump_text());
}

/// Stays: the read names ANOTHER object; `other` may lack an own key `e`
/// has (`toString`), so `other[name]` may hit the chain's built-ins.
#[test]
fn keep_skinforin1_other_object_wide() {
    let pf = whole(&src(
        "Object.setPrototypeOf(other, Object.prototype.valueOf);",
        "for (const name in e) { dst[name] = other[name]; }",
    ));
    assert!(escape_with_field(&pf, "keptmark").is_some(), "{}", pf.dump_text());
}

/// Stays: the program made an ENUMERABLE member on Object.prototype; the
/// same-object read yields it.
#[test]
fn keep_skinforin1_program_enumerable_proto_member() {
    let pf = whole(&src(
        "(Object.prototype as any).foo = Function.prototype.call;",
        SAME_LOCAL,
    ));
    assert!(escape_with_field(&pf, "keptmark").is_some(), "{}", pf.dump_text());
}

/// Control (stays): unknown key, no for-in: any member.
#[test]
fn keep_skinforin1_unknown_key_wide() {
    let pf = whole(&src("", "dst.foo = e[process.argv[4]];"));
    assert!(escape_with_field(&pf, "keptmark").is_some(), "{}", pf.dump_text());
}
