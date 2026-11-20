//! Diagnostic export; the counterfactual is an upper bound, never a proof.
use crate::{analyze, load_rules, without_indirect_seeds, ThrowClass, SEEDS};
use anyhow::Result;
use std::path::{Path, PathBuf};
pub fn write(root: &Path, archives: &[PathBuf], out: &Path) -> Result<()> {
    let rules = load_rules(SEEDS)?;
    let (g, res) = analyze(archives, &rules, root)?;
    let shadow = crate::shadowed_symbols(root)?;
    let potential = without_indirect_seeds(&g, &rules, Some(root))?;
    let mut text=String::from("symbol\tthrow_class\tthrows\tpasses\topen_cause\tunmapped_only\tunmapped_producers\tinterpreted\twithout_indirect\n");
    for (name, row) in &res.table {
        let defs = &res.defs[name];
        let choose = defs
            .iter()
            .copied()
            .find(|&i| res.throw_reach.class[i] == ThrowClass::Open)
            .unwrap_or(defs[0]);
        let cause = if row.throw_class == ThrowClass::Open {
            res.throw_reach.open_cause(&g, choose)
        } else {
            "-".into()
        };
        let unmapped = defs
            .iter()
            .any(|&i| !res.throw_reach.own_unmapped[i].is_empty());
        let other_open = defs
            .iter()
            .any(|&i| res.throw_reach.open_witness[i].is_some());
        let producers: std::collections::BTreeSet<_> = defs
            .iter()
            .flat_map(|&i| res.throw_reach.own_unmapped[i].iter())
            .cloned()
            .collect();
        let interpreted = defs.iter().any(|&i| res.throw_reach.interpreted_reach[i]);
        let p = if shadow.contains(name) {
            ThrowClass::Open
        } else {
            potential.table[name].throw_class
        };
        text.push_str(&format!(
            "{name}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            row.throw_class,
            row.throws_name(),
            row.passes_text(),
            cause.replace(['\t', '\n'], " "),
            unmapped && !other_open,
            producers.into_iter().collect::<Vec<_>>().join(" | "),
            interpreted,
            p
        ));
    }
    std::fs::write(out, text)?;
    crate::record_census::write(root, &g, &res, archives, &out.with_extension("local.tsv"))?;
    Ok(())
}
