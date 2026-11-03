//! DECISIONS 252: a work budget per subject. A subject over its budget
//! gives up alone: its nodes hold ⊤ (what reaches them escapes, its rules
//! are retired), its sites are unseen code with the cause `gave-up-budget`,
//! and no other subject loses a fact. The program-wide abort is gone.
use super::*;
use crate::model::{TokKind, TOP};

/// The verdict of the first site of `key` at or after `marker` in `src`.
fn verdict_at(pf: &ProgramFacts, src: &str, marker: &str, key: &str) -> Verdict {
    let at = src.find(marker).expect("marker") as u32;
    pf.sites
        .iter()
        .filter(|s| s.key == key && s.byte_offset >= at)
        .min_by_key(|s| s.byte_offset)
        .unwrap_or_else(|| panic!("site .{key} after {marker:?} in\n{}", pf.dump_text()))
        .verdict
        .clone()
}

fn proven(v: &Verdict) -> bool {
    !matches!(v, Verdict::No(_))
}

/// The causes of every site record of `key` (the ledger's view).
fn site_causes(pf: &ProgramFacts, key: &str) -> Vec<String> {
    let want = format!("\"key\":{}", crate::diagnostics::quote(key));
    let mut out = Vec::new();
    for (_, lines) in &pf.diagnostics {
        for l in lines
            .iter()
            .filter(|l| l.contains("\"record\":\"site\"") && l.contains(&want))
        {
            let Some(at) = l.find("\"causes\":[") else {
                continue;
            };
            let rest = &l[at + 10..];
            let list = &rest[..rest.find(']').unwrap_or(0)];
            for part in list.split("\"cause\":\"").skip(1) {
                out.push(part[..part.find('"').unwrap()].to_string());
            }
        }
    }
    out
}

/// `heavy` births a literal per statement through its parameter (work far
/// over the test budget); `light` reads class fields; `caller` passes an
/// object to `heavy` and reads the result and the object after the call.
fn heavy_light(n: usize) -> String {
    let mut s = String::from(
        "class L { v = 1; w = 2; }\n\
         function light(l: L) { return l.v + l.w; }\n\
         function heavy(o: any, n: number) { let s = 0;\n",
    );
    for i in 0..n {
        s.push_str(&format!(
            "  const q{i} = {{ a{i}: n, f: o }}; s += q{i}.a{i};\n"
        ));
    }
    s.push_str(
        "  return { len: s }; }\n\
         function caller() { const o = { p: 1 }; const r = heavy(o, 3); return r.len + o.p; }\n\
         console.log(light(new L()), caller());\n",
    );
    s
}

