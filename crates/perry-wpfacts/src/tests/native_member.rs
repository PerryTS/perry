//! BIV (CHB §2): `NativeMember`, a member read or method call whose
//! receivers are all of one builtin family (primitive Strings, Array exotic
//! objects, collections of one kind) and whose key the family's intrinsic
//! chain defines, while no receiver holds the key as an own property and no
//! builtin prototype is written under the key (by the program, or by a
//! store into ⊤). Whole program and per unit; per unit is never stronger.
//!
//! The receivers are typed `any`, so the HIR keeps the generic read/call (a
//! typed receiver folds to an intrinsic op), and they are the function's own
//! values, so its unit holds them too. Every negative has its control: the
//! same function without the write is a NativeMember.
use super::*;

fn modules(mods: &[(&str, &str)], per_unit: bool, cfg: &FactsConfig) -> ProgramFacts {
    let hirs: Vec<(String, perry_hir::Module)> = mods
        .iter()
        .map(|(name, src)| {
            let path = format!("{name}.ts");
            let module = perry_parser::parse_typescript(src, &path).expect("parses");
            let mut hir = perry_hir::lower_module(&module, name, &path).expect("lowers");
            for imp in &mut hir.imports {
                if let Some(n) = imp.source.strip_prefix("./") {
                    imp.resolved_path = Some(format!("{n}.ts"));
                }
            }
            (path, hir)
        })
        .collect();
    let list: Vec<(String, &perry_hir::Module)> =
        hirs.iter().map(|(p, h)| (p.clone(), h)).collect();
    match per_unit {
        true => crate::compute_units(&list, cfg),
        false => crate::compute(&list, cfg),
    }
}

fn one(src: &str, units: bool) -> ProgramFacts {
    modules(&[("t", src)], units, &FactsConfig::default())
}

/// The verdicts of the sites of key `key` in module `mi` (`call`: method
/// calls, else reads), in source order.
fn verdicts_in(pf: &ProgramFacts, mi: u32, key: &str, call: bool) -> Vec<Verdict> {
    let mut s: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.module == mi && s.key == key && s.is_call == call)
        .collect();
    s.sort_by_key(|s| s.byte_offset);
    s.iter().map(|s| s.verdict.clone()).collect()
}

fn native(v: &[Verdict]) -> bool {
    !v.is_empty() && v.iter().all(|v| *v == Verdict::NativeMember)
}

fn none_native(v: &[Verdict]) -> bool {
    !v.is_empty() && v.iter().all(|v| matches!(v, Verdict::No(_)))
}

/// `function main() { <make> <write> return <use>; }`: the control (no
/// write) is a NativeMember, the negative is not.
fn control_and_negative(make: &str, write: &str, use_: &str, key: &str, call: bool) {
    for units in [false, true] {
        let src = |w: &str| {
            format!("function main() {{ {make} {w} return {use_}; }}\nconsole.log(main());")
        };
        let pf = one(&src(""), units);
        let v = verdicts_in(&pf, 0, key, call);
        assert!(
            native(&v),
            "control units={units} {}: {v:?}\n{}",
            src(""),
            pf.dump_text()
        );
        let pf = one(&src(write), units);
        let v = verdicts_in(&pf, 0, key, call);
        assert!(
            none_native(&v),
            "units={units} {}: {v:?}\n{}",
            src(write),
            pf.dump_text()
        );
    }
}

const POSITIVE: &str = r#"
    function main(i: number, x: number, k: string) {
        const s: any = process.argv.length > 0 ? "abc" : "de";
        const a: any = [1, 2, 3];
        const m: any = new Map();
        // `new Set()` with a known `Set` folds `st.has` to an intrinsic op.
        const S: any = Set;
        const st: any = new S();
        a.push(x);
        m.set(k, 1);
        st.add(x);
        return s.charAt(i) + s.slice(1) + s.length + a.slice(-8).join(",") + m.get(k) + m.size + st.has(x);
    }
    console.log(main(1, 4, "k"));
"#;

#[test]
fn builtin_members_of_each_family_are_native_members() {
    for units in [false, true] {
        let pf = one(POSITIVE, units);
        for (key, call) in [
            ("charAt", true),
            ("slice", true),
            ("push", true),
            ("join", true),
            ("set", true),
            ("get", true),
            ("size", false),
            ("add", true),
            ("has", true),
        ] {
            let v = verdicts_in(&pf, 0, key, call);
            assert!(
                native(&v),
                "units={units} .{key}: {v:?}\n{}",
                pf.dump_text()
            );
        }
        // `length` keeps its own verdict.
        let v = verdicts_in(&pf, 0, "length", false);
        assert!(v.contains(&Verdict::OwnLength), "units={units}: {v:?}");
        // The members a weak kind has.
        let pf = one(
            "function main(k: object) { const w: any = new WeakMap(); w.set(k, 1); return w.get(k); } console.log(main({}));",
            units,
        );
        let v = verdicts_in(&pf, 0, "get", true);
        assert!(native(&v), "units={units}: {v:?}");
    }
}

