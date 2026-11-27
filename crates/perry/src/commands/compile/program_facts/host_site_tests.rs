//! HOSTSURF H5: a proven host site compiles to the direct call of §3.2: the
//! registry load (`perry_host_prototype_v1`), ONE own-slot read
//! (`perry_host_own_member_v1`) and the provider's endpoint call
//! (`perry_binding_native_call_v1`); no by-name dispatch
//! (`js_method_call_slow`) and no generic [[Set]] (`js_put_value_set`, kept
//! only on a nullable receiver's nullish edge, which throws). The facts come
//! from the H2 test provider (`perry-wpfacts` `tests/hostsurf/tdom.json`);
//! the IR is emitted only (nothing links the provider's endpoint). The
//! soundness fixtures F1-F5 keep their unproven sites on the generic path.
use super::*;

const TDOM: &str = include_str!("../../../../../perry-wpfacts/src/tests/hostsurf/tdom.json");
const CLICK: &str =
    include_str!("../../../../../perry-wpfacts/src/tests/hostsurf/native_click.ts");
const NATIVE_CALL: &str = "@perry_binding_native_call_v1(";

fn host() -> &'static perry_wpfacts::host::HostRealm {
    let s = perry_hir::realm_manifest::host_surface::parse(TDOM, "tdom.json").expect("parses");
    perry_wpfacts::host::load(&[s])
        .expect("loads")
        .expect("one surface")
}

/// The module's facts with the test provider, its published elision facts
/// and the module's IR.
fn compile(src: &str) -> (perry_wpfacts::ProgramFacts, perry_codegen::ElisionFacts, String) {
    let ast = perry_parser::parse_typescript(src, "main.ts").expect("parses");
    let hir = perry_hir::lower_module(&ast, "main", "main.ts").expect("lowers");
    let pf = perry_wpfacts::compute(
        &[("main.ts".into(), &hir)],
        &perry_wpfacts::FactsConfig {
            namespace_pairs: namespace_pairs_on(),
            host: Some(host()),
            ..Default::default()
        },
    );
    assert!(!pf.stats.aborted, "{}", pf.dump_text());
    let elision = elision_for(&pf, "main.ts", &|_, _, _, _, _| None);
    let ir = ir_of(&hir, elision.clone());
    (pf, elision, ir)
}

fn ir_of(hir: &perry_hir::Module, elision: perry_codegen::ElisionFacts) -> String {
    let opts = perry_codegen::CompileOptions {
        emit_ir_only: true,
        program_facts: perry_codegen::ProgramFactsView {
            elision: std::sync::Arc::new(elision),
            ..Default::default()
        },
        ..Default::default()
    };
    let bytes = perry_codegen::compile_module(hir, opts).expect("compiles");
    String::from_utf8(bytes).expect("LLVM IR is UTF-8")
}

fn calls(ir: &str, callee: &str) -> usize {
    ir.lines()
        .filter(|l| l.contains("call ") && l.contains(callee))
        .count()
}

/// The proven host sites of the facts: `(offset, key, store)` of every site
/// with a host verdict whose holder is a registered provider object.
fn proven(pf: &perry_wpfacts::ProgramFacts) -> Vec<(u32, String, bool)> {
    use perry_wpfacts::Verdict;
    let mut v: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| {
            matches!(&s.verdict,
                Verdict::HostMember { holder: Some(_), .. }
                | Verdict::HostGet { holder: Some(_), .. }
                | Verdict::HostSet { holder: Some(_), .. })
        })
        .map(|s| (s.byte_offset, s.key.clone(), s.access.is_store()))
        .collect();
    v.sort();
    v.dedup();
    v
}

fn published(e: &perry_codegen::ElisionFacts) -> usize {
    e.host_member_calls.len() + e.host_accessor_sites.len()
}

/// Every `js_put_value_set` call stands in a nullable store's nullish arm
/// (`wpf.set.nullish*`), which ends in `unreachable`.
fn put_value_only_on_nullish_edges(ir: &str) -> bool {
    let mut label = "";
    for l in ir.lines() {
        if !l.starts_with(' ') && l.ends_with(':') {
            label = l;
        }
        if l.contains("call ") && l.contains("@js_put_value_set(") && !label.starts_with("wpf.set.nullish")
        {
            return false;
        }
    }
    true
}

const DOM: &str = "const document: any = (globalThis as any).document;\n\
                   const records: any = document.getElementById('records');\n\
                   const cell: any = document.createElement('p');\n\
                   records.appendChild(cell);\n\
                   cell.textContent = 'x';\n\
                   records.removeChild(cell);\n";

