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