/// Obligation 1: one family.
#[test]
fn a_receiver_of_two_families_is_no_native_member() {
    for (make, use_, key) in [
        // A string or an array.
        (
            "const v: any = process.argv.length > 2 ? \"abc\" : [1, 2];",
            "v.slice(1)",
            "slice",
        ),
        // A string or undefined.
        (
            "const v: any = process.argv.length > 2 ? \"abc\" : undefined;",
            "v.charAt(0)",
            "charAt",
        ),
        // A Map or a Set (both define `has`).
        (
            "const v: any = process.argv.length > 2 ? new Map() : new Set();",
            "v.has(1)",
            "has",
        ),
    ] {
        for units in [false, true] {
            let pf = one(
                &format!("function main() {{ {make} return {use_}; }}\nconsole.log(main());"),
                units,
            );
            let v = verdicts_in(&pf, 0, key, true);
            assert!(none_native(&v), "units={units} {make}: {v:?}");
        }
    }
}

/// Obligation 2: the family's prototype defines the key, per collection kind.
#[test]
fn a_key_the_family_does_not_define_is_no_native_member() {
    for units in [false, true] {
        for (make, use_, key, call) in [
            ("const a: any = [1];", "a.foo()", "foo", true),
            ("const s: any = new Set();", "s.get(1)", "get", true),
            ("const s: any = new Map();", "s.add(1)", "add", true),
            ("const w: any = new WeakMap();", "w.size", "size", false),
            ("const w: any = new WeakSet();", "w.clear()", "clear", true),
            ("const s: any = \"ab\";", "s.foo()", "foo", true),
        ] {
            let pf = one(
                &format!("function main() {{ {make} return {use_}; }}\nconsole.log(main());"),
                units,
            );
            let v = verdicts_in(&pf, 0, key, call);
            assert!(none_native(&v), "units={units} {make} {use_}: {v:?}");
        }
    }
}

/// Obligation 3: no receiver holds the key as an own property.
#[test]
fn an_own_key_on_a_receiver_is_no_native_member() {
    let arr = "const a: any = [1];";
    let map = "const a: any = new Map();";
    for (make, write, use_, key) in [
        // A named own key.
        (arr, "a.push = (x: number) => x;", "a.push(2)", "push"),
        (map, "a.get = (k: number) => k;", "a.get(1)", "get"),
        // A computed key that may be a name.
        (arr, "a[process.argv[2]] = 1;", "a.push(2)", "push"),
        (map, "a[process.argv[2]] = 1;", "a.get(1)", "get"),
        // `__proto__` and a prototype set.
        (
            arr,
            "a.__proto__ = { push() { return 0; } };",
            "a.push(2)",
            "push",
        ),
        (
            map,
            "a.__proto__ = { get() { return 0; } };",
            "a.get(1)",
            "get",
        ),
        (
            arr,
            "Object.setPrototypeOf(a, { push() { return 0; } });",
            "a.push(2)",
            "push",
        ),
        (
            map,
            "Object.setPrototypeOf(a, { get() { return 0; } });",
            "a.get(1)",
            "get",
        ),
        // Handed to unknown code, which may write any key.
        (
            arr,
            "(globalThis as any)[process.argv[2]](a);",
            "a.push(2)",
            "push",
        ),
        (
            map,
            "(globalThis as any)[process.argv[2]](a);",
            "a.get(1)",
            "get",
        ),
    ] {
        control_and_negative(make, write, use_, key, true);
    }
}

/// Obligation 4 (c): a program write onto the family's prototype, in this
/// module or in another one (per unit: another unit's write).
#[test]
fn a_program_write_onto_a_builtin_prototype_is_no_native_member() {
    let user = "export function run() { const a: any = [1, 2]; return a.indexOf(0); }";
    for units in [false, true] {
        let alone = modules(
            &[
                ("main", "import { run } from \"./u\";\nconsole.log(run());"),
                ("u", user),
            ],
            units,
            &FactsConfig::default(),
        );
        let v = verdicts_in(&alone, 1, "indexOf", true);
        assert!(
            native(&v),
            "control units={units}: {v:?}\n{}",
            alone.dump_text()
        );
        for write in [
            "(Array.prototype as any).indexOf = function () { return 7; };",
            "(Object.prototype as any).indexOf = function () { return 7; };",
            "(Array.prototype as any)[process.argv[2]] = 1;",
        ] {
            // The same module.
            let pf = one(&format!("{write}\n{user}\nconsole.log(run());"), units);
            let v = verdicts_in(&pf, 0, "indexOf", true);
            assert!(none_native(&v), "units={units} {write}: {v:?}");
            // Another module.
            let pf = modules(
                &[
                    (
                        "main",
                        &format!("import {{ run }} from \"./u\";\n{write}\nconsole.log(run());"),
                    ),
                    ("u", user),
                ],
                units,
                &FactsConfig::default(),
            );
            let v = verdicts_in(&pf, 1, "indexOf", true);
            assert!(
                none_native(&v),
                "units={units} other module {write}: {v:?}\n{}",
                pf.dump_text()
            );
        }
        // A write under another key keeps it.
        let pf = one(
            &format!("(Array.prototype as any).other = 1;\n{user}\nconsole.log(run());"),
            units,
        );
        let v = verdicts_in(&pf, 0, "indexOf", true);
        assert!(native(&v), "units={units}: {v:?}");
    }
}

