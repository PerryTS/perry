//! R-GLOBAL-NAME (E20G): a program store onto the global object under a
//! named key escapes its value only when the runtime itself reads that
//! global by name (`undeclared::global_store_escapes`): a declared
//! constructor's global (instanceof, class metadata and the realm's links
//! read those through `js_get_global_this_builtin_value`), and the names
//! `members::global_runtime_reads` declares (`Temporal`, a worker global's
//! message handlers). Any other named global (`globalThis`, `window`,
//! `web3d`) is an own property of the global object that only program code
//! reads, through its field: no E20 sink and no realm hold. A computed key
//! may name any global and keeps E20; so do the other built-in holders.
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

/// Is the first `.v` read site after `/*A*/` proven?
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

fn program(store: &str) -> String {
    format!(
        "class N {{ v = 1; }}
        const n = new N();
        const k: any = process.argv[2];
        {store}
        console.log(/*A*/ n.v);"
    )
}

fn plain(store: &str) -> String {
    format!(
        "class N {{ v = 1; }}
        const n = new N();
        {store}
        console.log(/*A*/ n.v);"
    )
}

/// RED: named stores of a global the runtime never reads by name.
#[test]
fn e20g_named_global_store_is_the_programs() {
    let mut bad = Vec::new();
    for store in [
        "(globalThis as any).web3d = n;",
        "(globalThis as any).__THREE__ = n;",
        "const g = globalThis as Record<string, unknown>; g[\"window\"] = n;",
        "const g = globalThis as any; g[\"performance\"] = { now: () => 0, o: n };",
        "(globalThis as any).performance = { now: () => 0, o: n };",
        "(globalThis as any).Reflect = { o: n };",
    ] {
        if !v_proven(&plain(store)) {
            bad.push(format!("{store}: n.v not proven"));
        }
        if held(&plain(store)) {
            bad.push(format!("{store}: realm held"));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// RED: a program read of the stored global sees the stored value.
#[test]
fn e20g_named_global_reads_back_the_store() {
    let src = "class N { v = 1; }
        (globalThis as any).web3d = new N();
        const w: any = (globalThis as any).web3d;
        console.log(/*A*/ w.v);";
    assert!(v_proven(src), "w.v not proven");
}

/// KEEP: globals the runtime reads by name (a declared constructor's, a
/// name the global member declares), other holders. Computed-key global
/// stores keep their E20 path unchanged (`Key::Any`); GK decides them first.
#[test]
fn e20g_runtime_read_globals_still_sink() {
    let mut bad = Vec::new();
    for store in [
        "(globalThis as any).Temporal = n;",
        "(globalThis as any).Array = n;",
        "(globalThis as any).Promise = n;",
        "(globalThis as any).WebAssembly = n;",
        "(globalThis as any).console = n;",
        "(globalThis as any).process = n;",
        "(Promise.prototype as any).then = n;",
    ] {
        if v_proven(&program(store)) {
            bad.push(format!("{store}: n.v proven, the runtime may read n"));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// Is the first store site of `key` after `/*S*/` proven?
fn store_proven(src: &str, key: &str) -> bool {
    let pf = facts(src);
    let at = src.find("/*S*/").expect("marker") as u32;
    let s = pf
        .sites
        .iter()
        .filter(|s| s.key == key && !s.is_call && s.access.is_store() && s.byte_offset >= at)
        .min_by_key(|s| s.byte_offset)
        .unwrap_or_else(|| panic!("store .{key} after /*S*/ in\n{}", pf.dump_text()));
    !matches!(s.verdict, Verdict::No(_))
}

/// RED (owner decision 2026-11-19): a program store onto a declared
/// writable data global the runtime cannot call is a proven store site;
/// callable globals, `console` and `WebAssembly` stay unproven.
#[test]
fn e20g_declared_data_global_store_site() {
    let mut bad = Vec::new();
    for (key, store, proven) in [
        (
            "performance",
            "const g = globalThis as any; /*S*/ g.performance = { now: () => 0 };",
            true,
        ),
        (
            "performance",
            "const g = globalThis as any; /*S*/ g[\"performance\"] = { now: () => 0 };",
            true,
        ),
        (
            "Array",
            "const g = globalThis as any; /*S*/ g.Array = { o: 1 };",
            false,
        ),
        (
            "console",
            "const g = globalThis as any; /*S*/ g.console = { o: 1 };",
            false,
        ),
        (
            "WebAssembly",
            "const g = globalThis as any; /*S*/ g.WebAssembly = { o: 1 };",
            false,
        ),
    ] {
        if store_proven(&plain(store), key) != proven {
            bad.push(format!("{store}: store site proven != {proven}"));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}
