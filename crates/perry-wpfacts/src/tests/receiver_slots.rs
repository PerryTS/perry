//! DEC220: private own-slot certificates, with conservative layout contracts.
use super::*;

fn target(pf: &ProgramFacts) -> &crate::SiteFacts {
    let reads: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == "target" && !s.is_call)
        .collect();
    assert_eq!(reads.len(), 1, "{}", pf.dump_text());
    reads[0]
}

#[test]
fn an_escaped_sibling_does_not_escape_the_private_own_slot() {
    let pf = facts(
        "declare function keep(x: any): void;
         const privateRecord = { source: 1, target: 2 };
         const sibling = { source: 3, target: 4 };
         keep(sibling); console.log(privateRecord.target);",
    );
    assert_eq!(target(&pf).verdict, Verdict::T0Mono, "{}", pf.dump_text());
    let layouts: Vec<_> = pf
        .templates
        .iter()
        .filter(|t| t.birth_keys == ["source", "target"])
        .collect();
    assert_eq!(
        layouts.len(),
        1,
        "same layout must be shared\n{}",
        pf.dump_text()
    );
    assert!(
        !layouts[0].prefix_stable && layouts[0].escape.is_some(),
        "layout contract must keep the escaping birth\n{}",
        pf.dump_text()
    );
}

#[test]
fn deleting_or_reconfiguring_a_siblings_slot_keeps_the_private_slot() {
    for mutation in [
        "delete sibling.target;",
        "Object.defineProperty(sibling, 'target', { value: 99, configurable: true });",
        "Object.setPrototypeOf(sibling, null);",
    ] {
        let pf = facts(&format!(
            "const privateRecord = {{ source: 1, target: 2 }};
             const sibling = {{ source: 3, target: 4 }};
             {mutation} console.log(privateRecord.target);"
        ));
        assert_eq!(
            target(&pf).verdict,
            Verdict::T0Mono,
            "{mutation}\n{}",
            pf.dump_text()
        );
    }
}

#[test]
fn reaching_the_private_receiver_through_a_sibling_refuses_the_certificate() {
    for publication in [
        "keep(privateRecord);",
        "const alias = privateRecord; keep(alias);",
        "sibling.source = privateRecord; keep(sibling);",
        "Object.setPrototypeOf(sibling, privateRecord); keep(sibling);",
    ] {
        let pf = facts(&format!(
            "declare function keep(x: any): void;
             const privateRecord = {{ source: 1 as any, target: 2 }};
             const sibling = {{ source: 3 as any, target: 4 }};
             {publication} console.log(privateRecord.target);"
        ));
        assert!(
            matches!(target(&pf).verdict, Verdict::No(_)),
            "{publication}\n{}",
            pf.dump_text()
        );
    }
}

#[test]
fn changing_the_receivers_descriptor_or_chain_refuses_the_certificate() {
    for mutation in [
        "delete privateRecord.target;",
        "Object.defineProperty(privateRecord, 'target', { get() { return 99; } });",
        "Object.setPrototypeOf(privateRecord, sibling);",
        "Object.defineProperty(Object.prototype, 'target', { get() { return 99; } });",
        "Object.defineProperty(Object.prototype, key, { get() { return 99; } });",
    ] {
        let pf = facts(&format!(
            "declare const key: string;
             const privateRecord = {{ source: 1, target: 2 }};
             const sibling = {{ source: 3, target: 4 }};
             {mutation} console.log(privateRecord.target);"
        ));
        assert!(
            matches!(target(&pf).verdict, Verdict::No(_)),
            "{mutation}\n{}",
            pf.dump_text()
        );
    }
}

#[test]
fn unknown_class_construction_keeps_the_unknown_birth_escaped() {
    let pf = facts(
        "declare function keep(x: any): void;
         class MadeElsewhere { target = 2; }
         keep(MadeElsewhere);",
    );
    let layout = tmpl(&pf, "MadeElsewhere");
    assert!(
        layout.escape.is_some() && !layout.prefix_stable,
        "{}",
        pf.dump_text()
    );
}

#[test]
fn unknown_construction_keeps_constructor_effects_and_returned_aliases() {
    for body in ["return privateRecord as any;", "keep(privateRecord);"] {
        let pf = facts(&format!(
            "declare function keep(x: any): void;
             const privateRecord = {{ source: 1, target: 2 }};
             class Maker {{ constructor() {{ {body} }} }}
             keep(Maker); console.log(privateRecord.target);"
        ));
        assert!(
            matches!(target(&pf).verdict, Verdict::No(_)),
            "{body}\n{}",
            pf.dump_text()
        );
    }
}