#[test]
fn hsb4_proven_sites_call_the_endpoint() {
    let (pf, e, ir) = compile(DOM);
    // getElementById, createElement, appendChild, removeChild; textContent.
    assert_eq!(e.host_member_calls.len(), 4, "{:?}\n{}", e.host_member_calls, pf.dump_text());
    assert_eq!(e.host_accessor_sites.len(), 1, "{:?}", e.host_accessor_sites);
    assert_eq!(published(&e), proven(&pf).len());
    assert!(e
        .host_accessor_sites
        .keys()
        .all(|(_, k, kind)| k == "textContent" && *kind == perry_codegen::AccessorKind::Set));
    // One registry load, one own-slot read and one endpoint call per site.
    assert_eq!(calls(&ir, NATIVE_CALL), 5, "{ir}");
    assert_eq!(calls(&ir, "@perry_host_prototype_v1("), 5);
    assert_eq!(calls(&ir, "@perry_host_own_member_v1("), 5);
    assert_eq!(calls(&ir, "@js_method_call_slow("), 0, "{ir}");
    assert!(put_value_only_on_nullish_edges(&ir), "{ir}");
    // The setter is read as part 2 of its holder's own accessor.
    assert!(ir.contains("@perry_host_own_member_v1(double") && ir.contains(", i32 2)"), "{ir}");
    // The same module without the host facts: the by-name dispatch.
    let ast = perry_parser::parse_typescript(DOM, "main.ts").unwrap();
    let hir = perry_hir::lower_module(&ast, "main", "main.ts").unwrap();
    let mut bare = e.clone();
    bare.host_member_calls.clear();
    bare.host_accessor_sites.clear();
    let generic = ir_of(&hir, bare);
    assert_eq!(calls(&generic, NATIVE_CALL), 0);
    assert!(calls(&generic, "@js_method_call_slow(") > 0, "{generic}");
}

#[test]
fn hsb4_workload_publishes_every_proven_site() {
    let (pf, e, ir) = compile(CLICK);
    let p = proven(&pf);
    assert_eq!(published(&e), p.len(), "{:?}", p);
    // The 25 textContent stores of the workload's two builders, status and
    // benchmarkState included, are host setter sites.
    let sets = e
        .host_accessor_sites
        .keys()
        .filter(|(_, k, kind)| k == "textContent" && *kind == perry_codegen::AccessorKind::Set)
        .count();
    assert!(sets >= 15, "{sets}: {:?}", e.host_accessor_sites);
    assert!(calls(&ir, NATIVE_CALL) >= published(&e));
    assert_eq!(calls(&ir, NATIVE_CALL), calls(&ir, "@perry_host_own_member_v1("));
    assert!(put_value_only_on_nullish_edges(&ir));
}

/// F1-F5 (HOSTSURF §5): the sites the program can change keep the generic
/// path; the host sites beside them still take the endpoint.
#[test]
fn hsb4_soundness_fixtures_keep_unproven_sites_generic() {
    let head = "const document: any = (globalThis as any).document;\n\
                const records: any = document.getElementById('records');\n";
    let fixtures: [(&str, &str); 6] = [
        ("F1a", "function f(this: any, n: any): any { return n; }\n\
                 (globalThis as any).HTMLElement.prototype.appendChild = f;\n\
                 const row: any = document.createElement('article');\n\
                 row.appendChild(document.createElement('p'));\n\
                 records.appendChild(row);\n"),
        ("F1b", "function f(this: any, n: any): any { return n; }\n\
                 Object.getPrototypeOf(Object.getPrototypeOf(document.createElement('p'))).appendChild = f;\n\
                 const row: any = document.createElement('article');\n\
                 row.appendChild(document.createElement('p'));\n\
                 records.appendChild(row);\n"),
        ("F2", "const k = String(Math.random());\n\
                (globalThis as any).HTMLElement.prototype[k] = function (n: any): any { return n; };\n\
                records.appendChild(document.createElement('p'));\n"),
        ("F3", "const row: any = document.createElement('article');\n\
                row.appendChild = function (n: any): any { return n; };\n\
                row.appendChild(document.createElement('p'));\n\
                records.appendChild(document.createElement('p'));\n"),
        ("F4", "const row: any = document.createElement('article');\n\
                Object.setPrototypeOf(row, { appendChild(n: any): any { return n; } });\n\
                row.appendChild(document.createElement('p'));\n"),
        ("F5", "const cell: any = document.createElement('p');\n\
                records.appendChild(cell);\n\
                records.addEventListener('click', function (): void { cell.mark = { n: 1 }; });\n\
                const back: any = document.getElementById('records');\n\
                console.log(back.mark);\n"),
    ];
    for (name, body) in fixtures {
        let src = format!("{head}{body}");
        let (pf, e, ir) = compile(&src);
        let p = proven(&pf);
        assert_eq!(published(&e), p.len(), "{name}: {p:?}");
        // Exactly the published sites call the endpoint.
        assert_eq!(calls(&ir, NATIVE_CALL), published(&e), "{name}\n{ir}");
        // Every site that is not proven keeps the ordinary lowering.
        for s in pf.sites.iter().filter(|s| s.key == "appendChild" || s.key == "mark") {
            let host = e
                .host_member_calls
                .contains_key(&(s.byte_offset, s.key.clone()))
                || e
                    .host_accessor_sites
                    .keys()
                    .any(|(o, k, _)| *o == s.byte_offset && *k == s.key);
            let is_host = p.iter().any(|(o, k, _)| *o == s.byte_offset && *k == s.key);
            assert_eq!(host, is_host, "{name}: {} {:?}", s.key, s.verdict);
        }
        match name {
            // The program's function / the unknown-key store / the relink:
            // no appendChild site is a host site.
            "F1a" | "F1b" | "F2" | "F4" => assert!(
                !p.iter().any(|(_, k, _)| k == "appendChild"),
                "{name}: {p:?}"
            ),
            // The expando's own receiver stays generic; `records` is proven.
            "F3" => assert_eq!(
                p.iter().filter(|(_, k, _)| k == "appendChild").count(),
                1,
                "{name}: {p:?}"
            ),
            // The expando reads and stores stay program accesses.
            _ => assert!(!p.iter().any(|(_, k, _)| k == "mark"), "{name}: {p:?}"),
        }
    }
}
