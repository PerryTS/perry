use super::*;

#[test]
fn diagnostic_capture_is_observational_and_off_by_default() {
    let src = r#"
        class Base { x: number = 1; m(n: number) { return this.x + n; } }
        class Child extends Base {}
        function identity<T>(x: T) { return x; }
        function read(a: any, k: any) { return a[k]; }
        const c = new Child();
        const bound = identity.bind(undefined, 1);
        const f = read({m: identity}, "m");
        console.log(c.x, bound(), f(2));
        f(...[3]);
    "#;
    let off = facts(src);
    let on = facts_with(
        src,
        FactsConfig {
            diagnostics: true,
            ..Default::default()
        },
    );
    assert!(off.diagnostics.is_empty());
    assert!(!on.diagnostics.is_empty());
    assert_eq!(off.module_digests, on.module_digests);
    assert_eq!(format!("{:?}", off.bindings), format!("{:?}", on.bindings));
    let legacy = |p: &ProgramFacts| {
        p.dump_text()
            .lines()
            .filter(|l| {
                l.starts_with("M ")
                    || l.starts_with("T ")
                    || l.starts_with("  F ")
                    || l.starts_with("FACT ")
                    || l.starts_with("S ")
            })
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    assert_eq!(legacy(&off), legacy(&on));
    assert_eq!(
        (
            off.stats.nodes,
            off.stats.constraints,
            off.stats.work,
            off.stats.pops
        ),
        (
            on.stats.nodes,
            on.stats.constraints,
            on.stats.work,
            on.stats.pops
        )
    );
    let lines = &on.diagnostics[0].1;
    for required in [
        "\"kind\":\"element\"",
        "\"kind\":\"new\"",
        "\"kind\":\"value-call\"",
        "\"record\":\"node\"",
        "\"primitiveMask\":",
        "\"raw\":",
        "\"bound\":{",
        "\"ancestors\":[0]",
    ] {
        assert!(
            lines.iter().any(|l| l.contains(required)),
            "missing {required}"
        );
    }
}

#[test]
fn diagnostic_raw_callee_survives_a_failed_verdict() {
    let pf = facts_with(
        r#"
        function invoke(f: any) { return f(1); }
        const source: any = (globalThis as any).TYC2Unknown;
        console.log(invoke(source));
    "#,
        FactsConfig {
            diagnostics: true,
            ..Default::default()
        },
    );
    assert!(pf
        .sites
        .iter()
        .any(|s| matches!(&s.verdict, Verdict::No(_)) && s.is_call));
    assert!(pf
        .diagnostics
        .iter()
        .flat_map(|(_, l)| l)
        .any(|l| l.contains("\"record\":\"site\"") && l.contains("\"raw\":[0")));
}

#[test]
fn diagnostic_source_call_zero_excludes_synthetic_protocol_callees() {
    let pf = facts_with(
        r#"
        class C {}
        const c = new C();
        function unrelated<T>(x: T) { return x; }
        const b = unrelated.bind(undefined, 1);
        const source: any = (globalThis as any).TYC2Unknown;
        source["fn"](b());
    "#,
        FactsConfig {
            diagnostics: true,
            ..Default::default()
        },
    );
    let lines = &pf.diagnostics[0].1;
    let site = lines
        .iter()
        .find(|l| {
            l.contains("\"record\":\"site\"")
                && l.contains("\"kind\":\"new\"")
                && l.contains("\"key\":\"C\"")
        })
        .unwrap();
    let raw = site
        .split("\"raw\":[")
        .nth(1)
        .unwrap()
        .split("]")
        .next()
        .unwrap();
    assert!(!raw.is_empty());
    for id in raw.split(",") {
        let needle = format!("\"record\":\"token\",\"id\":{id},");
        let token = lines.iter().find(|l| l.contains(&needle)).unwrap();
        assert!(!token.contains(":unrelated\""), "{token}");
        assert!(!token.contains("ObjectProtoMethod"), "{token}");
    }
}
