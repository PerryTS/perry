//! Throw origins from the same resolved graph as the GC and unwind columns.
use crate::{
    graph::Graph,
    table::{PassSource, ThrowClass},
};
use anyhow::{bail, Result};
use std::collections::{BTreeSet, VecDeque};

#[derive(Clone, Debug)]
pub struct Declaration {
    pub symbol: String,
    pub operands: Vec<PassSource>,
}
impl Declaration {
    #[cfg(test)]
    pub fn own(symbol: &str, argument: u16) -> Self {
        Self {
            symbol: symbol.into(),
            operands: vec![PassSource {
                source: None,
                argument,
            }],
        }
    }
}

#[derive(Default, Debug)]
pub struct ThrowReach {
    pub class: Vec<ThrowClass>,
    pub passes: Vec<BTreeSet<PassSource>>,
    pub own_unmapped: Vec<BTreeSet<String>>,
    pub open_witness: Vec<Option<usize>>,
    pub program_reach: Vec<bool>,
    pub any_leaf_reach: Vec<bool>,
    pub raw_program_reach: Vec<bool>,
    pub interpreted_reach: Vec<bool>,
    pub causes: Vec<Option<String>>,
}

fn leaf(g: &Graph, i: usize) -> Option<&'static str> {
    [
        "throw_core",
        "rethrow_caught",
        "throw_program_value",
        "throw_open",
    ]
    .into_iter()
    .find(|name| {
        let symbol = format!("perry_runtime::exception::throw_leaf::{name}");
        g.spellings(i).iter().any(|s| *s == symbol)
    })
}

fn backward(
    g: &Graph,
    extra: &[Vec<usize>],
    seeds: impl Fn(usize) -> bool,
    cut: impl Fn(usize) -> bool,
) -> Vec<Option<usize>> {
    let mut next = vec![None; g.nodes.len()];
    let mut work = VecDeque::new();
    for (i, n) in next.iter_mut().enumerate() {
        if seeds(i) {
            *n = Some(i);
            work.push_back(i);
        }
    }
    while let Some(t) = work.pop_front() {
        for &c in g.pred[t].iter().chain(&extra[t]) {
            if next[c].is_none() && !cut(c) {
                next[c] = Some(t);
                work.push_back(c);
            }
        }
    }
    next
}

#[cfg(test)]
pub(crate) fn solve(
    g: &Graph,
    extra: &[Vec<usize>],
    cut: &[bool],
    structural: &[Option<String>],
    declarations: &[Declaration],
) -> Result<ThrowReach> {
    let stems = std::collections::HashSet::new();
    let owned: Vec<_> = g
        .nodes
        .iter()
        .map(|n| crate::table::perry_object(&n.obj, &stems))
        .collect();
    let mut entries = vec![false; g.nodes.len()];
    for &i in g.by_global.values().flatten() {
        entries[i] = true;
    }
    solve_owned(g, extra, cut, structural, declarations, &owned, &entries)
}

