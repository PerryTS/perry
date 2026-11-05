//! Keeps built-in effect declarations true (BFX1 design 4(e)).
//!
//! A realm declaration may state what its body does (`effects = { ... }` on
//! the `#[realm(..)]` attribute; schema `realm_effect.rs`). From the same call
//! graph the GC classes come from, two upper bounds per function:
//! - reaches a JS entry: a `js` seed, or an indirect call/jump, idiom or
//!   extern no rule accounts for (unknown code may be JS);
//! - reaches the write barrier: a `barrier` seed (collections are cut: a
//!   collection is not the call keeping a value).
//!
//! A declared body that reaches a JS entry must declare an effect that can
//! run program code: `calls`, `coerce`, or a `reads`/`stores` (an ordinary
//! [[Get]]/[[Set]] runs accessors and Proxy traps; the graph cannot tell those
//! apart from a callback). A declared body that reaches the barrier must
//! declare `stores` or return a fresh object. Over-reporting forces a
//! declaration, never a wrong fact.
use crate::graph::Graph;
use anyhow::{bail, Context, Result};
use std::collections::{BTreeMap, HashMap, VecDeque};

#[path = "../../perry-runtime-manifest/src/realm_effect.rs"]
#[allow(dead_code)] // The schema also carries the compiler-table emitter.
pub mod realm_effect;
use realm_effect::{RealmEffect, RealmOperand};

/// Per graph node: the next hop toward a seed (`Some(i)` on the seed itself).
#[derive(Default, Debug)]
pub struct EffectReach {
    pub js: Vec<Option<usize>>,
    pub barrier: Vec<Option<usize>>,
    /// `barrier` seed rules that matched no function (vacuous otherwise).
    pub dead_barrier_rules: Vec<String>,
}

fn backward(
    g: &Graph,
    extra_pred: &[Vec<usize>],
    seed: impl Fn(usize) -> bool,
    cut: impl Fn(usize) -> bool,
) -> Vec<Option<usize>> {
    let nn = g.nodes.len();
    let mut next = vec![None; nn];
    let mut work = VecDeque::new();
    for i in 0..nn {
        if seed(i) && !cut(i) {
            next[i] = Some(i);
            work.push_back(i);
        }
    }
    while let Some(t) = work.pop_front() {
        for &c in g.pred[t].iter().chain(&extra_pred[t]) {
            if next[c].is_none() && !cut(c) {
                next[c] = Some(t);
                work.push_back(c);
            }
        }
    }
    next
}

impl EffectReach {
    pub(crate) fn solve(
        g: &Graph,
        extra_pred: &[Vec<usize>],
        js_seed: impl Fn(usize) -> bool,
        barrier_seed: impl Fn(usize) -> bool,
        js_cut: impl Fn(usize) -> bool,
        barrier_cut: impl Fn(usize) -> bool,
        dead_barrier_rules: Vec<String>,
    ) -> Self {
        Self {
            js: backward(g, extra_pred, js_seed, js_cut),
            barrier: backward(g, extra_pred, barrier_seed, barrier_cut),
            dead_barrier_rules,
        }
    }

    fn path(&self, g: &Graph, next: &[Option<usize>], mut i: usize) -> String {
        let mut steps = vec![g.label(i)];
        while let Some(n) = next[i].filter(|&n| n != i) {
            i = n;
            steps.push(g.label(i));
            if steps.len() > 12 {
                steps.push("...".into());
                break;
            }
        }
        steps
            .iter()
            .map(|s| s.chars().take(100).collect::<String>())
            .collect::<Vec<_>>()
            .join(" -> ")
    }
}

/// One callable realm declaration with a body.
pub struct Native {
    pub name: String,
    pub symbol: String,
    pub effects: Vec<RealmEffect>,
}

/// Callable declarations of a `realm-v1.json` manifest.
pub fn natives(manifest: &str) -> Result<Vec<Native>> {
    let v: serde_json::Value = serde_json::from_str(manifest).context("realm manifest JSON")?;
    let decls = v["declarations"]
        .as_array()
        .context("manifest has no declarations")?;
    let mut out = Vec::new();
    for d in decls {
        if d.get("body").and_then(|b| b.as_str()).is_none() {
            continue;
        }
        let name = d["name"]
            .as_str()
            .context("declaration without name")?
            .to_owned();
        let Some(symbol) = d["function_info"]["source_symbol"]
            .as_str()
            .and_then(|s| s.strip_prefix("source:"))
        else {
            continue;
        };
        let effects = match d.get("effects") {
            Some(e) => serde_json::from_value(e.clone())
                .with_context(|| format!("{name}: effects do not match the schema"))?,
            None => Vec::new(),
        };
        out.push(Native {
            name,
            symbol: symbol.to_owned(),
            effects,
        });
    }
    Ok(out)
}

fn runs_code(e: &RealmEffect) -> bool {
    matches!(
        e,
        RealmEffect::Calls { .. }
            | RealmEffect::Coerce { .. }
            | RealmEffect::Reads(_)
            | RealmEffect::Stores { .. }
            | RealmEffect::Open
    )
}

fn keeps(e: &RealmEffect) -> bool {
    matches!(
        e,
        RealmEffect::Stores { .. }
            | RealmEffect::Returns(RealmOperand::Fresh { .. })
            | RealmEffect::Open
    )
}

/// The verdict for one declared body against its two reach bits.
pub fn violations(effects: &[RealmEffect], js: bool, barrier: bool) -> Vec<&'static str> {
    let mut out = Vec::new();
    if effects.is_empty() {
        return out;
    }
    if js && !effects.iter().any(runs_code) {
        out.push("reaches a JS entry but declares no calls/coerce/reads/stores");
    }
    if barrier && !effects.iter().any(keeps) {
        out.push("reaches the write barrier but declares no stores and no fresh result");
    }
    out
}

