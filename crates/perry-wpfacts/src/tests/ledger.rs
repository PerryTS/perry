//! LEDGER: every unproven site names the first unknown value on its proof
//! path from a closed list, and the dump has one record per HIR site.
use super::*;

fn diag() -> FactsConfig {
    FactsConfig {
        diagnostics: true,
        ..Default::default()
    }
}

fn whole(src: &str) -> ProgramFacts {
    facts_with(src, diag())
}

fn units(mods: &[(&str, &str)]) -> ProgramFacts {
    let parsed: Vec<_> = mods
        .iter()
        .map(|(p, s)| {
            let m = perry_parser::parse_typescript(s, p).expect("parses");
            let name = p.trim_end_matches(".ts");
            let mut hir = perry_hir::lower_module(&m, name, p).expect("lowers");
            for imp in &mut hir.imports {
                if let Some(n) = imp.source.strip_prefix("./") {
                    imp.resolved_path = Some(format!("{n}.ts"));
                }
            }
            hir
        })
        .collect();
    let list: Vec<(String, &perry_hir::Module)> = mods
        .iter()
        .zip(&parsed)
        .map(|((p, _), h)| (p.to_string(), h))
        .collect();
    let mut cfg = diag();
    cfg.max_work = Some(50_000_000);
    crate::compute_units(&list, &cfg)
}

/// The cause names of every site record of `kind` with `key`, in every
/// analysis unit.
fn causes(pf: &ProgramFacts, kind: &str, key: &str) -> Vec<String> {
    let want = format!(
        "\"kind\":{},\"key\":{}",
        crate::diagnostics::quote(kind),
        crate::diagnostics::quote(key)
    );
    let mut out = Vec::new();
    for (_, lines) in &pf.diagnostics {
        for l in lines
            .iter()
            .filter(|l| l.contains("\"record\":\"site\"") && l.contains(&want))
        {
            let Some(at) = l.find("\"causes\":[") else {
                continue;
            };
            let rest = &l[at + 10..];
            let list = &rest[..rest.find(']').unwrap_or(0)];
            for part in list.split("\"cause\":\"").skip(1) {
                out.push(part[..part.find('"').unwrap()].to_string());
            }
        }
    }
    out
}

fn completeness(pf: &ProgramFacts) -> Vec<String> {
    pf.diagnostics
        .iter()
        .filter(|(u, _)| u == "<census>")
        .flat_map(|(_, l)| l.iter().filter(|l| l.contains("\"completeness\"")).cloned())
        .collect()
}

/// Every seed text (`add_top` / `top_val` / `let why`) and escape reason
/// (`reason`) in the crate names a cause: a new origin without a named
/// member fails here instead of reaching a dump as `unclassified`.
#[test]
fn every_seed_text_has_a_cause() {
    fn files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                if p.file_name().is_some_and(|n| n != "tests") {
                    files(&p, out);
                }
            } else if p.extension().is_some_and(|x| x == "rs")
                && !p.ends_with("tests.rs")
                && !p.ends_with("ledger.rs")
            {
                out.push(p);
            }
        }
    }
    // The first string literal opening within `window` bytes of `at`.
    fn literal(s: &str, at: usize, window: usize) -> Option<String> {
        let end = (at + window).min(s.len());
        let q = at + s.get(at..end)?.find('"')?;
        let rest = &s[q + 1..];
        let close = rest.find('"')?;
        Some(rest[..close].to_string())
    }
    // `{..}` placeholders stand for one word.
    fn filled(lit: &str) -> String {
        let mut out = String::new();
        let mut depth = 0;
        for c in lit.chars() {
            match c {
                '{' => {
                    depth += 1;
                    if depth == 1 {
                        out.push('x');
                    }
                }
                '}' => depth -= 1,
                c if depth == 0 => out.push(c),
                _ => {}
            }
        }
        out
    }
    let mut paths = Vec::new();
    files(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut paths,
    );
    let mut checked = 0;
    let mut missing = Vec::new();
    for p in paths {
        let s = std::fs::read_to_string(&p).unwrap();
        let mut seeds = Vec::new();
        let mut reasons = Vec::new();
        for (needle, window, reason) in [
            ("add_top(", 160, false),
            ("top_val(", 60, false),
            ("let why = ", 120, false),
            ("reason(\"", 10, true),
            ("to_sink(", 120, true),
            ("reason(&format!(\"", 24, true),
        ] {
            for (at, _) in s.match_indices(needle) {
                if needle == "add_top(" && s[..at].ends_with("fn ") {
                    continue;
                }
                if let Some(l) = literal(&s, at, window) {
                    if reason {
                        reasons.push(l)
                    } else {
                        seeds.push(l)
                    }
                }
            }
        }
        for lit in seeds {
            // Verdict texts (`escape: {e}`) are checked by the verdict test.
            if ["escape: ", "hard: ", "chain: "]
                .iter()
                .any(|p| lit.starts_with(p))
            {
                continue;
            }
            let f = filled(&lit.replace("{UNMODELED_OPERAND}", crate::gen_expr::UNMODELED_OPERAND));
            checked += 1;
            let ok = match f.split_once(": ") {
                // `<what>: {why}`: the why is an escape reason (below).
                Some((what, "x")) => crate::ledger::head(what).is_some(),
                _ => crate::ledger::classify(&f).is_some(),
            };
            if !ok {
                missing.push(format!("{}: seed {lit:?}", p.display()));
            }
        }
        for lit in reasons {
            checked += 1;
            if crate::ledger::route(&filled(&lit)).is_none() {
                missing.push(format!("{}: reason {lit:?}", p.display()));
            }
        }
    }
    assert!(checked > 200, "the scan found only {checked} texts");
    assert!(
        missing.is_empty(),
        "texts without a cause:\n{}",
        missing.join("\n")
    );
}

