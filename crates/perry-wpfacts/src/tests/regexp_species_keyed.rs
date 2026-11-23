//! SpeciesConstructor of `@@matchAll` and `@@split` (22.2.6.9, 7.3.22) on a
//! regexp whose `constructor` is the realm's `%RegExp%` (PERFNOW19 r19_1:
//! `s.matchAll(/u/g)`): `C[@@species]` is a keyed read that gives
//! `%RegExp%` itself, which builds the realm's own regexp. When the model
//! touched `C` with any protocol, every member of `Function.prototype` was
//! read and called as an unknown callee, and the realm was held.
use super::*;

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

const BASE: &str = "const s: string = String(Date.now()); console.log(s.length);";

#[test]
fn base_does_not_hold_the_realm() {
    assert!(!held(BASE));
}

#[test]
fn match_all_with_the_realm_regexp_does_not_hold_the_realm() {
    assert!(!held(&format!("{BASE} s.matchAll(/u/g);")));
}

#[test]
fn split_with_the_realm_regexp_does_not_hold_the_realm() {
    assert!(!held(&format!("{BASE} s.split(/u/);")));
}
