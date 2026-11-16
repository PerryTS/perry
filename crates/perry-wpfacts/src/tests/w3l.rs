//! W3HOST5 (2026-11-16), S8: a bare call / read of a name the main realm
//! declares neither on the global object nor on its chain
//! (`addEventListener`) is [[Get]](globalThis, name) with `this` undefined
//! (ECMA-262 9.1.1.4.6 GetBindingValue, 13.3.6.1 EvaluateCall): the values
//! the program stored there; absent, the runtime's ReferenceError. The
//! Worker global's native of that name (perry-stdlib worker_threads
//! worker_surface.rs) may be the value only in a module a Worker may run
//! (`Model::worker_side`): reachable from a named Worker entry, or every
//! module when some Worker's entry is not named.
use super::*;

/// Solve `mods` (name, source) with `roots`; Worker filenames get `paths`
/// as the driver resolves them (a literal names its module). Returns the
/// unknown values' reasons, whether the realm is held, per-module worker
/// side, and the pts sizes of function `f`'s parameters.
fn solve(mods: &[(&str, &str)], roots: &[&str], worker_paths: &[&str]) -> (Vec<String>, bool, Vec<bool>, Vec<usize>) {
    let mut hirs = Vec::new();
    for (name, src) in mods {
        let path = format!("{name}.ts");
        let ast = Box::leak(Box::new(perry_parser::parse_typescript(src, &path).unwrap()));
        let mut hir = perry_hir::lower_module(ast, name, &path).unwrap();
        for imp in &mut hir.imports {
            if let Some(n) = imp.source.strip_prefix("./") {
                imp.resolved_path = Some(n.to_string());
            }
        }
        perry_hir::for_each_worker_new_mut(&mut hir, &mut |e| {
            if let perry_hir::Expr::WorkerNew { paths, .. } = e {
                *paths = worker_paths.iter().map(|p| p.to_string()).collect();
            }
        });
        // The driver resolves each Worker specifier through an import of
        // the creating module (`module_roots`): mirror it.
        let mut has_worker = false;
        perry_hir::for_each_worker_new(&hir, &mut |_| has_worker = true);
        if let Some(base) = hir.imports.first().cloned().filter(|_| has_worker) {
            for p in worker_paths {
                let mut i = base.clone();
                i.source = p.to_string();
                i.specifiers.clear();
                i.is_native = false;
                i.type_only = true;
                i.resolved_path = p.strip_prefix("./").map(|s| format!("{s}"));
                hir.imports.push(i);
            }
        }
        hirs.push((Box::leak(path.into_boxed_str()) as &str, &*Box::leak(Box::new(hir))));
    }
    let mut g = crate::gen::Gen::new(hirs, false, true);
    g.roots = roots.iter().map(|r| r.to_string()).collect();
    g.m.max_work = 50_000_000;
    g.register_all();
    g.link_exports();
    g.walk_all();
    g.feed_parent_values();
    g.m.summarize_and_replay();
    g.m.solve();
    let f = g.m.funcs.iter().find(|f| f.name == "f" || f.name.ends_with(":f") || f.name.ends_with("::f"));
    if f.is_none() {
        eprintln!("funcs: {:?}", g.m.funcs.iter().map(|f| f.name.clone()).collect::<Vec<_>>());
    }
    let ps = f.map_or(Vec::new(), |f| f.params.iter().map(|&p| g.m.nodes[p as usize].pts.len()).collect());
    let mut tops: Vec<String> = g.m.top_vals.keys().cloned().collect();
    tops.extend(g.m.escaped.values().map(|&r| g.m.reasons[r as usize].clone()));
    (tops, g.m.realm_held, g.m.worker_side.clone(), ps)
}

const STORED: &str = "const g = globalThis as any; const ls: any[] = [];\n\
    function f(t: string, cb: any): void { ls.push(cb); }\n\
    g[\"addEventListener\"] = f;\n\
    addEventListener(\"resize\", () => { console.log(\"resize\"); });\n\
    for (const h of ls) h();\n\
    console.log(ls.length, process.argv[2]);\n";

fn unknown_ael(tops: &[String]) -> bool {
    tops.iter().any(|t| t.contains("addEventListener"))
}

// R1 (RED on base): the bare call after `g["addEventListener"] = f` calls f.
#[test]
fn w3l_r1_bare_call_reads_the_stored_global() {
    let (tops, held, side, ps) = solve(&[("main", STORED)], &[], &[]);
    assert_eq!(side, vec![false]);
    assert!(!unknown_ael(&tops), "no unknown addEventListener: {tops:?}");
    assert!(!held, "the realm is not held: {tops:?}");
    assert_eq!(ps.len(), 2);
    assert!(ps[1] > 0, "f is called with the listener: {ps:?}");
}