/// Every legacy verdict reason of `derive.rs` names a cause.
#[test]
fn every_verdict_reason_has_a_cause() {
    let s = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/derive.rs"),
    )
    .unwrap();
    let mut n = 0;
    for (at, _) in s.match_indices("\"") {
        let rest = &s[at + 1..];
        let Some(close) = rest.find('"') else {
            continue;
        };
        let lit = &rest[..close];
        // Every literal a verdict or a lookup failure is built from.
        let before = &s[..at];
        let built = ["Err(", "No(", "Err(format!(", "No(format!("]
            .iter()
            .any(|p| before.ends_with(p));
        let reason = built
            || [
                "receiver ",
                "dead site",
                "escape: ",
                "hard: ",
                "chain: ",
                "slots ",
                "key not",
                "accessor key",
                "primitive receiver",
                "callee set holds",
                "not a constant static",
            ]
            .iter()
            .any(|p| lit.starts_with(p));
        if !reason {
            continue;
        }
        n += 1;
        let v = Verdict::No(lit.replace("{e}", "passed to a call of an unknown function value"));
        assert!(
            !matches!(crate::ledger::verdict_cause(&v), Some(Err(_))),
            "verdict reason without a cause: {lit:?}"
        );
    }
    assert!(n >= 12, "found only {n} verdict reasons");
}

macro_rules! cause_test {
    ($name:ident, $cause:literal, $kind:literal, $key:literal, $src:expr) => {
        #[test]
        fn $name() {
            let pf = whole($src);
            let got = causes(&pf, $kind, $key);
            assert!(
                got.iter().any(|c| c == $cause),
                "{} not in {:?}",
                $cause,
                got
            );
        }
    };
}

/// A legacy verdict names the failed obligation (its operands are known).
macro_rules! verdict_test {
    ($name:ident, $cause:ident, $key:literal, $src:expr) => {
        #[test]
        fn $name() {
            let pf = whole($src);
            let got: Vec<_> = pf
                .sites
                .iter()
                .filter(|s| s.key == $key)
                .map(|s| crate::ledger::verdict_cause(&s.verdict))
                .collect();
            assert!(
                got.iter()
                    .any(|c| matches!(c, Some(Ok(crate::ledger::Unknown::$cause)))),
                "{} not in {:?}",
                stringify!($cause),
                got
            );
        }
    };
}