#[test]
fn a_function_over_its_budget_gives_up_alone() {
    // The budget: eight work (two nodes) per literal. The module body
    // declares one class per literal (the HIR lowers a closed literal to a
    // class) and stays under it; `heavy` births several nodes and far more
    // work per literal and goes over it; nothing else grows with the
    // literals. The program without the literals is far under it too.
    const N: usize = 2_000;
    let budget = 8 * N as u64;
    let n0 = facts_with(
        &heavy_light(0),
        FactsConfig {
            budget: Budget(budget),
            ..Default::default()
        },
    );
    assert!(n0.stats.gave_up.is_empty(), "{:?}", n0.stats.gave_up);
    assert!((n0.stats.nodes as u64) < budget / 4, "{}", n0.stats.nodes);
    let src = heavy_light(N);
    let pf = facts_with(
        &src,
        FactsConfig {
            budget: Budget(budget),
            diagnostics: true,
            ..Default::default()
        },
    );
    let dump = pf.dump_text();
    // Exactly `heavy` gave up, and the dump says so.
    assert_eq!(pf.stats.gave_up.len(), 1, "{:?}\n{dump}", pf.stats.gave_up);
    let (name, _, kind) = &pf.stats.gave_up[0];
    assert!(name.contains("heavy"), "{name} gave up\n{dump}");
    assert!(dump.contains("\"record\":\"gave-up\""), "{dump}");
    assert!(
        dump.lines().any(|l| l.starts_with("# gave-up ")
            && l.contains("heavy")
            && l.contains(&format!("budget={budget} kind={kind}"))),
        "{dump}"
    );
    assert!(!pf.stats.settle_stalled);
    // (a) light's class-field reads are proven.
    assert!(proven(&verdict_at(&pf, &src, "return l.v", "v")), "{dump}");
    assert!(proven(&verdict_at(&pf, &src, "return l.v", "w")), "{dump}");
    // (b) heavy's own sites are unseen code: unproven, cause gave-up-budget.
    for key in ["a0", "a1000", "a1999"] {
        assert!(!proven(&verdict_at(&pf, &src, "function heavy", key)), "{key} proven\n{dump}");
        let causes = site_causes(&pf, key);
        assert!(
            !causes.is_empty() && causes.iter().all(|c| c == "gave-up-budget"),
            "{key}: {causes:?}\n{dump}"
        );
    }
    // (c) the caller reads ⊤ (not nothing) from the given-up call, and the
    // object it passed in is not read with its pre-call layout.
    assert!(!proven(&verdict_at(&pf, &src, "return r.len", "len")), "{dump}");
    let causes = site_causes(&pf, "len");
    assert!(
        !causes.is_empty() && causes.iter().all(|c| c == "gave-up-budget"),
        "the caller's result read: {causes:?}\n{dump}"
    );
    assert!(!proven(&verdict_at(&pf, &src, "return r.len", "p")), "{dump}");
    assert!(tmpl(&pf, "L").fixed_layout, "{dump}");
}

/// A model with two bodies: subject 0 owns a node holding `n` values with
/// `n` readers (work ~ n²), subject 1 a node with one value and one reader.
fn two_subjects(n: usize, budget: u64) -> (crate::model::Model, Vec<u32>, u32) {
    let mut m = copy_solver_model();
    m.budget = Budget(budget);
    m.cur_subject = 5;
    let src = m.node_l("heavy");
    let values: Vec<_> = (100..100 + n as u32).map(|i| m.tok(TokKind::ModNs(i))).collect();
    let readers: Vec<u32> = (0..n).map(|_| m.node_l("reader")).collect();
    for &r in &readers {
        m.edge(src, r);
    }
    m.cur_subject = 6;
    let one = m.tok(TokKind::ModNs(90));
    let light = m.node_l("light");
    let out = m.node_l("out");
    m.edge(light, out);
    m.add_tok(light, one);
    m.cur_subject = 5;
    m.add(src, &values, 0);
    m.solve();
    (m, readers, out)
}

#[test]
fn a_subject_over_its_budget_leaves_the_other_untouched() {
    let (m, readers, out) = two_subjects(200, 1_000);
    let heavy = m.charge(5).expect("subject 5");
    let light = m.charge(6).expect("subject 6");
    assert!(heavy.gave_up && heavy.work + heavy.pts > 1_000, "work {} pts {} gave_up {}", heavy.work, heavy.pts, heavy.gave_up);
    assert!(!light.gave_up && light.work <= 8, "work {} gave_up {}", light.work, light.gave_up);
    assert_eq!(m.gave_up.len(), 1, "{:?}", m.gave_up);
    // Readers of the given-up node hold ⊤; the other subject's reader holds
    // exactly its value.
    for &r in &readers {
        assert!(m.nodes[r as usize].pts.contains_tok(TOP), "reader {r} reads nothing");
    }
    let out_pts = m.nodes[out as usize].pts.to_sorted();
    assert_eq!(out_pts.len(), 1, "{out_pts:?}");
    assert!(!m.nodes[out as usize].pts.contains_tok(TOP));
}

