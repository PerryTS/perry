use super::*;
use crate::SiteFacts;

fn call_at<'a>(pf: &'a ProgramFacts, source: &str) -> Vec<&'a SiteFacts> {
    let offset = source.find("cb(value)").unwrap() as u32 + 1;
    let sites: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.is_call && s.byte_offset == offset)
        .collect();
    assert!(!sites.is_empty(), "{}", pf.dump_text());
    sites
}

fn catches_up(source: &str) {
    let whole = facts(source);
    let unit = units(source);
    assert!(call_at(&whole, source)
        .iter()
        .all(|s| matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set)));
    assert!(
        call_at(&unit, source)
            .iter()
            .all(|s| matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set)),
        "{}",
        unit.dump_text()
    );
    assert!(call_at(&unit, source).iter().all(|s| s.targets.len() == 1));
    // Naming the callback does not close its entry: per unit its input is
    // proven only where the whole program proves it (DECISIONS 224: what
    // the closed callee passes stands beside the markers, nothing more).
    not_stronger(&whole, &unit, "mustRemainUnknown");
}

fn is_proven(s: &SiteFacts) -> bool {
    !matches!(s.verdict, Verdict::No(_))
}

fn sites_of<'a>(pf: &'a ProgramFacts, key: &str) -> Vec<&'a SiteFacts> {
    let v: Vec<_> = pf.sites.iter().filter(|s| s.key == key).collect();
    assert!(!v.is_empty(), "{key}\n{}", pf.dump_text());
    v
}

/// Every site of `key` proven per unit is proven whole (per unit is never
/// stronger than the whole program).
fn not_stronger(whole: &ProgramFacts, unit: &ProgramFacts, key: &str) {
    for u in sites_of(unit, key).into_iter().filter(|s| is_proven(s)) {
        let w: Vec<_> = sites_of(whole, key)
            .into_iter()
            .filter(|w| w.byte_offset == u.byte_offset)
            .collect();
        assert!(
            !w.is_empty() && w.iter().all(|w| is_proven(w)),
            "per unit stronger at {key}@{}\n{}",
            u.byte_offset,
            unit.dump_text()
        );
    }
}

fn refuses_call(source: &str) {
    let unit = units(source);
    assert!(
        call_at(&unit, source)
            .iter()
            .all(|s| matches!(s.verdict, Verdict::No(_))),
        "{}",
        unit.dump_text()
    );
}

#[test]
fn pub_b_direct_identity_keeps_unknown_entry() {
    catches_up(
        r#"function apply(cb: any, value: any) { return cb(value); }
function root() { return apply((x: any) => x.mustRemainUnknown, {mustRemainUnknown: 1}); } console.log(root());"#,
    );
    refuses_call(
        r#"function apply(cb: any, value: any) { return cb(value); }
function root() { return apply((globalThis as any).unknown, {mustRemainUnknown: 1}); } console.log(root());"#,
    );
    let captured = r#"function apply(cb: any, value: any) { return cb(value); }
function root() { const captured = (globalThis as any).capture; return apply((x: any) => x.mustRemainUnknown + captured.captureMustRemainUnknown, {mustRemainUnknown: 1}); } console.log(root());"#;
    catches_up(captured);
    let unit = units(captured);
    let reads: Vec<_> = unit
        .sites
        .iter()
        .filter(|s| s.key == "captureMustRemainUnknown")
        .collect();
    assert!(!reads.is_empty());
    assert!(
        reads.iter().all(|s| matches!(s.verdict, Verdict::No(_))),
        "{}",
        unit.dump_text()
    );
}

