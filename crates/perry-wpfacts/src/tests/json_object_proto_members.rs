//! A member of a `JSON.parse` object's chain is `Object.prototype`'s
//! (ECMA-262 25.5.1 JSON.parse: the objects ParseJSON makes are ordinary
//! objects, [[Prototype]] %Object.prototype%, 10.1.13
//! OrdinaryObjectCreate): `toString`, `valueOf` and `toLocaleString` read
//! as values are those realm methods (20.1.3.6, 20.1.3.7, 20.1.3.5), never
//! an opaque builtin function value whose call could use any argument.
//! skinbench: GLTFLoader's `objectDef.extensions[ name ]` (a `for...in` key
//! of a json object) may read any of them; the value reaches
//! `extensionsRequired.indexOf( extensionName )`'s callee.
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

fn src(tail: &str) -> String {
    format!(
        "const unknown: any = (globalThis as any)[process.argv[2]];
         const kept = {{ keptmark: 1 }};
         const cb = () => kept;
         const j: any = JSON.parse(process.argv[3]);
         {tail}
         console.log(String(cb() === kept));"
    )
}

/// `Object.prototype.toString` read off a json object and called: its
/// argument is unused (20.1.3.6).
#[test]
fn a_json_tostring_value_uses_no_argument() {
    let pf = whole(&src("const f: any = j.toString; console.log(f(cb));"));
    assert_eq!(escape_with_field(&pf, "keptmark"), None, "{}", pf.dump_text());
}

/// `valueOf` (20.1.3.7) and `toLocaleString` (20.1.3.5, `Invoke(O,
/// "toString")`) read as values: arguments unused.
#[test]
fn json_valueof_and_tolocalestring_values_use_no_argument() {
    let pf = whole(&src(
        "const f: any = j.valueOf; const g: any = j.toLocaleString; \
         try { console.log(f(cb), g(cb, cb)); } catch (e) { console.log(\"t\"); }",
    ));
    assert_eq!(escape_with_field(&pf, "keptmark"), None, "{}", pf.dump_text());
}

/// skinbench: a `for...in` key of a json object read back and called with
/// an argument (GLTFLoader `objectDef.extensions[ name ]`).
#[test]
fn a_json_for_in_member_value_uses_no_argument() {
    let pf = whole(&src(
        "for (const k in j) { const f: any = j[k]; try { console.log(f(cb)); } catch (e) { console.log(\"t\"); } }",
    ));
    assert_eq!(escape_with_field(&pf, "keptmark"), None, "{}", pf.dump_text());
}

/// Control: a key the program wrote onto `Object.prototype` through an
/// unknown store reads what was stored; its call gets the argument.
#[test]
fn a_chain_key_written_through_an_unknown_store_reads_the_store() {
    let pf = whole(&src(
        "(Object.prototype as any)[process.argv[4]] = unknown; const f: any = j[process.argv[5]]; f(cb);",
    ));
    assert!(escape_with_field(&pf, "keptmark").is_some(), "{}", pf.dump_text());
}

/// Control: the member value handed to `new Function` code with the
/// argument escapes.
#[test]
fn a_json_member_value_given_to_unknown_code_with_the_argument_escapes() {
    let pf = whole(&src("new Function(process.argv[4])()(j.toString, cb);"));
    assert!(escape_with_field(&pf, "keptmark").is_some(), "{}", pf.dump_text());
}

/// A json object's member under an unknown key, called with an argument:
/// own json data, or an `Object.prototype` method that uses no argument.
#[test]
fn a_json_member_under_an_unknown_key_uses_no_argument() {
    let pf = whole(&src(
        "const f: any = j[process.argv[4]]; try { console.log(f(cb)); } catch (e) { console.log(\"t\"); }",
    ));
    assert_eq!(escape_with_field(&pf, "keptmark"), None, "{}", pf.dump_text());
}