pub(crate) fn solve_owned(
    g: &Graph,
    extra: &[Vec<usize>],
    cut: &[bool],
    structural: &[Option<String>],
    declarations: &[Declaration],
    owned: &[bool],
    entries: &[bool],
) -> Result<ThrowReach> {
    let nn = g.nodes.len();
    let leaves: Vec<_> = (0..nn).map(|i| leaf(g, i)).collect();
    let stop = |i: usize| cut[i] || leaves[i].is_some();
    let raw = backward(
        g,
        extra,
        |i| {
            g.spellings(i).contains(&"js_throw")
                || g.nodes[i].calls.contains("js_throw")
                || g.nodes[i].externs.contains("js_throw")
        },
        |_| false,
    );
    let raw_callers: Vec<_> = (0..nn)
        .filter(|&i| raw[i].is_some() && !g.spellings(i).contains(&"js_throw") && owned[i])
        .map(|i| g.label(i))
        .collect();
    if !raw_callers.is_empty() {
        bail!(
            "runtime graph reaches raw js_throw:\n  {}",
            raw_callers.join("\n  ")
        );
    }
    let any_leaf = backward(g, extra, |i| leaves[i].is_some(), |_| false);
    let raw_program = backward(
        g,
        extra,
        |i| leaves[i] == Some("throw_program_value"),
        |_| false,
    );
    let program = backward(g, extra, |i| leaves[i] == Some("throw_program_value"), stop);
    let core = backward(g, extra, |i| leaves[i] == Some("throw_core"), stop);
    let mut declared = vec![None; nn];
    for d in declarations {
        let found: Vec<_> = (0..nn)
            .filter(|&i| g.spellings(i).contains(&d.symbol.as_str()))
            .collect();
        // Absent functions can be disabled for this target or eliminated by
        // optimization. Coverage below still rejects every emitted bare path.
        for i in found {
            if program[i].is_none() {
                bail!(
                    "{}: unused passes declaration (no program-value leaf reachable)",
                    d.symbol
                );
            }
            if d.operands.is_empty() {
                bail!("{}: empty passes declaration", d.symbol);
            }
            if declared[i].is_some_and(|old: &Declaration| old.operands != d.operands) {
                bail!("{}: conflicting passes declarations", d.symbol);
            }
            declared[i] = Some(d);
        }
    }
    let uncovered = backward(
        g,
        extra,
        |i| leaves[i] == Some("throw_program_value"),
        |i| stop(i) || declared[i].is_some(),
    );
    let missing: Vec<_> = (0..nn)
        .filter(|&i| uncovered[i].is_some() && leaves[i].is_none() && !cut[i] && entries[i])
        .map(|i| g.label(i))
        .collect();
    if !missing.is_empty() {
        bail!(
            "program-value leaf reachable without a passes declaration:\n  {}",
            missing.join("\n  ")
        );
    }
    let causes: Vec<_> = (0..nn)
        .map(|i| {
            if cut[i] {
                None
            } else if leaves[i] == Some("throw_open") {
                Some("throw_open".into())
            } else if leaves[i].is_some() {
                None
            } else {
                structural[i].clone()
            }
        })
        .collect();
    let open = backward(g, extra, |i| causes[i].is_some(), stop);
    let mut passes = vec![BTreeSet::new(); nn];
    let mut own_unmapped = vec![BTreeSet::new(); nn];
    let mut work = VecDeque::new();
    let mut queued = vec![false; nn];
    for (i, d) in declared.iter().enumerate() {
        if let Some(d) = d {
            passes[i].extend(d.operands.iter().cloned());
            work.push_back(i);
            queued[i] = true;
        }
    }
    while let Some(t) = work.pop_front() {
        queued[t] = false;
        let stored: Vec<_> = passes[t]
            .iter()
            .filter(|p| p.source.is_some())
            .cloned()
            .collect();
        let mut unknown = own_unmapped[t].clone();
        if passes[t].iter().any(|p| p.source.is_none()) {
            unknown.insert(g.label(t));
        }
        for &c in g.pred[t].iter().chain(&extra[t]) {
            if stop(c) || declared[c].is_some() {
                continue;
            }
            let before = (passes[c].len(), own_unmapped[c].len());
            passes[c].extend(stored.iter().cloned());
            own_unmapped[c].extend(unknown.iter().cloned());
            if before != (passes[c].len(), own_unmapped[c].len()) && !queued[c] {
                work.push_back(c);
                queued[c] = true;
            }
        }
    }
    let class = (0..nn)
        .map(|i| {
            if open[i].is_some() || !own_unmapped[i].is_empty() {
                ThrowClass::Open
            } else if program[i].is_some() {
                ThrowClass::Passes
            } else if core[i].is_some() {
                ThrowClass::Core
            } else {
                ThrowClass::None
            }
        })
        .collect();
    Ok(ThrowReach {
        class,
        passes,
        own_unmapped,
        open_witness: open,
        program_reach: program.iter().map(Option::is_some).collect(),
        any_leaf_reach: any_leaf.iter().map(Option::is_some).collect(),
        raw_program_reach: raw_program.iter().map(Option::is_some).collect(),
        causes,
        interpreted_reach: vec![false; nn],
    })
}

impl ThrowReach {
    pub fn open_cause(&self, g: &Graph, mut i: usize) -> String {
        if let Some(mut t) = self.open_witness[i] {
            let mut caller = i;
            while t != i {
                caller = i;
                i = t;
                t = self.open_witness[i].unwrap_or(i);
            }
            if self.causes[i].as_deref() == Some("throw_open") {
                format!("throw_open at {}", g.label(caller))
            } else {
                format!(
                    "{}: {}{}",
                    self.causes[i].as_deref().unwrap_or("unknown"),
                    g.label(i),
                    g.nodes[i]
                        .sites
                        .first()
                        .map_or(String::new(), |s| format!(" @{s}"))
                )
            }
        } else {
            format!(
                "unmapped own argument: {}",
                self.own_unmapped[i]
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" | ")
            )
        }
    }
}

pub(crate) fn declarations(root: &std::path::Path) -> Result<Vec<Declaration>> {
    let raw = perry_runtime_manifest::realm_source::read_throw_declarations(
        &root.join("crates/perry-runtime/src"),
    )
    .map_err(anyhow::Error::msg)?;
    let values: serde_json::Value = serde_json::from_str(&raw.encode())?;
    let mut result = Vec::new();
    for value in values
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("throw declarations must be an array"))?
    {
        let symbol = value["symbol"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("throw declaration missing symbol"))?
            .to_owned();
        let mut operands = Vec::new();
        for op in value["operands"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("{symbol}: missing operands"))?
        {
            operands.push(PassSource {
                source: op["source"].as_str().map(str::to_owned),
                argument: u16::try_from(
                    op["argument"]
                        .as_u64()
                        .ok_or_else(|| anyhow::anyhow!("{symbol}: invalid argument"))?,
                )?,
            });
        }
        result.push(Declaration { symbol, operands });
    }
    Ok(result)
}