#[test]
fn pub_b_forwarder_identity_keeps_unknown_entry() {
    catches_up(
        r#"function apply(cb: any, value: any) { return cb(value); }
function forward(cb: any, value: any) { return apply(cb, value); } function root() { return forward((x: any) => x.mustRemainUnknown, {mustRemainUnknown: 1}); } console.log(root());"#,
    );
    refuses_call(
        r#"function apply(cb: any, value: any) { return cb(value); }
function forward(cb: any, value: any) { return apply(cb, value); } function root() { return forward((globalThis as any).unknown, {mustRemainUnknown: 1}); } console.log(root());"#,
    );
    refuses_call(
        r#"function apply(cb: any, value: any) { return cb(value); }
function forward(cb: any, value: any) { return apply(cb, value); } function root() { forward(); return forward((x: any) => x.mustRemainUnknown, {mustRemainUnknown: 1}); } console.log(root());"#,
    );
    refuses_call(
        r#"function apply(cb: any, value: any) { return cb(value); }
function forward(cb: any, value: any) { return apply(cb, value); } function root() { return forward((x: any) => x.mustRemainUnknown, {mustRemainUnknown: 1}); } console.log(root()); (globalThis as any).retained = forward;"#,
    );
}

#[test]
fn pub_b_escape_identity_keeps_unknown_entry() {
    catches_up(
        r#"function apply(cb: any, value: any) { return cb(value); }
function root() { const f = (x: any) => x.mustRemainUnknown; const alias = f; const result = apply(alias, {mustRemainUnknown: 1}); return result; } console.log(root());"#,
    );
    refuses_call(
        r#"function apply(cb: any, value: any) { return cb(value); }
function root() { const f = (x: any) => x.mustRemainUnknown; const alias = f; const result = apply(alias, {mustRemainUnknown: 1}); (globalThis as any).retained = f; return result; } console.log(root());"#,
    );
    let returned = r#"function apply(cb: any, value: any) { return cb(value); }
function root() { const f = (x: any) => x.mustRemainUnknown; const alias = f; const result = apply(alias, {mustRemainUnknown: 1}); return f; } (globalThis as any).retained = root();"#;
    let unit = units(returned);
    let entries: Vec<_> = unit
        .sites
        .iter()
        .filter(|s| s.key == "mustRemainUnknown")
        .collect();
    assert!(!entries.is_empty());
    assert!(
        entries.iter().all(|s| matches!(s.verdict, Verdict::No(_))),
        "{}",
        unit.dump_text()
    );
}

#[test]
fn pub_b_union_identity_keeps_unknown_entry() {
    catches_up(
        r#"function apply(cb: any, value: any) { return cb(value); }
function root() { const f = (x: any) => x.mustRemainUnknown; const selected = f; return apply(selected, {mustRemainUnknown: 1}); } console.log(root());"#,
    );
    refuses_call(
        r#"function apply(cb: any, value: any) { return cb(value); }
function root() { const f = (x: any) => x.mustRemainUnknown; const selected = (globalThis as any).pick ? f : (globalThis as any).unknown; return apply(selected, {mustRemainUnknown: 1}); } console.log(root());"#,
    );
}

#[test]
fn pub_described_results_normalize_escaped_boxes_beside_top() {
    use crate::gen::Gen;
    for escaped in [false, true] {
        for unknown in [false, true] {
            for box_first in [false, true] {
                let ast = perry_parser::parse_typescript(
                    r#"function F() { return "a,b".split(","); }"#,
                    "owner.ts",
                )
                .unwrap();
                let hir = perry_hir::lower_module(&ast, "owner", "owner.ts").unwrap();
                let mut g = Gen::new(vec![("owner.ts", &hir)], false, false);
                g.register_all();
                g.link_exports();
                g.walk_all();
                let f =
                    g.m.funcs
                        .iter()
                        .position(|f| f.name.ends_with(":F"))
                        .unwrap() as u32;
                let boxed = (0..g.m.toks.len() as u32)
                    .find(|&t| g.m.is_box(t))
                    .expect("split box");
                let n = g.m.node(|| "mixed result".into());
                if box_first {
                    g.m.add_tok(n, boxed);
                }
                if escaped {
                    let r = g.m.reason("unknown holder of the box");
                    g.m.escape(boxed, r);
                }
                if unknown {
                    g.m.add_top(n, |_| "unknown result alternative".into());
                }
                if !box_first {
                    g.m.add_tok(n, boxed);
                }
                let d = crate::contracts::describe_nodes(
                    &g,
                    &[],
                    f,
                    vec![n],
                    false,
                    &mut Vec::new(),
                    &Default::default(),
                );
                assert_eq!(d.unknown, unknown);
                // DEC189 is independent of arrival order. An escaped box is
                // subsumed only beside TOP; private boxes and known-only
                // escaped boxes must still be described (refusal controls).
                assert_eq!(
                    d.value.boxes.len(),
                    usize::from(!(escaped && unknown)),
                    "escaped={escaped} unknown={unknown} box_first={box_first}: {d:?}"
                );
            }
        }
    }
}

