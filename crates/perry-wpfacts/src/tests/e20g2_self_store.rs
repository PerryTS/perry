//! E20G2: the global object stored onto its own named key, bare global
//! reads after a program store.
use super::*;

fn facts(src: &str) -> ProgramFacts {
    let path = "main.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let mut pb = perry_hir::patched_builtins::PatchedBuiltins::new();
    perry_hir::scan_patched_builtins(&module, &mut pb);
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    perry_hir::patched_builtins::set_patched_builtins(std::sync::Arc::new(pb));
    let hir = perry_hir::lower_module(&module, "main", &path).expect("lowers");
    perry_hir::patched_builtins::clear_patched_builtins();
    crate::compute(&[(path, &hir)], &cfg)
}

fn v_proven(src: &str) -> bool {
    let pf = facts(src);
    let at = src.find("/*A*/").expect("marker") as u32;
    let s = pf
        .sites
        .iter()
        .filter(|s| s.key == "v" && !s.is_call && s.byte_offset >= at)
        .min_by_key(|s| s.byte_offset)
        .unwrap_or_else(|| panic!("site .v after /*A*/ in\n{}", pf.dump_text()));
    !matches!(s.verdict, Verdict::No(_))
}

fn held(src: &str) -> bool {
    let ast = Box::leak(Box::new(
        perry_parser::parse_typescript(src, "t.ts").expect("parses"),
    ));
    let mut set = perry_hir::PatchedBuiltins::new();
    perry_hir::scan_patched_builtins(ast, &mut set);
    perry_hir::set_patched_builtins(std::sync::Arc::new(set));
    let hir = perry_hir::lower_module(ast, "t", "t.ts");
    perry_hir::clear_patched_builtins();
    let hir = Box::leak(Box::new(hir.expect("lowers")));
    let mut g = crate::gen::Gen::new(vec![("t.ts", &*hir)], false, false);
    g.m.max_work = 50_000_000;
    g.register_all();
    g.link_exports();
    g.walk_all();
    g.feed_parent_values();
    g.m.summarize_and_replay();
    g.m.solve();
    g.m.realm_held
}

fn plain(store: &str) -> String {
    format!(
        "class N {{ v = 1; }}
        const n = new N();
        {store}
        console.log(/*A*/ n.v);"
    )
}

const SELF: [&str; 4] = [
    "globalThis.globalThis = globalThis;",
    "(globalThis as any).window = globalThis;",
    "(globalThis as any).self = globalThis;",
    "const g = globalThis as Record<string, unknown>; g[\"globalThis\"] = g; g[\"window\"] = g;",
];

/// RED: the global object stored onto its own named key hands nothing to
/// unknown code.
#[test]
fn e20g2_self_store_keeps_the_realm() {
    let mut bad = Vec::new();
    for s in SELF {
        if held(&plain(s)) {
            bad.push(format!("{s}: realm held"));
        }
        if !v_proven(&plain(s)) {
            bad.push(format!("{s}: n.v not proven"));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// KEEP: the global object handed to an unknown function escapes.
#[test]
fn e20g2_global_to_unknown_call_escapes() {
    let src = "class N { v = 1; }
        const n = new N();
        (globalThis as any).keep = n;
        const f: any = (globalThis as any)[process.argv[2]];
        f(globalThis);
        console.log(/*A*/ n.v);";
    assert!(!v_proven(src), "n.v proven although globalThis escaped");
}

/// Soundness: bare `window` / `self` read the program's store.
#[test]
fn e20g2_bare_window_reads_the_store() {
    let mut bad = Vec::new();
    for name in ["window", "self"] {
        let src = format!(
            "class N {{ v = 1; }}
            (globalThis as any).{name} = new N();
            const w: any = {name};
            console.log(/*A*/ w.v);"
        );
        if !v_proven(&src) {
            bad.push(format!("{name}: w.v not proven"));
        }
        // Either stored class: the read sees the store, not the global object.
        let src2 = format!(
            "class N {{ v = 1; }}
            class M {{ q = 2; v = 3; }}
            (globalThis as any).{name} = process.argv.length > 9 ? new N() : new M();
            const w: any = {name};
            console.log(/*A*/ w.v);"
        );
        let pf = facts(&src2);
        let at = src2.find("/*A*/").unwrap() as u32;
        let site = pf
            .sites
            .iter()
            .filter(|s| s.key == "v" && !s.is_call && s.byte_offset >= at)
            .min_by_key(|s| s.byte_offset)
            .unwrap();
        if !matches!(site.verdict, Verdict::T0Set | Verdict::T0Mono) {
            bad.push(format!("{name}: N | M read is {:?}", site.verdict));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// KEEP: any other value stored on the self-reference escapes (bare
/// `globalThis` reads are the global object, not the stored value).
#[test]
fn e20g2_other_value_on_self_key_escapes() {
    for s in [
        "(globalThis as any).globalThis = n;",
        "(globalThis as any).global = n;",
    ] {
        assert!(!v_proven(&plain(s)), "{s}: n.v proven although n escaped");
    }
}
