//! PN66 RXSTAT (LADDER c17-A6): `console.log(/a/)`. The console's inspect
//! reads the RegExp's members under any key, among them `constructor` =
//! RegExp, and reads RegExp under any key too. Perry's RegExp owns exactly
//! its realm declaration (`length`, `name`, `prototype`, `escape`,
//! `@@species`; probe pn66/probe/rx.ts), then `Function.prototype`'s
//! members: no unknown static, so nothing unknown is called.
use super::*;

fn held(src: &str) -> bool {
    let ast = Box::leak(Box::new(perry_parser::parse_typescript(src, "t.ts").unwrap()));
    let hir = Box::leak(Box::new(perry_hir::lower_module(ast, "t", "t.ts").unwrap()));
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

/// `console.log(/a/)` (pair pn62_m6c).
#[test]
fn rxstat_console_log_of_a_regexp_holds_nothing() {
    assert!(!held("console.log(/a/);\n"));
}

/// A computed read on RegExp: its declared statics or `Function.prototype`'s.
#[test]
fn rxstat_any_key_read_on_regexp_holds_nothing() {
    assert!(!held("declare const k: string;\nconst v = (RegExp as any)[k];\nv();\n"));
}