#[test]
fn a_real_class_prototype_exposed_to_unknown_code_keeps_its_chain_obligation() {
    let pf = facts(
        "declare function keep(x: any): void;
         class Item { target = 2; }
         const privateRecord = new Item(); keep(Item);
         console.log(privateRecord.target);",
    );
    assert!(
        matches!(target(&pf).verdict, Verdict::No(_)),
        "{}",
        pf.dump_text()
    );
    assert!(tmpl(&pf, "Item").chain.is_some(), "{}", pf.dump_text());
}

fn both_modes(src: &str) -> Vec<(&'static str, ProgramFacts)> {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let h = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let cfg = FactsConfig {
        max_work: Some(50_000_000),
        ..FactsConfig::default()
    };
    vec![
        ("whole", crate::compute(&[("t.ts".to_string(), &h)], &cfg)),
        (
            "unit",
            crate::compute_units(&[("t.ts".to_string(), &h)], &cfg),
        ),
    ]
}

fn own_read_is_proven(pf: &ProgramFacts, key: &str) -> bool {
    let sites: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == key && !s.is_call)
        .collect();
    assert!(!sites.is_empty(), "no live read {key}\n{}", pf.dump_text());
    sites
        .iter()
        .all(|s| matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set))
}

#[test]
fn a_constructor_publishing_this_exposes_its_program_births_too() {
    for src in [
        "declare function keep(x: any): void;
         class Publishes { static saved: any; target = 2;
           constructor() { Publishes.saved = this; } }
         const privateRecord = new Publishes(); keep(Publishes);
         console.log(privateRecord.target);",
        "declare function keep(x: any): void;
         let saved: any;
         class Publishes { target = 2; constructor() { saved = this; } }
         keep(() => saved);
         const privateRecord = new Publishes(); keep(Publishes);
         console.log(privateRecord.target);",
        "declare function keep(x: any): void;
         class Publishes { target = 2;
           constructor() { (globalThis as any).saved = this; } }
         keep(globalThis);
         const privateRecord = new Publishes(); keep(Publishes);
         console.log(privateRecord.target);",
    ] {
        for (mode, pf) in both_modes(src) {
            assert!(
                !own_read_is_proven(&pf, "target"),
                "{mode}\n{}",
                pf.dump_text()
            );
        }
    }
}

#[test]
fn a_method_publishes_the_receivers_the_program_calls_it_on() {
    for program_call in [false, true] {
        let src = format!(
            "declare function keep(x: any): void;
             function run() {{
             const privateRecord = {{ source: 1, target: 2 }};
             class Holder {{ value: any;
               constructor(value: any) {{ this.value = value; }}
               publish() {{ keep(this.value); }} }}
             const holder = new Holder(privateRecord); keep(Holder);
             {} return privateRecord.target;
             }} console.log(run(), run());",
            if program_call {
                "holder.publish();"
            } else {
                ""
            }
        );
        for (mode, pf) in both_modes(&src) {
            assert_eq!(
                own_read_is_proven(&pf, "target"),
                !program_call,
                "{mode}, program call={program_call}\n{}",
                pf.dump_text()
            );
        }
    }
}

#[test]
fn an_exposed_real_prototype_keeps_inherited_reads_unproven() {
    for change in [
        "keep(Item.prototype);",
        "Object.defineProperty(Item.prototype, 'inherited', { get() { return 99; } });",
        "(Item.prototype as any).inherited = (globalThis as any).foreignMethod;",
    ] {
        let src = format!(
            "declare function keep(x: any): void;
             class Item {{ target = 2; inherited() {{ return 1; }} }}
             const privateRecord = new Item(); keep(Item);
             {change} console.log(privateRecord.inherited, privateRecord.target);"
        );
        for (mode, pf) in both_modes(&src) {
            assert!(
                !own_read_is_proven(&pf, "inherited"),
                "{mode}\n{}",
                pf.dump_text()
            );
            assert!(
                !own_read_is_proven(&pf, "target"),
                "{mode}: real prototype obligation\n{}",
                pf.dump_text()
            );
        }
    }
}

#[test]
fn an_unknown_subclass_runs_super_without_acquiring_private_arguments() {
    for program_new in [false, true] {
        let src = format!(
            "declare function keep(x: any): void;
             const privateRecord = {{ source: 1, target: 2 }};
             class Base {{ constructor(value: any) {{ keep(value); }} }}
             function subclass(C: any) {{
               return class X extends C {{ constructor(value: any) {{ super(value); }} }};
             }}
             const X = subclass(Base); keep(Base); keep(X);
             {} console.log(privateRecord.target);",
            if program_new {
                "new X(privateRecord);"
            } else {
                ""
            }
        );
        for (mode, pf) in both_modes(&src) {
            assert_eq!(
                own_read_is_proven(&pf, "target"),
                !program_new,
                "{mode}, program new={program_new}\n{}",
                pf.dump_text()
            );
        }
    }
}
