//! Driver for the compiler-only reachability census. The knob bypasses object
//! caches so the generated-reference contract is checked on every module.
//! Its final-symbol join runs before stripping and changes no output bytes.
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use object::{Object, ObjectSection, ObjectSymbol, SymbolKind};
use perry_codegen::codegen::reach_report::Census;

use super::{resolve::resolve_import_with_context, CompilationContext};

pub(super) struct Session {
    path: PathBuf,
    pub census: Arc<Census>,
    _scope: perry_codegen::codegen::reach_report::ProgramScope,
}

pub(super) fn start(ctx: &CompilationContext, library: bool) -> Result<Option<Session>> {
    let Some(path) = std::env::var_os("PERRY_REACH_REPORT") else {
        return Ok(None);
    };
    let report_path = PathBuf::from(&path);
    for destination in destinations(&report_path) {
        for input in ctx.native_modules.keys() {
            if paths_collide(&destination, input) {
                bail!("reach report path collides with input {}", input.display());
            }
        }
    }
    let links = import_links(ctx);
    let modules: Vec<_> = ctx
        .native_modules
        .iter()
        .map(|(p, m)| (p.to_string_lossy().into_owned(), m))
        .collect();
    let entry = ctx
        .entry_canonical
        .as_deref()
        .context("reach census requires entry path")?;
    let census = Arc::new(Census::compute(
        &modules,
        &links,
        &entry.to_string_lossy(),
        library,
        &ctx.patched_builtins.dynamic_code_reach,
    ));
    finish_start(path, census)
}

/// Every module's import/re-export specifier, resolved to the module path the
/// compile linked (shared with the retained-source census).
pub(super) fn import_links(ctx: &CompilationContext) -> BTreeMap<(String, String), String> {
    let mut links = BTreeMap::new();
    for (path, m) in &ctx.native_modules {
        let sources =
            m.imports
                .iter()
                .map(|i| i.source.as_str())
                .chain(m.exports.iter().filter_map(|e| match e {
                    perry_hir::Export::ReExport { source, .. }
                    | perry_hir::Export::ExportAll { source }
                    | perry_hir::Export::NamespaceReExport { source, .. } => Some(source.as_str()),
                    _ => None,
                }));
        for source in sources {
            if let Some((resolved, _)) = resolve_import_with_context(source, path, ctx) {
                links.insert(
                    (path.to_string_lossy().into_owned(), source.to_owned()),
                    resolved.to_string_lossy().into_owned(),
                );
            }
        }
    }
    links
}

fn finish_start(path: std::ffi::OsString, census: Arc<Census>) -> Result<Option<Session>> {
    let path = PathBuf::from(path);
    let mut hir = BufWriter::new(std::fs::File::create(path.with_extension("hir.tsv"))?);
    writeln!(
        hir,
        "symbol\tmodule\tlabel\treachable\treason\tonly_conservative"
    )?;
    for (node, v) in census.nodes.iter().zip(&census.verdicts) {
        writeln!(
            hir,
            "{}\t{}\t{}\t{}\t{}\t{}",
            node.symbol.as_deref().unwrap_or("-"),
            cell(&node.module),
            cell(&node.label),
            yn(v.reachable),
            cell(&v.reason),
            yn(v.only_conservative)
        )?;
    }
    hir.flush()?;
    eprintln!(
        "[reach] {} semantic nodes; {} unreachable; {} only conservative",
        census.nodes.len(),
        census.verdicts.iter().filter(|v| !v.reachable).count(),
        census
            .verdicts
            .iter()
            .filter(|v| v.only_conservative)
            .count()
    );
    let scope = perry_codegen::codegen::reach_report::install(Arc::clone(&census))?;
    Ok(Some(Session {
        path,
        census,
        _scope: scope,
    }))
}

impl Session {
    pub fn check(&self) -> Result<()> {
        let mut out = BufWriter::new(std::fs::File::create(
            self.path.with_extension("generated.tsv"),
        )?);
        writeln!(out, "symbol\tmodule\treferences")?;
        for f in self.census.machines() {
            writeln!(
                out,
                "{}\t{}\t{}",
                cell(&f.symbol),
                cell(&f.module),
                f.references.join(",")
            )?;
        }
        out.flush()?;
        self.census.check()?;
        eprintln!("[reach] generated semantic reference contract passed");
        Ok(())
    }