cause_test!(
    escaped_parameter,
    "escaped-parameter",
    "property",
    "x",
    r#"const g: any = (globalThis as any).hostFn; g(function (o: any) { return o.x; });"#
);
cause_test!(
    escaped_this,
    "escaped-this",
    "property",
    "y",
    r#"const g: any = (globalThis as any).hostFn; g({ m() { return (this as any).y; } });"#
);
cause_test!(
    intrinsic_prototype_top_store,
    "intrinsic-prototype-top-store",
    "property",
    "q",
    r#"const g: any = (globalThis as any).hostFn; (Array.prototype as any)[g()] = g(); const m: any = [1].map; console.log(m.q);"#
);
cause_test!(
    opaque_builtin,
    "opaque-builtin",
    "property",
    "a",
    r#"const c: any = structuredClone({ a: 1 }); console.log(c.a);"#
);
cause_test!(
    computed_key,
    "computed-key",
    "property",
    "w",
    r#"const g: any = (globalThis as any).hostFn; console.log((globalThis as any)[g()].w);"#
);
cause_test!(
    escaped_object_field,
    "escaped-object-field",
    "property",
    "b",
    r#"const g: any = (globalThis as any).hostFn; const o = { a: { b: 1 } }; g(o); console.log(o.a.b);"#
);
cause_test!(
    external_input,
    "external-input",
    "value-call",
    "write",
    r#"process.stdout.write("x");"#
);
cause_test!(
    unmodeled_construct,
    "unmodeled-construct",
    "property",
    "x",
    r#"function F(this: any) { return (new.target as any).x; } new (F as any)();"#
);
cause_test!(
    no_values,
    "no-values",
    "property",
    "z",
    r#"const g: any = JSON.parse("0"); const r = g(); console.log(r.z);"#
);
cause_test!(
    function_without_identity,
    "function-without-identity",
    "property",
    "x",
    r#"const g: any = (globalThis as any).hostFn; const o: any = {}; Object.defineProperty(o, "p", { get: g }); console.log(o.p.x);"#
);
cause_test!(
    reflective_operation,
    "reflective-operation",
    "property",
    "b",
    r#"const g: any = (globalThis as any).hostFn; const t = { a: { b: 1 } }; new Proxy(t, g()); console.log(t.a.b);"#
);

verdict_test!(
    polymorphic_beyond_set,
    PolymorphicBeyondSet,
    "x",
    r#"class A { x = 1; y = 2 } class B { y = 3; x = 4 } const o = Math.random() > 0.5 ? new A() : new B(); console.log(o.x);"#
);
verdict_test!(
    primitive_receiver,
    PrimitiveReceiver,
    "x",
    r#"const a: any[] = []; console.log(a[0].x);"#
);
verdict_test!(
    non_function_callee,
    NonFunctionCallee,
    "m",
    r#"const o: any = { m: 1 }; o.m();"#
);
verdict_test!(
    hard_layout,
    HardLayout,
    "y",
    r#"const o: any = { x: 1, y: 2 }; delete o.x; console.log(o.y);"#
);
verdict_test!(
    accessor_key,
    AccessorKey,
    "x",
    r#"class A { get x() { return 1; } } console.log(new A().x);"#
);
verdict_test!(
    builtin_receiver,
    BuiltinReceiver,
    "q",
    r#"function f() {} console.log((f as any).q);"#
);
verdict_test!(
    receiver_not_a_layout,
    ReceiverNotALayout,
    "q",
    r#"console.log(JSON.parse("{}").q);"#
);

// Interpreted code: `Function.call` runs the Function constructor (E14).
cause_test!(
    dynamic_code,
    "dynamic-code",
    "property",
    "x",
    r#"const r: any = (Function as any).call(null, "return 1"); console.log(r.x);"#
);

/// B-codes mark a typed array hard, and a typed array is no fact unit, so
/// no site's first unknown value is a B-code; the escape texts still name
/// their member.
#[test]
fn buffer_aliasing() {
    use crate::ledger::{classify, Unknown};
    assert_eq!(
        classify("field of escaped array: B1 typed array over a shared buffer"),
        Some(Unknown::BufferAliasing)
    );
    assert_eq!(
        classify("B2 buffer transfer/resize"),
        Some(Unknown::BufferAliasing)
    );
}

/// The unit edge: another unit's parameter, receiver and value, and a
/// namespace re-export the program does not resolve.
#[test]
fn unit_edge_causes() {
    let pf = units(&[
        ("u.ts", "export function f(o: any) { return o.x; } export class C { m() { return this.y; } } export function mk(): any { return (globalThis as any).q; }"),
        ("a.ts", "export const v = { w: 1 };"),
        ("b.ts", "export const v = { w: 2 };"),
        ("c.ts", "export * from \"./a\"; export * from \"./b\";"),
        ("t.ts", "import { f, C, mk } from \"./u\"; import * as c from \"./c\"; const h: any = (globalThis as any).hostFn; h(f); h(C); console.log(mk().z, (c as any).v.w);"),
    ]);
    for (key, cause) in [
        ("x", "unit-edge-parameter"),
        ("y", "unit-edge-this"),
        ("z", "unit-edge-value"),
        ("w", "module-namespace"),
    ] {
        let got = causes(&pf, "property", key);
        assert!(
            got.iter().any(|c| c == cause),
            "{key}: {cause} not in {got:?}"
        );
    }
}

