//! Private methods use their declaring brand and member, independently of
//! public prototype lookups. Controls: public methods/numeric fields, public
//! same-spelling shadows, and identical private spellings in distinct classes.
//! Runtime prototype-reparenting control:
//! test_gap_class_private_method_after_prototype_mutation.
use super::*;

#[test]
fn a_private_method_receives_this_and_its_argument_and_returns_the_field_object() {
    let src = r##"
        class Payload { n = 1; }
        class Client {
            marker = true;
            opts: any;
            constructor(o: any) { this.opts = this.#init(o); }
            #init(o: any) { return { ...o, ok: this.marker }; }
        }
        console.log(new Client(new Payload()).opts.ok);
    "##;
    let ast = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&ast, "t", "t.ts").expect("lowers");
    let client = hir.classes.iter().find(|c| c.name == "Client").unwrap();
    let payload = hir.classes.iter().find(|c| c.name == "Payload").unwrap();
    let init = client.methods.iter().find(|m| m.name == "#init").unwrap();
    let pf = compute(&[("t.ts".to_string(), &hir)], &FactsConfig::default());
    let opts = field(tmpl(&pf, "Client"), "opts");
    assert!(
        !opts.kind.top && opts.kind.mask() & (1 << 7) != 0,
        "the private method returns an object: {}",
        pf.dump_text()
    );
    let bindings = &pf.bindings[0];
    let recv = &bindings
        .this_of
        .iter()
        .find(|(id, _)| *id == init.id)
        .expect("private this")
        .1;
    assert!(
        recv.class_ids.contains(&client.id),
        "private this: {recv:?}"
    );
    let arg = &bindings
        .locals
        .iter()
        .find(|(id, _)| *id == init.params[0].id)
        .expect("private argument")
        .1;
    assert!(
        arg.class_ids.contains(&payload.id),
        "private argument: {arg:?}"
    );
    assert_eq!(field(tmpl(&pf, "Payload"), "n").kind.mask(), 1);
}

#[test]
fn distinct_private_brands_keep_numeric_and_string_results_apart() {
    let pf = facts(
        r##"
        class A { value: any; constructor() { this.value = this.#read(); } #read() { return 1; } }
        class B { value: any; constructor() { this.value = this.#read(); } #read() { return "b"; } }
        console.log(new A().value, new B().value);
    "##,
    );
    let a = field(tmpl(&pf, "A"), "value").kind.mask();
    let b = field(tmpl(&pf, "B"), "value").kind.mask();
    assert!(
        a & 1 != 0 && a & 2 == 0,
        "A mask {a:#x}: {}",
        pf.dump_text()
    );
    assert!(
        b & 2 != 0 && b & 1 == 0,
        "B mask {b:#x}: {}",
        pf.dump_text()
    );
}

#[test]
fn a_public_same_spelling_shadow_does_not_select_the_private_body() {
    let pf = facts(
        r##"
        class C {
            out: any;
            constructor() { this.out = this.#make(); }
            #make() { return { x: 1 }; }
            publicValue() { return 3; }
        }
        (C.prototype as any)["#make"] = function() { return "shadow"; };
        const c = new C(); console.log(c.out.x, c.publicValue());
    "##,
    );
    let out = field(tmpl(&pf, "C"), "out");
    assert!(
        !out.kind.top && out.kind.mask() & (1 << 7) != 0 && out.kind.mask() & 2 == 0,
        "public spelling cannot choose the private method: {}",
        pf.dump_text()
    );
    assert_eq!(site(&pf, "publicValue").verdict, Verdict::T0Mono);
}

#[test]
fn a_private_write_prevents_a_false_number_only_field_kind() {
    let pf = facts(
        r#"
        class C {
            n: any = 1;
            constructor() { this.#change(); }
            #change() { this.n = "oops"; }
        }
        const c = new C();
        console.log(c.n + 1);
    "#,
    );
    assert_eq!(
        field(tmpl(&pf, "C"), "n").kind.mask(),
        1 | 2,
        "the private write contributes String beside Number: {}",
        pf.dump_text()
    );
}