#[derive(Default)]
pub struct Report {
    pub lines: Vec<String>,
    pub census: String,
    pub failures: usize,
}

/// Compares every declared native with the graph; writes the census of all
/// realm natives (per distinct body symbol).
pub fn check(g: &Graph, reach: &EffectReach, natives: &[Native]) -> Result<Report> {
    if !reach.dead_barrier_rules.is_empty() {
        bail!(
            "barrier seeds matched no function (the check would be vacuous):\n  {}",
            reach.dead_barrier_rules.join("\n  ")
        );
    }
    let mut by_name: HashMap<&str, Vec<usize>> = HashMap::new();
    for i in 0..g.nodes.len() {
        for s in g.spellings(i) {
            by_name.entry(s).or_default().push(i);
        }
    }
    let mut report = Report::default();
    let mut census: BTreeMap<&str, (bool, bool, bool)> = BTreeMap::new();
    report
        .census
        .push_str("name\tsymbol\tfound\tjs\tbarrier\tdeclared\tverdict\n");
    for n in natives {
        let nodes = by_name.get(n.symbol.as_str());
        let found = nodes.is_some();
        let pick = |next: &[Option<usize>]| {
            nodes.and_then(|ns| ns.iter().copied().find(|&i| next[i].is_some()))
        };
        let (js, barrier) = (pick(&reach.js), pick(&reach.barrier));
        census.insert(&n.symbol, (found, js.is_some(), barrier.is_some()));
        let mut verdict = if n.effects.is_empty() {
            "undeclared".to_owned()
        } else {
            "ok".to_owned()
        };
        if !n.effects.is_empty() {
            if !found {
                verdict = "FAIL: declared body not in the call graph".into();
                report.failures += 1;
                report
                    .lines
                    .push(format!("{}: {} ({})", n.name, verdict, n.symbol));
            } else {
                let bad = violations(&n.effects, js.is_some(), barrier.is_some());
                if !bad.is_empty() {
                    report.failures += 1;
                    verdict = format!("FAIL: {}", bad.join("; "));
                    let mut line = format!("{}: {}", n.name, verdict);
                    if let Some(i) = js.filter(|_| bad.iter().any(|b| b.contains("JS"))) {
                        line.push_str(&format!("\n    js path: {}", reach.path(g, &reach.js, i)));
                    }
                    if let Some(i) = barrier.filter(|_| bad.iter().any(|b| b.contains("barrier"))) {
                        line.push_str(&format!(
                            "\n    barrier path: {}",
                            reach.path(g, &reach.barrier, i)
                        ));
                    }
                    report.lines.push(line);
                }
            }
        }
        report.census.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            n.name,
            n.symbol,
            found as u8,
            js.is_some() as u8,
            barrier.is_some() as u8,
            (!n.effects.is_empty()) as u8,
            verdict
        ));
    }
    let count = |f: &dyn Fn(&(bool, bool, bool)) -> bool| census.values().filter(|v| f(v)).count();
    let declared = natives.iter().filter(|n| !n.effects.is_empty()).count();
    report.lines.push(format!(
        "effects-census: natives={} bodies={} found={} js={} barrier={} js_and_barrier={} js_only={} barrier_only={} neither={} missing={}",
        natives.len(),
        census.len(),
        count(&|v| v.0),
        count(&|v| v.1),
        count(&|v| v.2),
        count(&|v| v.1 && v.2),
        count(&|v| v.1 && !v.2),
        count(&|v| !v.1 && v.2),
        count(&|v| v.0 && !v.1 && !v.2),
        count(&|v| !v.0),
    ));
    report.lines.push(format!(
        "effects-check: declared={declared} failures={}",
        report.failures
    ));
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::realm_effect::*;
    use super::violations;
    use std::borrow::Cow;

    fn this() -> RealmSlot {
        RealmSlot {
            root: RealmSlotRoot::This,
            steps: Cow::Borrowed(&[]),
        }
    }

    #[test]
    fn a_callback_caller_declared_as_returning_a_primitive_fails() {
        // Sabotage: forEach declared `returns(prim(undefined))` only.
        let wrong = [RealmEffect::Returns(RealmOperand::Prim(Cow::Borrowed(&[
            RealmPrim::Undefined,
        ])))];
        assert_eq!(violations(&wrong, true, false).len(), 1);
        let right = [
            RealmEffect::Calls {
                callee: RealmSlot {
                    root: RealmSlotRoot::Arg(0),
                    steps: Cow::Borrowed(&[]),
                },
                this: RealmOperand::Slot(RealmSlot {
                    root: RealmSlotRoot::Arg(1),
                    steps: Cow::Borrowed(&[]),
                }),
                args: Cow::Borrowed(&[]),
            },
            wrong[0].clone(),
        ];
        assert!(violations(&right, true, false).is_empty());
    }

    #[test]
    fn a_barrier_reaching_body_must_store_or_return_fresh() {
        let reads = [RealmEffect::Reads(Cow::Owned(vec![this()]))];
        assert_eq!(violations(&reads, false, true).len(), 1);
        let fresh = [RealmEffect::Returns(RealmOperand::Fresh {
            proto: Cow::Borrowed("Array.prototype"),
            elems: Cow::Borrowed(&[]),
        })];
        assert!(violations(&fresh, false, true).is_empty());
        let stores = [RealmEffect::Stores {
            src: this(),
            dst: this(),
        }];
        assert!(violations(&stores, false, true).is_empty());
        // Undeclared bodies are unknown code: nothing to check.
        assert!(violations(&[], true, true).is_empty());
    }
}