/// One record per HIR site: the census of a program with every site kind
/// accounts for each, and no recorded site is outside the HIR.
#[test]
fn every_hir_site_has_one_record() {
    let pf = whole(
        r#"class A { x = 1; m() { return this.x; } }
        const a = new A(); const o: any = { k: 1 }; const arr = [1, 2];
        function put(t: any, j: any, v: any) { t[j] = v; } put(o, "j", 1); o.k = a.m(); o.k++; arr[1]++;
        const f = (v: any) => v.k; console.log(f(o), new (A as any)(), [...arr].length);"#,
    );
    let lines = completeness(&pf);
    let strays: Vec<_> = pf
        .diagnostics
        .iter()
        .flat_map(|(_, l)| l.iter().filter(|l| l.contains("\"stray\"")))
        .collect();
    assert!(strays.is_empty(), "{strays:?}");
    assert!(
        lines.iter().any(|l| l.contains("\"recordedNotInHir\":0")),
        "{lines:?}"
    );
    for l in lines.iter().filter(|l| l.contains("\"hir\":")) {
        let num = |k: &str| {
            let at = l.find(&format!("\"{k}\":")).unwrap() + k.len() + 3;
            l[at..]
                .split(|c: char| !c.is_ascii_digit())
                .next()
                .unwrap()
                .parse::<u64>()
                .unwrap()
        };
        assert_eq!(
            num("hir"),
            num("recorded") + num("recordedThroughCopy") + num("calleeOfCall") + num("unrecorded"),
            "{l}"
        );
        assert_eq!(num("unrecorded"), 0, "{l}");
    }
    for kind in [
        "\"kind\":\"write\"",
        "\"kind\":\"update\"",
        "\"kind\":\"element-update\"",
    ] {
        assert!(
            lines.iter().any(|l| l.contains(kind)),
            "no {kind} sites: {lines:?}"
        );
    }
}

/// Print the causes of each probe program (run with `--ignored
/// --nocapture`): the probes the per-cause tests below pin.
#[test]
#[ignore]
fn ledger_probe() {
    let probes: &[(&str, &str, &str, &str)] = &[
        (
            "dynamic",
            "property",
            "x",
            r#"const r: any = (Function("return { x: 1 }") as any)(); console.log(r.x);"#,
        ),
        (
            "dynamic2",
            "property",
            "x",
            r#"const f: any = new Function("return { x: 1 }"); const r: any = f(); console.log(r.x);"#,
        ),
        (
            "buffer",
            "property",
            "q",
            r#"const u = new Uint8Array(new ArrayBuffer(8)); const b: any = u.buffer; console.log(b.q);"#,
        ),
        (
            "buffer2",
            "property",
            "q",
            r#"const b = new ArrayBuffer(8); const u = new Uint8Array(b); const v: any = new DataView(u.buffer); console.log(v.q);"#,
        ),
    ];
    for (name, kind, key, src) in probes {
        let pf = if *name == "dynamic" {
            facts_with(
                src,
                FactsConfig {
                    diagnostics: true,
                    ..interp_cfg()
                },
            )
        } else {
            whole(src)
        };
        println!("PROBE whole {name}: {:?}", causes(&pf, kind, key));
        for (_, lines) in &pf.diagnostics {
            for l in lines.iter().filter(|l| {
                l.contains("\"record\":\"site\"") && l.contains(&format!("\"key\":\"{key}\""))
            }) {
                if let Some(at) = l.find("\"causes\":[") {
                    println!("PROBE line {name}: {}", &l[at..l.len().min(at + 300)]);
                }
            }
        }
        let v: Vec<_> = pf
            .sites
            .iter()
            .filter(|s| s.key == *key)
            .map(|s| &s.verdict)
            .collect();
        println!("PROBE verdict {name}: {v:?}");
    }
    let pf = units(&[
        ("u.ts", "export function f(o: any) { return o.x; } export class C { m() { return this.y; } } export function mk(): any { return (globalThis as any).q; }"),
        ("a.ts", "export const v = { w: 1 };"),
        ("b.ts", "export const v = { w: 2 };"),
        ("c.ts", "export * from \"./a\"; export * from \"./b\";"),
        ("t.ts", "import { f, C, mk } from \"./u\"; import * as c from \"./c\"; const h: any = (globalThis as any).hostFn; h(f); h(C); console.log(mk().z, (c as any).v.w);"),
    ]);
    for (kind, key) in [
        ("property", "x"),
        ("property", "y"),
        ("property", "z"),
        ("property", "w"),
    ] {
        println!("PROBE units {key}: {:?}", causes(&pf, kind, key));
        let v: Vec<_> = pf
            .sites
            .iter()
            .filter(|s| s.key == key)
            .map(|s| &s.verdict)
            .collect();
        println!("PROBE units verdict {key}: {v:?}");
    }
    println!("PROBE completeness {:?}", completeness(&pf));
}

