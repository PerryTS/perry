//! A `JSON.parse` value converted to a primitive (GLTFLoader.js: GLTF JSON
//! fields in string concatenations and template literals). Node: the parsed
//! tree holds ordinary objects, arrays and primitives only; ToPrimitive of
//! one runs `Object.prototype.toString` / `Array.prototype.toString` (whose
//! `join` is the intrinsic), and nothing the program stored into the tree is
//! handed to anyone.
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

fn src(conv: &str) -> String {
    format!(
        "class A {{ a = 1; }}
         const j: any = JSON.parse(process.argv[2] ?? '{{}}');
         const a = new A();
         j.keep = a;
         const x: any = j.x;
         console.log({conv});
         console.log(a.a);"
    )
}

#[test]
fn a_parsed_value_converted_to_a_string_keeps_what_the_tree_holds() {
    let pf = whole(&src("'' + x"));
    assert_eq!(escape_with_field(&pf, "a"), None, "{}", pf.dump_text());
}

#[test]
fn a_parsed_value_in_a_template_keeps_what_the_tree_holds() {
    let pf = whole(&src("`${x}`"));
    assert_eq!(escape_with_field(&pf, "a"), None, "{}", pf.dump_text());
}

/// Control: the parsed value handed to unknown code takes what it holds.
#[test]
fn a_parsed_value_handed_to_unknown_code_escapes_what_it_holds() {
    let pf = whole(&src("(globalThis as any)[process.argv[3]](x)"));
    assert!(escape_with_field(&pf, "a").is_some(), "{}", pf.dump_text());
}

/// An escaped derived class (three.core.js `Material` subclasses reached
/// from unknown code): whoever constructs it, `super()` initializes a FRESH
/// ordinary object (OrdinaryCreateFromConstructor, ECMA-262 10.1.13), never
/// `Object.prototype` or another intrinsic. Node: `Object.prototype.zz` is
/// never written, so `o.zz` is undefined and `o.zz(a)` throws a TypeError.
fn ctor_src(hand: &str) -> String {
    format!(
        "const unknown: any = (globalThis as any)[process.argv[2]];
         class B {{ zz: any; constructor() {{ this.zz = (x: any) => unknown.keep(x); }} }}
         class C extends B {{ constructor() {{ super(); }} }}
         {hand}
         class A {{ a = 1; }}
         const a = new A();
         const o: any = {{ q: 1 }};
         try {{ o.zz(a); }} catch (e) {{ console.log(\"TypeError\"); }}
         console.log(a.a);"
    )
}

#[test]
fn an_escaped_derived_class_never_initializes_object_prototype() {
    let pf = whole(&ctor_src("unknown.take(C);"));
    assert_eq!(escape_with_field(&pf, "a"), None, "{}", pf.dump_text());
}

/// GLTFLoader.js: `json[k]` on a `JSON.parse` object with a computed key,
/// then three.core.js's Material constructor writing its fields on what the
/// read gave (`this.stencilWrite = ...`). DECISIONS 217 (owner 10-27): a
/// computed key of unknown string never reads the `__proto__` accessor, so
/// the read never yields `Object.prototype`, the store never lands there and
/// an unrelated literal's `plain.stencilWrite` reads `undefined`.
fn parsed_src(read: &str, ctor: &str) -> String {
    format!(
        "const unknown: any = (globalThis as any)[process.argv[2]];
         class A {{ a = 1; }}
         const a = new A();
         function Material(this: any) {{ this.stencilWrite = a; }}
         const j: any = JSON.parse(process.argv[3] ?? '{{}}');
         const k: any = process.argv[4];
         const v: any = {read};
         {ctor}
         const plain: any = {{ q: 1 }};
         console.log(typeof plain.stencilWrite === 'object' ? 1 : 0);
         unknown.take(plain.stencilWrite);
         console.log(a.a);"
    )
}

#[test]
fn a_computed_read_of_a_parsed_object_never_yields_object_prototype() {
    let pf = whole(&parsed_src("j[k]", "Material.call(v);"));
    assert_eq!(escape_with_field(&pf, "a"), None, "{}", pf.dump_text());
}

/// Control: the program itself runs the constructor on `Object.prototype`.
#[test]
fn material_called_on_object_prototype_writes_there() {
    let pf = whole(&parsed_src("j[k]", "Material.call(Object.prototype);"));
    assert!(escape_with_field(&pf, "a").is_some(), "{}", pf.dump_text());
}

/// Control: the named `__proto__` read of a parsed object is `Object.prototype`.
#[test]
fn a_named_proto_read_of_a_parsed_object_writes_there() {
    let pf = whole(&parsed_src("j.__proto__", "Material.call(v);"));
    assert!(escape_with_field(&pf, "a").is_some(), "{}", pf.dump_text());
}