#[test]
fn a_given_up_subject_does_no_work_in_later_rounds() {
    let (mut m, readers, _) = two_subjects(200, 1_000);
    let w0 = m.charge(5).expect("subject 5").work;
    let before = m.work;
    // New values into the given-up node, a new reader of it, a new node of
    // its subject: arrivals escape, the reader reads ⊤, the node is born
    // retired; none of it is this subject's work.
    let more: Vec<_> = (300..340).map(|i| m.tok(TokKind::ModNs(i))).collect();
    m.cur_subject = 6;
    let feeder = m.node_l("feeder");
    m.edge(feeder, readers[0]);
    m.add(feeder, &more, 0);
    m.cur_subject = 5;
    let born = m.node_l("late");
    assert!(m.nodes[born as usize].retired && m.nodes[born as usize].pts.contains_tok(TOP));
    m.solve();
    // A live node would deliver the 40 arrivals to 200 readers (8,000 work);
    // the retired one escapes them without a pop: O(1) per arrival and birth.
    let w1 = m.charge(5).expect("subject 5").work;
    assert!(w1 - w0 < 40, "work charged after the give-up: {} -> {w1}", w0);
    assert!(m.work >= before);
    assert_eq!(m.gave_up.len(), 1, "{:?}", m.gave_up);
}

#[test]
fn a_shared_subject_over_its_budget_gives_up_alone() {
    // `Object.prototype`'s any-key node holds many values read by many
    // holders through their prototype chain (the dense-copy shape): the
    // work is the shared token's, and only it gives up.
    let mut m = copy_solver_model();
    m.budget = Budget(8_000);
    m.cur_subject = 5;
    let root = m.tok(TokKind::ObjectProto);
    let shared = m.tok_subject(root);
    assert_eq!(shared, crate::model::SUBJECT_TOK | root);
    let holders: Vec<_> = (0..24).map(|i| m.tok(TokKind::ModNs(i))).collect();
    let values: Vec<_> = (100..200).map(|i| m.tok(TokKind::ModNs(i))).collect();
    let any = m.field_any(root);
    for &t in &holders {
        let pn = m.proto_node(t);
        m.add_tok(pn, root);
    }
    let mut reads = Vec::new();
    for i in 0..64 {
        let key = crate::model::Key::Named(m.atom(&format!("property{i}")));
        for &t in &holders {
            reads.push(m.cv(t, key));
        }
    }
    // An unrelated body keeps its value.
    let one = m.tok(TokKind::ModNs(90));
    let a = m.node_l("a");
    let b = m.node_l("b");
    m.edge(a, b);
    m.add_tok(a, one);
    m.add(any, &values, 0);
    m.solve();
    assert_eq!(m.gave_up.len(), 1, "{:?}", m.gave_up);
    assert!(m.charge(shared).is_some_and(|c| c.gave_up), "{:?}", m.gave_up);
    assert!(m.charge(5).is_none_or(|c| !c.gave_up));
    for &h in &holders {
        assert!(m.charge(m.tok_subject(h)).is_none_or(|c| !c.gave_up), "holder gave up");
    }
    assert!(m.nodes[reads[0] as usize].pts.contains_tok(TOP), "a read of the given-up node reads nothing");
    assert_eq!(m.nodes[b as usize].pts.to_sorted(), vec![one]);
}

/// The settle stall guard (DESIGN §2) is a pure verdict: a settle pass that
/// reports more with `(pops, work)` unchanged since the last pass that did
/// recorded nothing and is a stall; moved counters, a first pass, or a pass
/// that reports nothing are not.
#[test]
fn a_settle_pass_without_a_pop_or_work_since_the_last_is_a_stall() {
    use crate::solve::settle_stall;
    assert!(settle_stall(Some((3, 10)), (3, 10), true));
    assert!(!settle_stall(Some((3, 10)), (4, 10), true));
    assert!(!settle_stall(Some((3, 10)), (3, 11), true));
    assert!(!settle_stall(None, (3, 10), true));
    assert!(!settle_stall(Some((3, 10)), (3, 10), false));
}
