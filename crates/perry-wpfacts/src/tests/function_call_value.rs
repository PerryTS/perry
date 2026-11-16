//! `Function.prototype`'s methods arriving as a VALUE in a callee position
//! (a field holding `Function.prototype.call` called as its object's
//! method, `Function.prototype.call.call(o)`), with a `this` that is no
//! function. Node: `call`, `apply`, `bind` and `toString` throw a TypeError
//! before they read `this` or an argument (ECMA-262 20.2.3.1-.3, .5);
//! nothing happens to the values. The receiver keeps its facts and is not
//! handed to unknown code (`builtins::never_callable`,
//! `Model::function_this_not_callable`). With a function `this` the
//! existing model runs the function (`function_method`); with an unknown
//! one (⊤) the call is still one of an unknown function value.
use super::*;

fn whole(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

/// The escape of the `t.ts` template that has an own field `key`.
fn escape_with_field(pf: &ProgramFacts, key: &str) -> Option<String> {
    pf.templates
        .iter()
        .filter(|t| t.display.starts_with("t.ts"))
        .find(|t| t.fields.iter().any(|f| f.key == key))
        .unwrap_or_else(|| panic!("no t.ts template with field {key}\n{}", pf.dump_text()))
        .escape
        .clone()
}

/// Every read of `key` in the module is proven (not `Verdict::No`).
fn reads_proven(pf: &ProgramFacts, key: &str) {
    let v: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == key && !s.is_call)
        .map(|s| s.verdict.clone())
        .collect();
    assert!(
        !v.is_empty() && v.iter().all(|v| !matches!(v, Verdict::No(_))),
        "reads of {key}: {v:?}\n{}",
        pf.dump_text()
    );
}

fn kept(src: &str, key: &str) {
    let pf = whole(src);
    assert_eq!(escape_with_field(&pf, key), None, "{}", pf.dump_text());
    reads_proven(&pf, key);
}

// The tssrc shape: a field of an object holds the builtin function value,
// and the object is the call's receiver.
#[test]
fn a_field_holding_function_call_called_on_its_object_keeps_it() {
    for m in ["call", "apply", "bind", "toString"] {
        let src = format!(
            r#"
const o = {{ pa: 1, m: Function.prototype.{m} }};
const arg = {{ pb: 2 }};
try {{ o.m(arg, arg); }} catch (e) {{}}
console.log(o.pa + 1, arg.pb + 1);
"#
        );
        kept(&src, "pa");
        kept(&src, "pb");
    }
}

#[test]
fn function_call_called_through_call_on_an_object_keeps_it() {
    kept(
        r#"
const c = Function.prototype.call;
const o = { pa: 1 };
const arg = { pb: 2 };
try { c.call(o, arg); } catch (e) {}
console.log(o.pa + 1, arg.pb + 1);
"#,
        "pa",
    );
}

// A function `this`: the function runs with the first argument as `this`
// (the existing model), the object keeps its facts.
#[test]
fn function_call_value_on_a_function_runs_it() {
    kept(
        r#"
const c = Function.prototype.call;
function f(this: any, x: any) { return this.pa + x.pb; }
const o = { pa: 1 };
const arg = { pb: 2 };
console.log(c.call(f, o, arg), Function.prototype.call.call(f, o, arg));
console.log(o.pa + 1, arg.pb + 1);
"#,
        "pa",
    );
}

// Control: the function is unknown (⊤): `o` is handed to unknown code.
#[test]
fn function_call_value_on_an_unknown_function_escapes_its_arguments() {
    let pf = whole(
        r#"
const c = Function.prototype.call;
const u = new Function(process.argv[2]);
const o = { pa: 1 };
c.call(u, o);
console.log(o.pa + 1);
"#,
    );
    assert!(escape_with_field(&pf, "pa").is_some(), "{}", pf.dump_text());
}
