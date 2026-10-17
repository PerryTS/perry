//! GC and unwind effects derived from the machine code of a runtime link set.
//!
//! This is the compatibility port of scripts/gc_call_effects/callgraph.py.
//! It deliberately retains that analyzer's ownership scan and conservative
//! relocation/alias semantics.
//!
//! Without the default `analyzer` feature the crate is only the sidecar
//! format ([`sidecar`]) and the table rows ([`Table`], [`parse_table`]): what
//! perry reads at compile time. The format has one definition, here, for the
//! producer and the consumer.

#[cfg(feature = "analyzer")]
mod archive;
#[cfg(feature = "analyzer")]
mod graph;
#[cfg(feature = "analyzer")]
mod instructions;
#[cfg(feature = "analyzer")]
mod rules;
pub mod sidecar;
mod table;

#[cfg(feature = "analyzer")]
pub use archive::build_graph;
#[cfg(feature = "analyzer")]
pub use graph::{classify, Classification, Graph, Mode, Node, Stats};
#[cfg(feature = "analyzer")]
pub use rules::{load_rules, Rule, SEEDS};
#[cfg(feature = "analyzer")]
pub use table::shadowed_symbols;
pub use table::{compare, parse_table, read_table, write_table, Class, Drift, Entry, Table};

#[cfg(feature = "analyzer")]
use anyhow::{bail, Result};
#[cfg(feature = "analyzer")]
use std::path::{Path, PathBuf};

#[cfg(feature = "analyzer")]
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[cfg(feature = "analyzer")]
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
#[cfg(feature = "analyzer")]
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
