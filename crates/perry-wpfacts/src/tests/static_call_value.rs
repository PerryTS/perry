//! A namespace static reached as a VALUE in a callee position with a
//! foreign `this` (a field holding `Object.create` called as its object's
//! method, the any-key protocol call on tssrc's compiler-options object).
//! Node: `Object.create`, `Object.setPrototypeOf` and
//! `Reflect.setPrototypeOf` never read the this value (ECMA-262 20.1.2.2,
//! 20.1.2.23, 28.1.13): the receiver is handed to no one. The arguments
//! keep the model of the plain call. With an unknown callee (⊤) the
//! receiver still escapes.
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

#[test]
fn a_field_holding_a_prototype_static_called_on_its_object_keeps_it() {
    for f in ["Object.create", "Object.setPrototypeOf", "Reflect.setPrototypeOf"] {
        let src = format!(
            r#"
const o = {{ pa: 1, m: {f} }};
try {{ (o as any).m(); }} catch (e) {{}}
console.log(o.pa + 1);
"#
        );
        let pf = whole(&src);
        assert_eq!(escape_with_field(&pf, "pa"), None, "{f}\n{}", pf.dump_text());
    }
}

// Control: the argument keeps the plain call's model (it becomes the
// result's prototype, and the result is unknown: it escapes).
#[test]
fn object_create_called_on_an_object_still_models_its_argument() {
    let pf = whole(
        r#"
const p = { pb: 2 };
const o = { pa: 1, m: Object.create };
const q = (o as any).m(p);
console.log(o.pa + 1, q);
"#,
    );
    assert_eq!(escape_with_field(&pf, "pa"), None, "{}", pf.dump_text());
    assert!(escape_with_field(&pf, "pb").is_some(), "{}", pf.dump_text());
}

// Control: the callee is unknown code: the receiver is its `this`.
#[test]
fn a_field_holding_an_unknown_function_called_on_its_object_escapes_it() {
    let pf = whole(
        r#"
const f: any = new Function(process.argv[2]);
const o = { pa: 1, m: f };
o.m();
console.log(o.pa + 1);
"#,
    );
    assert!(escape_with_field(&pf, "pa").is_some(), "{}", pf.dump_text());
}

// Fix 2: every own static of a namespace none of whose statics reads the
// this value (`Object`, `Reflect`, `Math`, `JSON`; ECMA-262 20.1.2, 28.1,
// 21.3.2, 25.5), the undescribed ones included, called as a value with a
// foreign this: the receiver is given to no one.
#[test]
fn a_field_holding_any_this_free_static_called_on_its_object_keeps_it() {
    for f in [
        "Object.defineProperties",
        "Object.defineProperty",
        "Object.fromEntries",
        "Object.getOwnPropertyDescriptors",
        "Object.groupBy",
        "Reflect.get",
        "Reflect.apply",
        "Reflect.deleteProperty",
        "JSON.rawJSON",
    ] {
        let src = format!(
            r#"
const o = {{ pa: 1, m: {f} }};
try {{ (o as any).m(); }} catch (e) {{}}
console.log(o.pa + 1);
"#
        );
        let pf = whole(&src);
        assert_eq!(escape_with_field(&pf, "pa"), None, "{f}\n{}", pf.dump_text());
    }
}

// The tssrc construct (commandLineParser's options object): an unknown-key
// read of a program object yields `Object.prototype`'s members (its
// `constructor` is `Object`), a second one `Object`'s statics; an
// unknown-key store puts them under the object's any-key field, so its
// `toString` / `valueOf` may be any of them, called by ToPrimitive
// (`Array.prototype.join`) with the object as this.
#[test]
fn join_of_an_object_holding_object_statics_under_any_key_keeps_it() {
    let pf = whole(
        r#"
const o: any = { pa: 1 };
const k = process.argv[2];
const k2 = process.argv[3];
const c = o[k];
const f = c[k2];
o[k2] = f;
try { console.log([o].join(",")); } catch (e) {}
console.log(o.pa + 1);
"#,
    );
    assert_eq!(escape_with_field(&pf, "pa"), None, "{}", pf.dump_text());
}

// Control: `Promise.withResolvers` uses its this value as the constructor
// (ECMA-262 27.2.4.8, NewPromiseCapability(C)): a static of a namespace
// whose statics read this still hands the receiver on.
#[test]
fn a_field_holding_a_this_reading_static_called_on_its_object_hands_it_on() {
    let pf = whole(
        r#"
const o = { pa: 1, m: Promise.withResolvers };
try { (o as any).m(); } catch (e) {}
console.log(o.pa + 1);
"#,
    );
    assert!(escape_with_field(&pf, "pa").is_some(), "{}", pf.dump_text());
}