/// The closed list of reasons a published value contract is unknown
/// (`ValueContract::unknown_because` / `unknown_for`, dump field `why`).
const UNKNOWN_WHY: &[&str] = &[
    "unit-aborted",
    "undescribed-store-value",
    "undescribed-cell-write",
    "global-written-by-unmodeled-code",
    "array-escaped",
    "binding-write-over-the-bound",
    "opaque-receiver",
    "top-value",
    "escaped-function-without-contract",
    "class-without-contract",
    "argument-marker-reach",
    "function-without-publication",
    "non-object-value",
    "parameter-marker",
    "broken-layout",
    "layout-without-contract",
];

/// Every reason a source file gives (`unknown_because("..")`,
/// `unknown_for("..")`, an `if` choosing between two literals) is in the
/// closed list, and every member of the list is given somewhere.
#[test]
fn every_unknown_reason_is_named() {
    fn files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                if p.file_name().is_some_and(|n| n != "tests") {
                    files(&p, out);
                }
            } else if p.extension().is_some_and(|x| x == "rs") && !p.ends_with("tests.rs") {
                out.push(p);
            }
        }
    }
    let mut paths = Vec::new();
    files(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut paths,
    );
    let mut given = std::collections::BTreeSet::new();
    for p in paths {
        let s = std::fs::read_to_string(&p).unwrap();
        for needle in ["unknown_because(", "unknown_for(", "let because = "] {
            for (at, _) in s.match_indices(needle) {
                // The literals up to the end of the statement or call.
                let end = (at + 300).min(s.len());
                let end = s[at..end].find(';').map_or(end, |e| at + e);
                let line = &s[at + needle.len()..end];
                for (i, part) in line.split('"').enumerate() {
                    if i % 2 == 1 && part.chars().all(|c| c.is_ascii_lowercase() || c == '-') {
                        if needle == "let because = " && !UNKNOWN_WHY.contains(&part) {
                            continue; // another variable
                        }
                        given.insert(part.to_string());
                    }
                }
            }
        }
    }
    for g in &given {
        assert!(UNKNOWN_WHY.contains(&g.as_str()), "unnamed reason {g}");
    }
    for w in UNKNOWN_WHY {
        assert!(given.contains(*w), "reason {w} is given nowhere");
    }
}

/// An unknown published value names why, in the value-output,
/// argument-output and contract-input records; a known one says null.
#[test]
fn unknown_values_carry_a_reason() {
    let pf = units(&[
        ("u.ts", "export function f(o: any) { return o.x; } export function mk(): any { return (globalThis as any).q; } export const g: any = (globalThis as any).hostValue;"),
        ("t.ts", "import { f, mk, g } from \"./u\"; const h: any = (globalThis as any).hostFn; h(f); console.log(mk().z, g.k); f({ a: 1 });"),
    ]);
    let (mut unknown, mut with_why, mut known_with_why) = (0, 0, 0);
    for (_, lines) in &pf.diagnostics {
        for l in lines.iter().filter(|l| {
            l.contains("\"record\":\"contract-input\"")
                || l.contains("\"record\":\"value-output\"")
                || l.contains("\"record\":\"argument-output\"")
        }) {
            let why = l.contains("\"why\":{\"reason\":\"");
            if l.contains("\"unknown\":true") || l.contains(";unknown=true;") {
                unknown += 1;
                with_why += why as usize;
            } else if why {
                known_with_why += 1;
            }
        }
    }
    assert!(unknown > 0, "the program publishes no unknown value");
    assert_eq!(unknown, with_why, "an unknown value without a reason");
    assert_eq!(known_with_why, 0, "a reason on a known value");
}
