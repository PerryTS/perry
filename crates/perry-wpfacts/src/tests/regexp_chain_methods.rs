//! A RegExp method that `RegExp.prototype` does not own (ECMA-262 22.2.6)
//! is the member of its declared chain: `Object.prototype`
//! (`toLocaleString`, `hasOwnProperty`, `valueOf`, ...) or absent (the call
//! throws a TypeError before any argument is used). skinbench: the elements
//! `Array.prototype.toLocaleString` runs on include RegExp literals of
//! three.core.js; each element's `toLocaleString(locales, options)` on a
//! RegExp is `Object.prototype.toLocaleString` (20.1.3.5:
//! `Invoke(O, "toString")`, its arguments unused).
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
         const re: any = /a+/g;
         {tail}
         console.log(String(cb() === kept));"
    )
}

/// `Object.prototype.toLocaleString` on a RegExp: the arguments are unused.
#[test]
fn regexp_tolocalestring_uses_no_argument() {
    let pf = whole(&src("console.log(re.toLocaleString(cb, cb));"));
    assert_eq!(escape_with_field(&pf, "keptmark"), None, "{}", pf.dump_text());
}

/// skinbench: an array holding a RegExp, `toLocaleString`'d with arguments
/// (each element's `toLocaleString(locales, options)` is invoked).
#[test]
fn array_tolocalestring_of_a_regexp_element_uses_no_argument() {
    let pf = whole(&src("const xs: any = [re, 1]; console.log(xs.toLocaleString(cb, cb));"));
    assert_eq!(escape_with_field(&pf, "keptmark"), None, "{}", pf.dump_text());
}

/// A method on neither `RegExp.prototype` nor `Object.prototype`: the call
/// throws a TypeError, its argument is never used.
#[test]
fn an_absent_regexp_method_uses_no_argument() {
    let pf = whole(&src("try { re.nosuchmethod(cb); } catch (e) { console.log(\"t\"); }"));
    assert_eq!(escape_with_field(&pf, "keptmark"), None, "{}", pf.dump_text());
}

/// Control: `hasOwnProperty` ToPropertyKey's its argument, so its own
/// `toString` runs, and what that hands to unknown code escapes.
#[test]
fn regexp_hasownproperty_runs_the_key_tostring() {
    let pf = whole(&src(
        "const k = { toString() { unknown.keep(kept); return \"k\"; } };
         console.log(re.hasOwnProperty(k));",
    ));
    assert!(escape_with_field(&pf, "keptmark").is_some(), "{}", pf.dump_text());
}

/// Control: a method unknown code may have written onto the chain runs
/// with the argument.
#[test]
fn a_written_chain_method_gets_the_argument() {
    let pf = whole(&src(
        "(Object.prototype as any)[process.argv[3]] = unknown;
         console.log(re.nosuchmethod(cb));",
    ));
    assert!(escape_with_field(&pf, "keptmark").is_some(), "{}", pf.dump_text());
}

/// Control: the argument really handed to unknown code escapes.
#[test]
fn an_argument_given_to_unknown_code_escapes() {
    let pf = whole(&src("new Function(process.argv[4])()(cb); console.log(re.toLocaleString());"));
    assert!(escape_with_field(&pf, "keptmark").is_some(), "{}", pf.dump_text());
}
