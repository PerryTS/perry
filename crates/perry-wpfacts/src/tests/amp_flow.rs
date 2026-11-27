//! FLOW-ONLY (AMP.md section 6): at the fixpoint every node that holds ⊤
//! is reached from a seed along (a) edge copies and (b) a result rule
//! (Load, IdxLoad, ProtoKey, a call's or `new`'s return) whose operand
//! holds ⊤ and which writes its own dst. Then the sites a seed changes are
//! a subset of the sites whose operand lies in FwdReach(seed).
//!
//! Each fixture is one unrelated program (classes with methods, arrays, a
//! promise, try/catch, console, builtin namespace reads) with one seed
//! appended after `/*SEED*/`; the program's byte offsets are the same in
//! both runs. The solve runs in flow record mode (`topstop::flow_on`):
//! every ⊤ creation is recorded, and `record` computes FwdReach from the
//! seeds over the model's edges (`succ`) and the result rules' dsts.
use super::*;
use crate::gen::Gen;
use crate::model::{Cons, Model, NodeId, TOP};
use crate::topstop::FlowArrival;
use std::cell::RefCell;
use std::collections::BTreeMap;

#[derive(Default, Clone)]
struct Flow {
    arrivals: Vec<FlowArrival>,
    /// ⊤ creations other than a result rule writing its own dst.
    violations: Vec<String>,
    /// (byte offset, key, is_call) -> the site's operand is in FwdReach.
    in_reach: BTreeMap<(u32, String, bool), bool>,
    top_nodes_outside: usize,
}

thread_local! {
    static LAST: RefCell<Option<Flow>> = const { RefCell::new(None) };
}

/// The dst a result rule writes (K6): what ⊤ in its operand may reach.
fn result_dst(m: &Model, c: &Cons) -> Option<NodeId> {
    Some(match *c {
        Cons::Load { dst, .. }
        | Cons::ProtoKey { dst, .. }
        | Cons::IdxLoad { dst, .. }
        | Cons::IdxKey { dst, .. }
        | Cons::IdxOwnLoad { dst, .. }
        | Cons::IdxOwnKey { dst, .. } => dst,
        Cons::Call { call }
        | Cons::CallThis { call, .. }
        | Cons::New { call }
        | Cons::NewProgram { call }
        | Cons::NewNamed { call, .. }
        | Cons::MCall { call, .. } => m.calls[call as usize].ret,
        _ => return None,
    })
}

/// After the solve (lib.rs, cfg(test)): in flow record mode, the arrivals,
/// FwdReach(seeds) and each site's membership.
pub(crate) fn record(g: &Gen) {
    let m = &g.m;
    let Some(arrivals) = m.stop_top.as_ref().and_then(|s| s.flow.clone()) else {
        return;
    };
    let mut reach = vec![false; m.nodes.len()];
    let mut work: Vec<NodeId> = arrivals
        .iter()
        .filter(|a| a.class == 1 && (a.node as usize) < m.nodes.len())
        .map(|a| a.node)
        .collect();
    while let Some(n) = work.pop() {
        if std::mem::replace(&mut reach[n as usize], true) {
            continue;
        }
        let node = &m.nodes[n as usize];
        let mut next: Vec<NodeId> = node.succ.clone();
        next.extend(node.cons.iter().filter_map(|c| result_dst(m, c)));
        work.extend(next.into_iter().filter(|&d| (d as usize) < m.nodes.len() && !reach[d as usize]));
    }
    let top_nodes_outside = (0..m.nodes.len())
        .filter(|&n| !reach[n] && n as NodeId != m.top_tok_node && m.nodes[n].pts.contains_tok(TOP))
        .count();
    let mut in_reach = BTreeMap::new();
    for s in g.read_sites.iter().chain(g.store_sites.iter()) {
        let k = (s.byte_offset, m.atoms[s.key as usize].to_string(), s.is_call);
        let r = (s.recv as usize) < reach.len() && reach[s.recv as usize];
        *in_reach.entry(k).or_insert(false) |= r;
    }
    // An A arrival is allowed only as a result rule writing its own dst;
    // a marker (M) is never one.
    let violations = arrivals
        .iter()
        .filter(|a| a.class != 1)
        .filter(|a| !(a.class == 2 && a.rule.is_some_and(|(_, c)| result_dst(m, &c) == Some(a.node))))
        .map(|a| {
            let rule = a.rule.map_or("-", |(_, c)| c.kind_name());
            format!("class={} rule={rule} {}:{} {}", a.class, a.at.file(), a.at.line(), a.what)
        })
        .collect();
    LAST.with_borrow_mut(|l| *l = Some(Flow { arrivals, violations, in_reach, top_nodes_outside }));
}

