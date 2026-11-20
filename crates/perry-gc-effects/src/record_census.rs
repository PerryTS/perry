//! Report-only FnInfo body census. Record identity comes from the carrier's
//! emitted symbol and ABI layout; every class still comes from machine code.
use crate::{Classification, Graph, ThrowClass};
use anyhow::Result;
use object::{Object, ObjectSection, ObjectSymbol, RelocationTarget, SymbolKind};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

fn body_targets(archives: &[PathBuf]) -> Result<BTreeSet<String>> {
    let mut targets = BTreeSet::new();
    let widths = [
        std::mem::size_of::<perry_abi::JsFunctionInfo>() as u64,
        std::mem::size_of::<perry_abi::NativeFunctionInfo>() as u64,
        (std::mem::size_of::<perry_abi::JsFunctionInfo>() + std::mem::size_of::<usize>()) as u64,
    ];
    for archive in archives {
        let bytes = std::fs::read(archive)?;
        let ar = object::read::archive::ArchiveFile::parse(bytes.as_slice())?;
        for member in ar.members() {
            let member = member?;
            let data = member.data(bytes.as_slice())?;
            let Ok(obj) = object::File::parse(data) else {
                continue;
            };
            for record in obj.symbols() {
                if record.kind() != SymbolKind::Data || !widths.contains(&record.size()) {
                    continue;
                }
                let Ok(name) = record.name() else {
                    continue;
                };
                let dem = rustc_demangle::try_demangle(name)
                    .map(|d| format!("{d:#}"))
                    .unwrap_or_else(|_| name.into());
                if !name.starts_with("perry_fn_info_") && !dem.ends_with("::INFO") {
                    continue;
                }
                let Some(section) = record.section_index() else {
                    continue;
                };
                let section = obj.section_by_index(section)?;
                for (offset, reloc) in section.relocations() {
                    // The primary code pointer is the first field of each
                    // emitted FnInfo / NativeExportInfo record.
                    if offset != record.address() - section.address() {
                        continue;
                    }
                    let RelocationTarget::Symbol(index) = reloc.target() else {
                        continue;
                    };
                    let target = obj.symbol_by_index(index)?;
                    if target.kind() == SymbolKind::Text {
                        targets.insert(target.name()?.into());
                    } else if let Some(sec) = target.section_index() {
                        let address = (target.address() as i128 + reloc.addend() as i128) as u64;
                        for function in obj.symbols().filter(|s| {
                            s.kind() == SymbolKind::Text
                                && s.section_index() == Some(sec)
                                && s.address() == address
                        }) {
                            targets.insert(function.name()?.into());
                        }
                    }
                }
            }
        }
    }
    Ok(targets)
}

pub fn write(
    root: &Path,
    g: &Graph,
    res: &Classification,
    archives: &[PathBuf],
    out: &Path,
) -> Result<()> {
    let targets = body_targets(archives)?;
    let mut nodes = BTreeMap::<String, Vec<usize>>::new();
    for (i, n) in g.nodes.iter().enumerate() {
        for name in &n.names {
            if targets.contains(name) {
                nodes.entry(name.clone()).or_default().push(i);
            }
        }
    }
    let mut text = String::from(
        "body\tclass\tprogram_value\tpasses\tdeclared\tany_throw_leaf\tprogram_value_raw\n",
    );
    let declarations = crate::throw_class::declarations(root)?;
    for (name, ns) in nodes {
        // Exported bodies already have rows; this census asks about local
        // built-in values, including the generated native-args bodies.
        if res.table.contains_key(&name) {
            continue;
        }
        let class = ns
            .iter()
            .map(|&i| res.throw_reach.class[i])
            .max()
            .unwrap_or(ThrowClass::Open);
        let program = ns.iter().any(|&i| res.throw_reach.program_reach[i]);
        let passes: BTreeSet<_> = ns
            .iter()
            .flat_map(|&i| res.throw_reach.passes[i].iter())
            .map(ToString::to_string)
            .collect();
        let declared = ns.iter().any(|&i| {
            declarations
                .iter()
                .any(|d| g.spellings(i).contains(&d.symbol.as_str()))
        });
        let label = g.dem.get(&name).unwrap_or(&name);
        text.push_str(&format!(
            "{label}\t{class}\t{program}\t{}\t{declared}\t{}\t{}\n",
            passes.into_iter().collect::<Vec<_>>().join(","),
            ns.iter().any(|&i| res.throw_reach.any_leaf_reach[i]),
            ns.iter().any(|&i| res.throw_reach.raw_program_reach[i])
        ));
    }
    std::fs::write(out, text)?;
    Ok(())
}
