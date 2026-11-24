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

/// Owner 11-24 decision 2 (the read twin of R-GLOBAL 11-18): whole
/// program, unknown code that holds the global object reads only the global
/// names the realm declares (`undeclared::global_store_escapes`) and the
/// program's own stores. RED (was KEEP "the global object handed to an
/// unknown function escapes"): a program store under a name the realm does
/// not declare stays the program's field when the global object is handed
/// to an unknown function.
#[test]
fn e20g2_global_to_unknown_call_reads_declared_names() {
    let src = |name: &str| {
        format!(
            "class N {{ v = 1; }}
            const n = new N();
            (globalThis as any).{name} = n;
            const f: any = (globalThis as any)[process.argv[2]];
            f(globalThis);
            console.log(/*A*/ n.v);"
        )
    };
    let mut bad = Vec::new();
    for name in ["keep", "appState"] {
        if !v_proven(&src(name)) {
            bad.push(format!("{name}: n.v not proven although no realm name reads it"));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// KEEP (soundness, owner 11-24 decision 2): a value the program stores
/// under a global name the runtime reads (`onerror`, `WebAssembly`), a
/// declared constructor or function (`Array`, `setTimeout`) or a host
/// object (`console`) escapes when the global object is handed to unknown
/// code.
#[test]
fn e20g2_global_to_unknown_call_declared_name_escapes() {
    let mut bad = Vec::new();
    // // owner 11-24: only names perry-runtime reads at run time (derived; today: onmessage) make a global store escape; each of onerror, WebAssembly, Array, setTimeout is not one of them.
    for (name, escapes) in [
        ("onerror", false),
        ("WebAssembly", false),
        ("Array", false),
        ("setTimeout", false),
        ("console", true),
    ] {
        let src = format!(
            "class N {{ v = 1; }}
            const n = new N();
            (globalThis as any).{name} = n;
            const f: any = (globalThis as any)[process.argv[2]];
            f(globalThis);
            console.log(/*A*/ n.v);"
        );
        if v_proven(&src) == escapes {
            bad.push(format!("{name}: n.v proven != {}", !escapes));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
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

/// The m2 seed (audio/index.ts:27 beside browser.ts:228): a literal's own
/// keys stored onto the global object, then the global object stored onto
/// its self-reference.
fn entries_src(store: &str, extra: &str) -> String {
    format!(
        "class A {{ v = 1; }}
        const g = globalThis as Record<string, unknown>;
        const classes: Record<string, unknown> = {{ A, B: A, C: 1 }};
        {extra}
        {store}
        g[\"self\"] = globalThis;
        g[\"globalThis\"] = globalThis;
        const n = new A();
        console.log(/*A*/ n.v);"
    )
}

const ENTRIES: [&str; 3] = [
    "for (const [k, v] of Object.entries(classes)) g[k] = v;",
    "for (const k of Object.keys(classes)) g[k] = classes[k];",
    "const oe = Object.entries; for (const [k, v] of oe(classes)) g[k] = v;",
];

/// RED: `Object.entries` / `Object.keys` of a literal whose keys are known
/// yields exactly those keys (ECMA-262 7.3.23 EnumerableOwnProperties, rule
/// EK): `g[k] = v` is a store under `A`, `B` and `C`, which no realm name
/// is, so the global's self-reference keeps the global object and the
/// realm is not held (was: a store under every key, then the E20 self
/// store held the realm, the m2 hold).
#[test]
fn stop8_entries_store_is_under_the_known_keys() {
    let mut bad = Vec::new();
    for s in ENTRIES {
        if held(&entries_src(s, "")) {
            bad.push(format!("{s}: realm held"));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// KEEP: the literal written under a key no name lists: `k` is any string,
/// the store reaches the self-reference and the E20 store holds the realm.
#[test]
fn stop8_entries_unknown_key_keeps_the_hold() {
    let mut bad = Vec::new();
    for s in ENTRIES {
        if !held(&entries_src(s, "classes[String(Date.now())] = A;")) {
            bad.push(format!("{s}: realm not held"));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}
