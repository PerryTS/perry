//! FETCHSTOP (c09 web3d audio.ts `fetch(this.base + file)`): a native that
//! hands a slot to unknown code (`fetch`'s `escapes(field(arg(1), body))`)
//! raises ⊤ in its call's throw region only once a value reaches that slot.
//! With no `init` the slot is empty forever: nothing is handed, nothing
//! thrown by unknown code. A program object as the body is handed: what
//! unknown code may throw reaches the region.
use crate::model::Model;
use crate::model::TOP;

fn solved(src: &str) -> Model {
    let ast = Box::leak(Box::new(perry_parser::parse_typescript(src, "t.ts").unwrap()));
    let hir = Box::leak(Box::new(perry_hir::lower_module(ast, "t", "t.ts").unwrap()));
    let mut g = crate::gen::Gen::new(vec![("t.ts", &*hir)], false, false);
    g.m.max_work = 300_000;
    g.register_all();
    g.link_exports();
    g.walk_all();
    g.feed_parent_values();
    g.m.summarize_and_replay();
    g.m.solve();
    g.m
}

/// Whether the throw region of some call holds ⊤ (nothing else in the
/// fixture reaches unknown code).
fn fetch_region_top(m: &Model) -> bool {
    let regions: Vec<u32> = m
        .calls
        .iter()
        .map(|c| c.region)
        .filter(|&r| r != u32::MAX)
        .collect();
    assert!(!regions.is_empty());
    regions.iter().any(|&r| m.nodes[r as usize].pts.contains_tok(TOP))
}

#[test]
fn fetch_without_init_hands_nothing() {
    let m = solved(
        "function load(file: string): Promise<unknown> {
           try { return fetch(\"http://x/\" + file); } catch (e) { return Promise.resolve(e); }
         }
         load(\"a.mp3\");",
    );
    assert!(!fetch_region_top(&m), "an empty body slot raised ⊤");
}

#[test]
fn fetch_with_program_body_is_handed() {
    let m = solved(
        "class B { a = 1; }
         function load(file: string): Promise<unknown> {
           try { return fetch(\"http://x/\" + file, { method: \"POST\", body: new B() as any }); } catch (e) { return Promise.resolve(e); }
         }
         load(\"a.mp3\");",
    );
    assert!(fetch_region_top(&m), "a handed program body must raise ⊤");
}