/// The diagnostics helpers of tssrc (declarations/diagnostics.ts): `make`
/// returns an inner helper or an unknown value. DEC189 subsumes only what
/// unknown code holds: the helper, published through its contract, stays
/// described beside that unknown, so the caller holds its stand-in and
/// handing its literal to unknown code is a census report. Subsumed, the
/// caller held only ⊤ and the owner claimed the literal private (tssrc
/// diagnostics.ts layouts). Per unit the literal escapes as it does whole.
fn helper_literals(mods: &[(&str, &str)]) -> Vec<(&'static str, Option<String>)> {
    let mut out = Vec::new();
    for (mode, pf) in [("whole", whole_of(mods)), ("unit", units_of(mods))] {
        let lits: Vec<_> = pf
            .templates
            .iter()
            .filter(|t| t.fields.iter().any(|f| f.key == "diagMessage"))
            .collect();
        assert!(!lits.is_empty(), "{mode}: live literal\n{}", pf.dump_text());
        for t in lits {
            out.push((mode, t.escape.clone()));
        }
    }
    out
}

const PUB_HELPER_OWNER: &str = r#"export function make(node: any): any {
  if (node.k === 1) return helper;
  return (globalThis as any).fallback;
  function helper(r: any) { const info = { diagMessage: r.m, errorNode: node }; console.log(info.diagMessage); return info; }
}"#;

const PUB_HELPER_OWNER_DESCRIBED: &str = r#"export function make(node: any): any {
  return helper;
  function helper(r: any) { const info = { diagMessage: r.m, errorNode: node }; console.log(info.diagMessage); return info; }
}"#;

const PUB_HELPER_SINK: &str = r#"import { make } from './a';
export function use(node: any, r: any) { const info = make(node)(r); (globalThis as any).sink(info); return 1; }
use({ k: 1 }, { m: "x" });"#;

const PUB_HELPER_PRIVATE: &str = r#"import { make } from './a';
export function use(node: any, r: any) { const info = make(node)(r); console.log(info.diagMessage); return 1; }
use({ k: 1 }, { m: "x" });"#;

#[test]
fn pub_described_result_beside_top_keeps_its_contract_function() {
    let rows = helper_literals(&[("a", PUB_HELPER_OWNER), ("b", PUB_HELPER_SINK)]);
    for (mode, esc) in &rows {
        assert!(
            esc.is_some(),
            "{mode}: unknown code receives the helper's literal: {rows:?}"
        );
    }
}

#[test]
fn pub_described_result_without_top_keeps_its_contract() {
    // Control: no unknown alternative. The helper is described by its
    // contract and the reader's `sink` is its census report: the literal
    // escapes per unit as it does whole.
    let rows = helper_literals(&[("a", PUB_HELPER_OWNER_DESCRIBED), ("b", PUB_HELPER_SINK)]);
    for (mode, esc) in &rows {
        assert!(
            esc.is_some(),
            "{mode}: the helper's literal reaches sink: {rows:?}"
        );
    }
    // Refusal control: no unknown alternative and no unknown holder. The
    // described helper keeps its contract per unit exactly as before.
    let rows = helper_literals(&[("a", PUB_HELPER_OWNER_DESCRIBED), ("b", PUB_HELPER_PRIVATE)]);
    let private = |m: &str| rows.iter().any(|(mode, e)| *mode == m && e.is_none());
    assert!(private("whole"), "{rows:?}");
    assert!(private("unit"), "{rows:?}");
}

