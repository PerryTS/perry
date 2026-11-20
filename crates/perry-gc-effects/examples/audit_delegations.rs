//! Audit even inactive delegated seeds against a release archive's real symbols.
//! cargo run --release -p perry-gc-effects --example audit_delegations -- ARCHIVE...
//! Use --rules FILE for a before/after comparison; stdout is a JSON audit table.
use anyhow::{bail, Result};
use clap::Parser;
use perry_gc_effects::{build_graph, classify, load_rules, workspace_root, Rule, SEEDS};
use std::{collections::BTreeSet, path::PathBuf};

#[derive(Parser)]
struct Args {
    #[arg(long)]
    rules: Option<PathBuf>,
    #[arg(required = true)]
    archives: Vec<PathBuf>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let text = args.rules.map(std::fs::read_to_string).transpose()?;
    let rules = load_rules(text.as_deref().unwrap_or(SEEDS))?;
    let g = build_graph(&args.archives)?;
    // Use the very same spellings (including LLVM suffix normalization) and
    // matcher as classification, rather than grepping raw nm output.
    let spellings = (0..g.nodes.len())
        .map(|i| g.spellings(i))
        .collect::<Vec<_>>();
    let matches = |r: &Rule| {
        spellings
            .iter()
            .flatten()
            .filter(|s| r.matches(s))
            .copied()
            .collect::<BTreeSet<_>>()
    };
    let classification = classify(&g, &rules, Some(&workspace_root()));
    let mut missing = Vec::new();
    let rows = rules
        .iter()
        .filter_map(|r| {
            let target = r.targets.as_ref()?;
            let targets = matches(target);
            if targets.is_empty() {
                missing.push(r.line);
            }
            let (callers, used) = if r.kind == "idiom" {
                let callers = g
                    .nodes
                    .iter()
                    .enumerate()
                    .filter(|(_, n)| n.idioms.contains(&r.pattern))
                    .map(|(i, _)| g.label(i))
                    .collect::<BTreeSet<_>>();
                let used = Some(!callers.is_empty());
                (callers, used)
            } else {
                let used = classification
                    .as_ref()
                    .ok()
                    .map(|res| !res.stats.unused_exemptions.contains(&r.description()));
                (matches(r).into_iter().map(str::to_owned).collect(), used)
            };
            Some(serde_json::json!({
                "line": r.line, "kind": r.kind, "pattern": r.pattern,
                "target_pattern": target.pattern, "callers": callers,
                "targets": targets, "used": used,
            }))
        })
        .collect::<Vec<_>>();
    serde_json::to_writer_pretty(std::io::stdout().lock(), &rows)?;
    eprintln!(
        "audited {} delegations; {} target patterns match no functions",
        rows.len(),
        missing.len()
    );
    if !missing.is_empty() {
        bail!("delegations with no target functions: seeds.txt lines {missing:?}");
    }
    // A successful target audit must also satisfy the ordinary classifier.
    classification?.validate()?;
    Ok(())
}
