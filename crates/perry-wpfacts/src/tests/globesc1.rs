//! GLOBESC1: what the global object's self-reference store
//! (`g["globalThis"] = globalThis`, web/src/browser.ts:229) hands to
//! unknown code. Each case runs with and without the self store; the
//! test asserts that none escapes (R-GLOBAL-SELF).
use super::*;

fn escaped(src: &str) -> (Vec<String>, bool) {
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
    let mut out = Vec::new();
    for t in 0..g.m.toks.len() as crate::model::Tok {
        if g.m.is_escaped(t) {
            let w = g
                .m
                .writer_escape_why(t)
                .map(|w| format!(" writer[{w}]"))
                .unwrap_or_default();
            out.push(format!("{}{w}", g.m.tok_name(t)));
        }
    }
    (out, g.m.realm_held)
}

const HEAD: &str = "const g = globalThis as Record<string, unknown>;
class Doc { v = 1; title(): string { return \"t\"; } }
const document = new Doc();
function helper(x: number): number { return x + 1; }
";

const CASES: [(&str, &str); 11] = [
    (
        "entries36",
        "const classes: Record<string, unknown> = { K0: Doc, K1: Doc, K2: Doc, K3: Doc, K4: Doc, K5: Doc, K6: Doc, K7: Doc, K8: Doc, K9: Doc, K10: Doc, K11: Doc, K12: Doc, K13: Doc, K14: Doc, K15: Doc, K16: Doc, K17: Doc, K18: Doc, K19: Doc, K20: Doc, K21: Doc, K22: Doc, K23: Doc, K24: Doc, K25: Doc, K26: Doc, K27: Doc, K28: Doc, K29: Doc, K30: Doc, K31: Doc, K32: Doc, K33: Doc, K34: Doc, K35: Doc }; for (const [k, v] of Object.entries(classes)) g[k] = v;",
    ),
    (
        "entriesCall",
        "function mk(f: () => unknown): unknown { return class X { q = f(); }; } const classes: Record<string, unknown> = { Doc, helper, Audio: mk(() => document) }; for (const [k, v] of Object.entries(classes)) g[k] = v;",
    ),
    (
        "entriesKnown",
        "const classes: Record<string, unknown> = { Doc, helper }; for (const [k, v] of Object.entries(classes)) g[k] = v;",
    ),
    (
        "entriesUnknown",
        "const classes: Record<string, unknown> = { Doc, helper }; classes[String(Date.now())] = 1; for (const [k, v] of Object.entries(classes)) g[k] = v;",
    ),
    (
        "unknownKey",
        "g[String(Date.now())] = helper;",
    ),
    ("named", "g[\"document\"] = document; g[\"helper\"] = helper; g[\"window\"] = globalThis;"),
    (
        "define",
        "function define(name: string, get: () => unknown): void { Object.defineProperty(g, name, { get, configurable: true }); }
        define(\"innerWidth\", () => document);",
    ),
    (
        "computed",
        "const names = [\"a\", \"b\"]; for (const k of names) { g[k] = helper; }",
    ),
    (
        "defineProp",
        "Object.defineProperty(g, \"navigator\", { value: { lang: document }, configurable: true, writable: true });",
    ),
    (
        "barewin",
        "g[\"window\"] = globalThis; const w = (window as any); w.helper2 = helper;",
    ),
    (
        "all",
        "g[\"document\"] = document; g[\"helper\"] = helper; g[\"window\"] = globalThis;
        function define(name: string, get: () => unknown): void { Object.defineProperty(g, name, { get, configurable: true }); }
        define(\"innerWidth\", () => document);
        const names = [\"a\", \"b\"]; for (const k of names) { g[k] = helper; }",
    ),
];

/// RED (R-GLOBAL-SELF): the self store hands nothing to unknown code in
/// any case (was 26 / 5 escapes in `entriesUnknown` / `unknownKey`), and
/// nothing escapes without it either.
#[test]
fn globesc1_self_store_escapes_nothing() {
    let mut bad = Vec::new();
    for (name, body) in CASES {
        let tail = "console.log(document.v);";
        let without = format!("{HEAD}{body}\n{tail}");
        let with = format!("{HEAD}{body}\ng[\"globalThis\"] = globalThis;\n{tail}");
        for (w, src) in [("without", without), ("with", with)] {
            let (esc, held) = escaped(&src);
            if !esc.is_empty() || held {
                bad.push(format!("{name} {w}: held {held}, escaped {esc:?}"));
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}