    pub fn finish(&self, binary: &Path) -> Result<()> {
        for destination in destinations(&self.path) {
            if paths_collide(&destination, binary) {
                bail!(
                    "reach report path collides with executable {}",
                    binary.display()
                );
            }
        }
        let precise = self.census.precise_nodes();
        let verdict = |s: &str| {
            self.census.node_for_symbol(s).map_or((true, false), |n| {
                (
                    self.census.verdicts[n].reachable,
                    self.census.verdicts[n].reachable && !precise[n],
                )
            })
        };
        let bytes = std::fs::read(binary)?;
        let object = object::File::parse(&*bytes)?;
        let text: Vec<_> = object
            .sections()
            .filter(|s| matches!(s.name(), Ok(".text" | "__text")))
            .collect();
        let total: u64 = text.iter().map(|s| s.size()).sum();
        if total == 0 {
            bail!("reach census: final link has no .text/__text");
        }
        let sections: BTreeSet<_> = text.iter().map(|s| s.index().0).collect();
        let mut by_address: BTreeMap<(usize, u64), Vec<(String, u64)>> = BTreeMap::new();
        for s in object.symbols() {
            if s.kind() != SymbolKind::Text || !s.is_definition() {
                continue;
            }
            let Some(section) = s.section_index() else {
                continue;
            };
            if !sections.contains(&section.0) {
                continue;
            }
            let Ok(name) = s.name() else {
                continue;
            };
            let name = if object.format() == object::BinaryFormat::MachO {
                name.strip_prefix('_').unwrap_or(name)
            } else {
                name
            };
            by_address
                .entry((section.0, s.address()))
                .or_default()
                .push((name.to_owned(), s.size()));
        }
        if by_address.is_empty() {
            bail!("reach census: final link has no text symbols (report must precede strip)");
        }
        let machines = self.census.machines();
        let machine_modules: BTreeMap<_, _> = machines
            .iter()
            .map(|m| (m.symbol.as_str(), m.module.as_str()))
            .collect();
        let addresses: Vec<_> = by_address.keys().copied().collect();
        let mut rows = BTreeMap::new();
        for (i, key) in addresses.iter().enumerate() {
            let mut aliases = by_address[key].clone();
            aliases.sort();
            // Alias bytes belong to the same executable range. A reachable
            // alias keeps the whole range live; never count a dead alias twice.
            let section = text.iter().find(|s| s.index().0 == key.0).unwrap();
            let next = addresses
                .get(i + 1)
                .filter(|k| k.0 == key.0)
                .map_or(section.address() + section.size(), |k| k.1);
            let declared = aliases.iter().map(|(_, sz)| *sz).max().unwrap();
            let size = if declared == 0 && object.format() == object::BinaryFormat::MachO {
                next - key.1
            } else {
                declared.min(next - key.1)
            };
            // Choose a semantic symbol for attribution when an alias has one.
            aliases.sort_by_key(|(s, _)| {
                let (live, coarse) = verdict(s);
                (
                    if live && !coarse {
                        0
                    } else if live {
                        1
                    } else {
                        2
                    },
                    self.census.node_for_symbol(s).is_none(),
                    s.clone(),
                )
            });
            let representative = aliases[0].0.clone();
            for (name, _) in aliases {
                let (live, coarse) = verdict(&name);
                let node = self.census.node_for_symbol(&name);
                let module = node
                    .map(|n| self.census.nodes[n].module.as_str())
                    .or_else(|| machine_modules.get(name.as_str()).copied())
                    .unwrap_or("<native-or-unattributed>");
                let reason = node
                    .map(|n| self.census.verdicts[n].reason.as_str())
                    .unwrap_or("generated/native-support-conservative");
                rows.insert(
                    name.clone(),
                    (
                        module.to_owned(),
                        live,
                        reason.to_owned(),
                        if name == representative { size } else { 0 },
                        coarse,
                        "linked",
                    ),
                );
            }
        }
        for node in &self.census.nodes {
            if let Some(symbol) = &node.symbol {
                let n = self.census.node_for_symbol(symbol).unwrap();
                let v = &self.census.verdicts[n];
                rows.entry(symbol.clone()).or_insert((
                    node.module.clone(),
                    v.reachable,
                    v.reason.clone(),
                    0,
                    v.reachable && !precise[n],
                    "not-in-final-link",
                ));
            }
        }
        let mut out = BufWriter::new(std::fs::File::create(&self.path)?);
        writeln!(
            out,
            "symbol\tmodule\treachable\treason\ttext_bytes\tonly_conservative\tlink_status"
        )?;
        let mut dead = 0;
        let mut coarse = 0;
        let mut attributed = 0;
        for (symbol, (module, live, reason, size, c, status)) in &rows {
            writeln!(
                out,
                "{}\t{}\t{}\t{}\t{}\t{}\t{}",
                cell(symbol),
                cell(module),
                yn(*live),
                cell(reason),
                size,
                yn(*c),
                status
            )?;
            attributed += size;
            if !live {
                dead += size;
            }
            if *c {
                coarse += size;
            }
        }
        out.flush()?;
        let summary = format!("metric\tbytes\nfile\t{}\ntext\t{total}\nunreachable_text\t{dead}\nonly_conservative_text\t{coarse}\nattributed_text\t{attributed}\nunattributed_padding\t{}\n",bytes.len(),total.saturating_sub(attributed));
        std::fs::write(self.path.with_extension("summary.tsv"), summary)?;
        eprintln!("[reach] final .text {total}; unreachable {dead} ({:.3}%); conservative-only {coarse}; report {}",100.0*dead as f64/total as f64,self.path.display());
        Ok(())
    }
}

fn yn(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "no"
    }
}
fn cell(s: &str) -> String {
    s.replace(['\t', '\n', '\r'], " ")
}

fn destinations(path: &Path) -> [PathBuf; 4] {
    [
        path.to_owned(),
        path.with_extension("hir.tsv"),
        path.with_extension("generated.tsv"),
        path.with_extension("summary.tsv"),
    ]
}
fn paths_collide(a: &Path, b: &Path) -> bool {
    a == b
        || a.canonicalize()
            .ok()
            .zip(b.canonicalize().ok())
            .is_some_and(|(a, b)| a == b)
}

#[cfg(test)]
mod tests {
    #[test]
    fn report_paths_detect_executable_and_input_collisions() {
        use std::path::Path;
        assert!(super::paths_collide(Path::new("x"), Path::new("x")));
        assert!(!super::paths_collide(Path::new("x"), Path::new("y")));
        assert_eq!(
            super::destinations(Path::new("x.tsv"))[1],
            Path::new("x.hir.tsv")
        );
    }
}
