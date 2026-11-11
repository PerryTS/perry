//! DEC 250, deletes: the runtime refuses to delete a member the image gives
//! a locked intrinsic prototype (except `constructor`, `builtin_proto::
//! locked_violation`) and lets a family it leaves open lose any key. A
//! delete that may reach the runtime's prototypes (the collapsed
//! `ObjectProto` token) is therefore a native write of the keys the lock
//! admits, read by `members::locked_read`: it removes no key of a program
//! object, so a literal whose chain miss reaches `Object.prototype` keeps
//! its layout. A delete on a program prototype stays HARD.
use super::*;

fn one(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

fn tmpl<'a>(pf: &'a ProgramFacts, first_key: &str) -> &'a crate::TemplateFacts {
    pf.templates
        .iter()
        .find(|t| {
            t.birth_keys.first().is_some_and(|k| k == first_key)
                || t.display.ends_with(&format!(":{first_key}"))
        })
        .unwrap_or_else(|| panic!("no template {first_key}\n{}", pf.dump_text()))
}

fn verdicts(pf: &ProgramFacts, key: &str) -> Vec<Verdict> {
    let mut s: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == key && !s.is_call)
        .collect();
    s.sort_by_key(|s| s.byte_offset);
    s.iter().map(|s| s.verdict.clone()).collect()
}

fn proven(v: &[Verdict]) -> bool {
    !v.is_empty() && v.iter().all(|v| !matches!(v, Verdict::No(_)))
}

fn unproven(v: &[Verdict]) -> bool {
    !v.is_empty() && v.iter().all(|v| matches!(v, Verdict::No(_)))
}

/// A value that may be `Object.prototype` (a parameter given it and a
/// literal) takes a delete: computed, named, `Reflect.deleteProperty`. The
/// literal `{ zoneIndex, fourOctet }` misses `missing` up to
/// `Object.prototype`. Red before: "Object.prototype E7" on its chain, no
/// fixed layout.
const DELETES: [&str; 3] = [
    "function strip(o: any, k: string) { delete o[k]; }",
    "function strip(o: any, _k: string) { delete o.zz; }",
    "function strip(o: any, k: string) { Reflect.deleteProperty(o, k); }",
];

const NONE: &str = "function strip(_o: any, _k: string) {}";

fn program(del: &str, body: &str) -> String {
    format!(
        "{del}\nstrip(Object.prototype, process.argv[2]); strip({{ a: 1 }}, process.argv[2]);\n\
         function main() {{ {body} }}\nconsole.log(main());"
    )
}

const BODY: &str = "const r: any = { zoneIndex: 1, fourOctet: 2 }; \
                    return r.zoneIndex + r.fourOctet + (r.missing === undefined ? 0 : 1);";

#[test]
fn a_delete_on_object_prototype_keeps_a_literal_layout() {
    let control = one(&program(NONE, BODY));
    let c = tmpl(&control, "zoneIndex");
    assert!(
        c.fixed_layout && c.prefix_stable,
        "control: {}",
        control.dump_text()
    );
    for del in DELETES {
        let pf = one(&program(del, BODY));
        let t = tmpl(&pf, "zoneIndex");
        assert!(
            t.fixed_layout && t.prefix_stable && t.chain.is_none(),
            "{del}: {:?}\n{}",
            t.chain,
            pf.dump_text()
        );
        assert_eq!(
            verdicts(&pf, "zoneIndex"),
            verdicts(&control, "zoneIndex"),
            "{del}"
        );
    }
}

/// Soundness: `constructor` is exempt from the lock, so its delete changes
/// what `o.constructor` reads: no longer the builtin `Object` (the control
/// arm, where `C(p)` keeps `p` and `p.x` is proven) but what the delete
/// left, so `p` reaches a call the facts do not know and `p.x` is unproven.
#[test]
fn a_delete_of_constructor_still_changes_its_read() {
    let body = "const o: any = { y: 1 }; const p: any = { x: 1 }; const C: any = o.constructor; \
                C(p); return p.x;";
    let v = verdicts(&one(&program(NONE, body)), "x");
    assert!(proven(&v), "control: {v:?}");
    for del in [
        "function strip(o: any, _k: string) { delete o.constructor; }",
        "function strip(o: any, k: string) { delete o[k]; }",
    ] {
        let pf = one(&program(del, body));
        let v = verdicts(&pf, "x");
        assert!(unproven(&v), "{del}: {v:?}\n{}", pf.dump_text());
    }
}

/// Soundness: a program class prototype is no locked intrinsic: a computed
/// delete on it is HARD on its instances' chain.
#[test]
fn a_delete_on_a_program_prototype_stays_hard() {
    let src = "class A { m() { return 1; } }\n\
               function strip(o: any, k: string) { delete o[k]; }\n\
               strip(A.prototype, process.argv[2]);\n\
               function main() { const a: any = new A(); return a.m(); }\nconsole.log(main());";
    let pf = one(src);
    let t = tmpl(&pf, "A");
    assert!(
        t.chain.as_deref().is_some_and(|c| c.contains("E7")),
        "{:?}\n{}",
        t.chain,
        pf.dump_text()
    );
}
