//! ARU V1 (design D3/D4): `length` of an Array exotic object or a primitive
//! String is an own, non-configurable data property, so the read is
//! `OwnLength` with no other condition. A value that may be no Array (the
//! `arguments` object, an array made through a written `constructor`, a
//! container of primitives) never is.
use super::*;

fn run(source: &str, units: bool) -> ProgramFacts {
    let parsed = perry_parser::parse_typescript(source, "t.ts").unwrap();
    let hir = perry_hir::lower_module(&parsed, "t", "t.ts").unwrap();
    let modules = [("t.ts".to_string(), &hir)];
    if units {
        crate::compute_units(&modules, &FactsConfig::default())
    } else {
        crate::compute(&modules, &FactsConfig::default())
    }
}

/// The verdicts of the `length` reads, in source order.
fn lengths(source: &str, units: bool) -> Vec<Verdict> {
    let pf = run(source, units);
    let mut s: Vec<_> = pf.sites.iter().filter(|s| s.key == "length" && !s.is_call).collect();
    s.sort_by_key(|s| s.byte_offset);
    s.iter().map(|s| s.verdict.clone()).collect()
}

fn own(v: &Verdict) -> bool {
    *v == Verdict::OwnLength
}

#[test]
fn array_sites_and_own_keys_results_have_own_length() {
    for units in [false, true] {
        let v = lengths(
            r#"
            function main() {
                const a = [1, 2, 3];
                const o = { x: 1, y: 2 };
                const k = Object.keys(o);
                const b = a.map((x) => x + 1);
                const c = a.filter((x) => x > 1).slice(1);
                function rest(...r: number[]) { return r.length; }
                console.log(a.length, k.length, b.length, c.length, rest(1, 2));
            }
            main();
        "#,
            units,
        );
        assert_eq!(v.len(), 5, "{v:?}");
        assert!(v.iter().all(own), "units={units}: {v:?}");
    }
}

#[test]
fn an_escaped_array_keeps_its_own_length() {
    for units in [false, true] {
        let v = lengths(
            r#"
            function main() {
                const ext: any = (globalThis as any)[process.argv[2]];
                const a = [1, 2];
                ext(a);
                console.log(a.length);
            }
            main();
        "#,
            units,
        );
        assert!(own(&v[0]), "units={units}: {v:?}");
    }
}

#[test]
fn arrays_and_strings_mix() {
    for units in [false, true] {
        let v = lengths(
            r#"
            function main() {
                const v = process.argv.length > 3 ? [1] : "ab";
                console.log(v.length);
            }
            main();
        "#,
            units,
        );
        assert!(own(v.last().unwrap()), "units={units}: {v:?}");
    }
}

#[test]
fn the_arguments_object_has_no_own_length_verdict() {
    for units in [false, true] {
        let v = lengths(
            r#"
            function f(x: number) {
                Object.defineProperty(arguments, "length", { get() { return "seven"; } });
                return arguments.length;
            }
            console.log(f(1));
        "#,
            units,
        );
        assert!(!v.iter().any(own), "units={units}: {v:?}");
    }
}

#[test]
fn an_array_made_through_a_written_constructor_is_no_array() {
    for units in [false, true] {
        for write in [
            "(a as any).constructor = C;",
            "(a as any)[process.argv[2]] = C;",
            "Object.setPrototypeOf(a, { constructor: C });",
            "ext(a);",
            "(Array.prototype as any).constructor = C;",
        ] {
            for make in ["a.map((x) => x)", "a.slice()", "a.splice(0, 1)", "a.filter((x) => x > 0)"] {
                let v = lengths(
                    &format!(
                        r#"
                    const ext: any = (globalThis as any)[process.argv[3]];
                    function C(this: any) {{ return {{ length: "x" }}; }}
                    (C as any)[Symbol.species] = C;
                    const a = [1, 2];
                    {write}
                    const b = {make};
                    console.log(b.length);
                "#
                    ),
                    units,
                );
                assert!(!own(v.last().unwrap()), "units={units} {write} {make}: {v:?}");
            }
        }
    }
}

#[test]
fn a_written_array_species_makes_no_array() {
    for units in [false, true] {
        let v = lengths(
            r#"
            class D { length = "x"; }
            Object.defineProperty(Array, Symbol.species, { value: D });
            const a = [1, 2];
            const b = a.map((x) => x);
            console.log(b.length);
        "#,
            units,
        );
        assert!(!own(v.last().unwrap()), "units={units}: {v:?}");
    }
}

#[test]
fn containers_that_may_be_no_array_and_other_objects_are_not_own_length() {
    for units in [false, true] {
        let v = lengths(
            r#"
            const m = process.memoryUsage() as any;
            const o: any = process.argv.length > 3 ? { length: 1 } : [1];
            const n: any = process.argv.length > 3 ? undefined : [1];
            console.log(m.length, o.length, n?.length);
        "#,
            units,
        );
        assert!(!v.iter().any(own), "units={units}: {v:?}");
    }
}