fn run(src: &str) -> (ProgramFacts, Flow) {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    crate::topstop::flow_on(true);
    LAST.with_borrow_mut(|l| *l = None);
    let pf = crate::compute(&[(path, &hir)], &cfg);
    crate::topstop::flow_on(false);
    let f = LAST.with_borrow_mut(|l| l.take()).expect("flow record ran");
    (pf, f)
}

const PROGRAM: &str = r#"declare function get(): any;
class P { x = 1; get() { return this.x; } bump(n: number) { this.x += n; return this; } }
const p = new P();
const arr = [1, 2, 3];
arr.push(4);
const s = p.get() + arr.length + p.bump(2).get();
Promise.resolve(s).then((v) => console.log(v));
try { console.log(p.get()); } catch (e) { console.log(e); }
const q = Math.max(1, 2);
console.log(q, performance.now(), JSON.stringify({ a: q }));
/*SEED*/
"#;

fn violations(f: &Flow) -> Vec<String> {
    f.violations.clone()
}

/// The verdict changes the seed makes in the program, and those whose
/// operand is outside FwdReach(seed).
fn check(seed: &str) -> (Vec<String>, Vec<String>, Flow) {
    let base_src = PROGRAM.to_string();
    let seed_src = format!("{PROGRAM}{seed}\n");
    let end = PROGRAM.find("/*SEED*/").unwrap() as u32;
    let (b, _) = run(&base_src);
    let (s, f) = run(&seed_src);
    let key = |x: &SiteFacts| (x.byte_offset, x.key.clone(), x.is_call);
    let bv: BTreeMap<_, String> = b
        .sites
        .iter()
        .filter(|x| x.byte_offset < end)
        .map(|x| (key(x), format!("{:?}", x.verdict)))
        .collect();
    let mut changed = Vec::new();
    let mut outside = Vec::new();
    for x in s.sites.iter().filter(|x| x.byte_offset < end) {
        let v = format!("{:?}", x.verdict);
        let k = key(x);
        if bv.get(&k) != Some(&v) {
            let line = format!("@{} .{} call={} {:?} -> {v}", k.0, k.1, k.2, bv.get(&k));
            if !f.in_reach.get(&k).copied().unwrap_or(false) {
                outside.push(line.clone());
            }
            changed.push(line);
        }
    }
    eprintln!(
        "AMP-FLOW seed {seed:?}: changed {} outside-reach {} top-nodes-outside-reach {} violations {}",
        changed.len(),
        outside.len(),
        f.top_nodes_outside,
        violations(&f).len()
    );
    for l in &outside {
        eprintln!("AMP-FLOW   outside {l}");
    }
    for v in violations(&f).iter().take(8) {
        eprintln!("AMP-FLOW   violation {v}");
    }
    (changed, outside, f)
}

fn assert_flow_only(seed: &str) {
    let (_, outside, f) = check(seed);
    assert!(outside.is_empty(), "{seed}: {} sites changed outside FwdReach(seed): {outside:#?}", outside.len());
    let v = violations(&f);
    assert!(v.is_empty(), "{seed}: {} ⊤ creations other than a result rule: {v:#?}", v.len());
}

/// F1: a ⊤ callee given a class instance (open until AMP-RESOLVE resolves
/// the callee: K2's ⊤-callee carve-out writes P's layout under any key).
#[test]
#[ignore = "AMP-RESOLVE (K4/K5) removes the ⊤-callee carve-out"]
fn f1_top_callee_given_an_instance() {
    assert_flow_only("const f = get();\nf(new P());");
}

/// F2: a store of an object into ⊤ (open until AMP-RESOLVE).
#[test]
#[ignore = "AMP-RESOLVE (K4/K5) removes the store-into-⊤ carve-out"]
fn f2_store_of_an_object_into_top() {
    assert_flow_only("const t = get();\nt.k = new P();");
}

/// F3: a computed store into ⊤ beside `arr.push` (K10).
#[test]
fn f3_computed_store_into_top() {
    assert_flow_only("const t = get();\nt[String(Math.random())] = 1;");
}

/// F4: a ⊤ call beside builtin namespace reads (K8).
#[test]
fn f4_top_call() {
    assert_flow_only("const r = get()();\nconsole.log(r);");
}

/// F8: the m2 shape (typeof of a computed global read): no site changes.
#[test]
fn f8_typeof_of_a_computed_global_read() {
    let (changed, _, _) =
        check("const gv = (globalThis as any)[\"x\" + Math.random()];\nconsole.log(typeof gv);");
    assert!(changed.is_empty(), "the m2 shape changed {} program sites: {changed:#?}", changed.len());
}
