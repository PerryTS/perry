//! GC and unwind effects derived from the machine code of a runtime link set.
//!
//! This is the compatibility port of scripts/gc_call_effects/callgraph.py.
//! It deliberately retains that analyzer's ownership scan and conservative
//! relocation/alias semantics. No table or compiler pipeline changes live here.

mod archive;
mod graph;
mod instructions;
mod rules;
mod table;

pub use archive::build_graph;
pub use graph::{classify, Classification, Graph, Mode, Node, Stats};
pub use rules::{load_rules, Rule, SEEDS};
pub use table::{compare, read_table, shadowed_symbols, write_table, Class, Drift, Entry, Table};

use anyhow::{bail, Result};
use std::path::{Path, PathBuf};

pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Analyze with all three hard checks enabled. A failure never writes a table.
pub fn analyze(
    archives: &[PathBuf],
    rules: &[Rule],
    root: &Path,
) -> Result<(Graph, Classification)> {
    let g = build_graph(archives)?;
    let mut res = classify(&g, rules, Some(root))?;
    let shadow = shadowed_symbols(root)?;
    for (name, row) in &mut res.table {
        if shadow.contains(name) {
            if row.class != Class::Reenters {
                res.stats.shadowed_demoted.push(name.clone());
            }
            row.class = Class::Reenters;
            row.throws = true;
        }
    }
    res.validate()?;
    Ok((g, res))
}
impl Classification {
    pub fn agreement_violations(&self) -> Vec<String> {
        self.table
            .iter()
            .filter(|(_, e)| e.class <= Class::AllocOnly && e.throws)
            .map(|(n, _)| n.clone())
            .collect()
    }
    pub fn validate(&self) -> Result<()> {
        let bad = self.agreement_violations();
        if !bad.is_empty() {
            bail!(
                "Leaf/AllocOnly symbols with throws=yes -- the solves disagree:\n  {}",
                bad.join("\n  ")
            );
        }
        if !self.stats.forbidden_calls.is_empty() {
            bail!(
                "a forbid premise is violated:\n  {}",
                self.stats.forbidden_calls.join("\n  ")
            );
        }
        if !self.stats.dead_rules.is_empty() {
            bail!(
                "named seed/cut rules that match NO node:\n  {}",
                self.stats.dead_rules.join("\n  ")
            );
        }
        Ok(())
    }
}