/// DECISIONS 224 witness (tssrc sys.ts `createFileWatcherCallback`,
/// session.ts): the closed callee runs the owned callback through its
/// contract (DECISIONS 223) and publishes what it passes; the callback's
/// parameter holds that beside its marker, as the whole program sees it.
/// Red on main (no callable identity: the call through `cb`) and on the
/// DEC223 head (bare parameter marker: the read through `x`).
#[test]
fn pub_callable_parameters_hold_what_their_runners_pass() {
    let src = r#"function apply(cb: any, value: any) { return cb(value); }
function root() { return apply((x: any) => x.described, {described: 1}); } console.log(root());"#;
    let whole = facts(src);
    let unit = units(src);
    for (mode, pf) in [("whole", &whole), ("unit", &unit)] {
        assert!(
            call_at(pf, src).iter().all(|s| is_proven(s)),
            "{mode}\n{}",
            pf.dump_text()
        );
        assert!(
            sites_of(pf, "described").iter().all(|s| is_proven(s)),
            "{mode}: the callee's argument stands beside the marker\n{}",
            pf.dump_text()
        );
    }
    not_stronger(&whole, &unit, "described");
}

/// DECISIONS 224 refusal arms: the join is only what known runners pass.
/// A callback also held by unknown code is called with anything (rule R:
/// its parameters stay ⊤), and a position some runner passes undescribed
/// stays the bare marker.
#[test]
fn pub_callable_parameters_stay_unknown_beyond_their_runners() {
    let escaped = r#"function apply(cb: any, value: any) { return cb(value); }
function root() { const f = (x: any) => x.described; (globalThis as any).held = f; return apply(f, {described: 1}); } console.log(root());"#;
    let undescribed = r#"function apply(cb: any, value: any) { return cb((globalThis as any).unknown); }
function root() { return apply((x: any) => x.described, {described: 1}); } console.log(root());"#;
    let union = r#"function apply(cb: any, value: any) { return cb((globalThis as any).pick ? value : (globalThis as any).unknown); }
function root() { return apply((x: any) => x.described, {described: 1}); } console.log(root());"#;
    for src in [escaped, undescribed, union] {
        let whole = facts(src);
        let unit = units(src);
        for (mode, pf) in [("whole", &whole), ("unit", &unit)] {
            assert!(
                sites_of(pf, "described").iter().all(|s| !is_proven(s)),
                "{mode}: unknown argument\n{src}\n{}",
                pf.dump_text()
            );
        }
    }
}

/// DECISIONS 224 witness, the tssrc sys.ts shape (`createSingleWatcherPerName`
/// runs `createWatcher` with a closure of its own): the closed callee runs
/// the published `create` with its own callback, which is published to the
/// runner's argument join in turn, so `inner(..)` in the owner calls it as
/// the whole program does. Red without DECISIONS 224 (a bare marker).
#[test]
fn pub_callable_runners_publish_their_own_callbacks() {
    let src = r#"function single(cb: any, create: any) { return create((v: any) => cb(v)); }
function root() { return single((x: any) => x.described, (inner: any) => inner({described: 1})); } console.log(root());"#;
    let whole = facts(src);
    let unit = units(src);
    let offset = src.find("inner({").unwrap() as u32 + 1;
    for (mode, pf) in [("whole", &whole), ("unit", &unit)] {
        let calls: Vec<_> = pf
            .sites
            .iter()
            .filter(|s| s.is_call && s.byte_offset == offset)
            .collect();
        assert!(!calls.is_empty(), "{mode}\n{}", pf.dump_text());
        assert!(
            calls.iter().all(|s| is_proven(s)),
            "{mode}: the runner's own callback\n{}",
            pf.dump_text()
        );
    }
    not_stronger(&whole, &unit, "described");
}

/// Refusal arm: the runner's own callback also held by unknown code is
/// called with anything (its parameter stays ⊤ per unit, as whole).
#[test]
fn pub_callable_runner_callbacks_held_by_unknown_code_stay_unknown() {
    let src = r#"function single(cb: any, create: any) { const own = (v: any) => cb(v.ownParam); (globalThis as any).held = own; return create(own); }
function root() { return single((x: any) => x, (inner: any) => inner({ownParam: 1})); } console.log(root());"#;
    let whole = facts(src);
    let unit = units(src);
    for (mode, pf) in [("whole", &whole), ("unit", &unit)] {
        assert!(
            sites_of(pf, "ownParam").iter().all(|s| !is_proven(s)),
            "{mode}: unknown callers\n{}",
            pf.dump_text()
        );
    }
}