/// Obligation 4 (d): a store into ⊤ may write a builtin prototype. On a
/// String the site's reason names it (the sites only it keeps); an array's
/// call of the now unknown member hands the array to unknown code as well.
#[test]
fn a_store_into_top_is_no_native_member_while_it_holds() {
    for units in [false, true] {
        for store in ["ext.indexOf = 1;", "ext[process.argv[3]] = 1;"] {
            let src = |s: &str| {
                format!(
                    r#"
                    const ext: any = (globalThis as any)[process.argv[2]];
                    {s}
                    function main() {{
                        const s: any = process.argv.length > 0 ? "ab" : "c";
                        const a: any = [1, 2];
                        return s.indexOf("b") + a.indexOf(0);
                    }}
                    console.log(main());
                "#
                )
            };
            let pf = one(&src(""), units);
            let v = verdicts_in(&pf, 0, "indexOf", true);
            assert!(native(&v), "control units={units}: {v:?}");
            let pf = one(&src(store), units);
            let v = verdicts_in(&pf, 0, "indexOf", true);
            assert!(
                matches!(v.as_slice(), [Verdict::No(w), Verdict::No(_)] if w.starts_with("native member: a store into ⊤")),
                "units={units} {store}: {v:?}"
            );
        }
    }
}

/// E23: an accessor on `Object.prototype` is a program-wide kill.
#[test]
fn an_object_prototype_accessor_is_no_native_member() {
    for units in [false, true] {
        let pf = one(
            r#"
            Object.defineProperty(Object.prototype, "zz", { get() { return 1; } });
            function main() { const a: any = [1, 2]; return a.indexOf(0); }
            console.log(main());
        "#,
            units,
        );
        let v = verdicts_in(&pf, 0, "indexOf", true);
        assert!(none_native(&v), "units={units}: {v:?}");
    }
}

/// (e) Interpreted code: the verdict stands behind the guard word, as T0's.
#[test]
fn interpreted_code_guards_a_native_member() {
    let cfg = FactsConfig {
        dynamic_code: Some("test-interpreter".into()),
        ..FactsConfig::default()
    };
    for units in [false, true] {
        let pf = modules(
            &[(
                "t",
                r#"
                new Function(process.argv[2])();
                function main() { const a: any = [1, 2]; return a.indexOf(0); }
                console.log(main());
            "#,
            )],
            units,
            &cfg,
        );
        assert!(pf.interp_guard.is_some(), "units={units}");
        let v = verdicts_in(&pf, 0, "indexOf", true);
        assert!(native(&v), "units={units}: {v:?}");
    }
}

/// Per unit never stronger than whole. Values another unit made (a
/// parameter of an exported function) are NativeMember whole only.
#[test]
fn per_unit_is_never_stronger_than_whole() {
    let srcs = [
        POSITIVE,
        "function f(v: any) { return v.slice(1) + v.join(\",\"); } console.log(f([1, 2, 3]));",
        "function main() { const a: any = [1]; a.push = (x: number) => x; return a.push(2); } console.log(main());",
        "(Array.prototype as any).indexOf = function () { return 7; }; function main() { const a: any = [1]; return a.indexOf(0); } console.log(main());",
    ];
    for src in srcs {
        let w = one(src, false);
        let u = one(src, true);
        for s in u
            .sites
            .iter()
            .filter(|s| s.verdict == Verdict::NativeMember)
        {
            let whole = w.sites.iter().find(|x| {
                x.module == s.module
                    && x.byte_offset == s.byte_offset
                    && x.key == s.key
                    && x.is_call == s.is_call
            });
            assert!(
                whole.is_some_and(|x| x.verdict == Verdict::NativeMember),
                "{src}: .{} per unit NativeMember, whole {:?}",
                s.key,
                whole.map(|x| &x.verdict)
            );
        }
    }
    // The parameter case: proven whole.
    let w = one(
        "function f(v: any) { return v.slice(1) + v.join(\",\"); } console.log(f([1, 2, 3]));",
        false,
    );
    assert!(
        native(&verdicts_in(&w, 0, "join", true)),
        "{}",
        w.dump_text()
    );
}