// K1: a named Worker entry: its module's bare call may be the Worker native
// (unknown); the main module's call stays [[Get]].
#[test]
fn w3l_k1_named_worker_entry() {
    let main = format!("import {{ Worker }} from \"node:worker_threads\";\nconst w = new Worker(\"./w.ts\");\n{STORED}");
    let w = "addEventListener(\"message\", (e: any) => { console.log(e); });\n";
    let (tops, _, side, ps) = solve(&[("main", &main), ("w", w)], &["w.ts"], &["./w.ts"]);
    assert_eq!(side, vec![false, true], "only the entry is worker side");
    assert!(unknown_ael(&tops), "the worker's call is unknown: {tops:?}");
    assert!(ps[1] > 0, "main's call is f: {ps:?}");
}

// K2: a Worker whose filename comes from argv names no entry: every module
// may run in a Worker; the call keeps the unknown path.
#[test]
fn w3l_k2_unknown_worker_entry() {
    let main = format!("import {{ Worker }} from \"node:worker_threads\";\nconst w = new Worker(process.argv[2]);\n{STORED}");
    let (tops, _, side, _) = solve(&[("main", &main)], &[], &[]);
    assert_eq!(side, vec![true]);
    assert!(unknown_ael(&tops), "unknown path kept: {tops:?}");
}

// K3: stored under an unknown key: the value set includes what was stored.
#[test]
fn w3l_k3_unknown_key_store() {
    let src = "const g = globalThis as any; const ls: any[] = [];\n\
        function f(t: string, cb: any): void { ls.push(cb); }\n\
        g[String(process.argv[2])] = f;\n\
        addEventListener(\"resize\", () => { console.log(\"resize\"); });\n\
        console.log(ls.length);\n";
    let (_, _, _, ps) = solve(&[("main", src)], &[], &[]);
    assert!(ps[1] > 0, "f may be the callee: {ps:?}");
}

// K4: `delete globalThis.addEventListener`, then the call: the callee is
// the [[Get]] value (no Worker native, no sentinel replacement). The
// delete itself is recorded as a write the model does not follow
// (`global_value_written`), so that value is unknown (⊤): SOUND BUT
// UNKNOWN, not yet the exact ReferenceError arm (a modeled delete of a
// program-owned global would make it exact; not built).
#[test]
fn w3l_k4_deleted_then_called() {
    let src = "delete (globalThis as any).addEventListener;\n\
        try { addEventListener(\"resize\", () => {}); } catch (e) { console.log(\"RE\"); }\n";
    let (tops, held, _, _) = solve(&[("main", src)], &[], &[]);
    assert!(!unknown_ael(&tops), "no Worker native, no replaced sentinel: {tops:?}");
    let _ = held;
}

// K5: a bare read of the name is the stored value too.
#[test]
fn w3l_k5_bare_read() {
    let src = "const g = globalThis as any; const ls: any[] = [];\n\
        function f(t: string, cb: any): void { ls.push(cb); }\n\
        g[\"addEventListener\"] = f;\n\
        const a = addEventListener; a(\"x\", () => {});\n\
        console.log(ls.length);\n";
    let (tops, _, _, ps) = solve(&[("main", src)], &[], &[]);
    assert!(!unknown_ael(&tops), "{tops:?}");
    assert!(ps[1] > 0, "{ps:?}");
}

// R2 (RED on base): the same program through the compile driver entry
// (`crate::compute`): the bare call site is proven.
#[test]
fn w3l_r2_compute_path_proves_the_bare_call() {
    let path = "main.ts".to_string();
    let module = perry_parser::parse_typescript(STORED, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "main", &path).expect("lowers");
    let list: Vec<(String, &perry_hir::Module)> = vec![(path, &hir)];
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    let pf = crate::compute(&list, &cfg);
    let at = STORED.find("addEventListener(").unwrap() as u32;
    let v: Vec<_> = pf.sites.iter().filter(|s| s.byte_offset >= at && s.byte_offset < at + 20).map(|s| (s.key.clone(), s.verdict.clone())).collect();
    assert!(!v.is_empty() && v.iter().all(|(_, v)| !format!("{v:?}").starts_with("No(")), "{v:?}\n{}", pf.dump_text().lines().filter(|l| l.contains("addEventListener") || l.contains("unksrc")).take(20).collect::<Vec<_>>().join("\n"));
}

// K6: `console.log` lowers to the same sentinel (`GlobalGet(0).log`): the
// sentinel routes the Console methods (and `resolve` / `reject`) by name,
// so they are never [[Get]](globalThis, name) (`sentinel_user_global`).
#[test]
fn w3l_k6_console_names_keep_their_route() {
    let src = "const l = console.log; l(\"a\"); console.log(\"b\", process.argv[2]);\n";
    let path = "main.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "main", &path).expect("lowers");
    let list: Vec<(String, &perry_hir::Module)> = vec![(path, &hir)];
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    let pf = crate::compute(&list, &cfg);
    let d = pf.dump_text();
    assert!(!d.contains("absent global"), "{d}");
}
