//! PEELK23 (`String.prototype[@@iterator]` reads no argument): a call of a
//! primitive receiver's `@@iterator` runs `String.prototype[@@iterator]`
//! (ECMA-262 22.1.3.36: RequireObjectCoercible(this), ToString(this), a
//! String Iterator over its code points) on a string and throws a TypeError
//! on every other primitive kind; no argument is read, so none reaches
//! unknown code. The result is an iterator whose values are strings.
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

fn src(recv: &str) -> String {
    format!(
        "const lit: any = {{ keptmark: 1 }};
         const s: any = {recv};
         const it: any = s[Symbol.iterator](lit, lit);
         let out = \"\";
         for (const c of it) {{ out += c; }}
         console.log(out);"
    )
}

#[test]
fn string_iterator_reads_no_argument() {
    let pf = whole(&src("process.argv.length > 99 ? \"ab\" : \"cd\""));
    assert_eq!(escape_with_field(&pf, "keptmark"), None, "{}", pf.dump_text());
}

#[test]
fn mixed_primitive_iterator_reads_no_argument() {
    let pf = whole(&src("process.argv.length > 99 ? \"ab\" : process.argv.length > 98 ? 3 : true"));
    assert_eq!(escape_with_field(&pf, "keptmark"), None, "{}", pf.dump_text());
}

#[test]
fn unknown_code_receiver_stays_unknown() {
    let pf = whole(&src("new Function(process.argv[2])()"));
    assert!(escape_with_field(&pf, "keptmark").is_some(), "{}", pf.dump_text());
}
