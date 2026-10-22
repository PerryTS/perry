//! Driver for the compiler-only retained-source census
//! (`PERRY_SOURCE_NEED_REPORT=<path>`). The classification runs over the final
//! HIR before codegen; codegen reports the byte range each owner occupies in
//! its module's retained-source blob; this joins both after the final link and
//! checks the blob total against the linked `.perry_src` section. It changes
//! no output bytes and bypasses object caches so every module reports.
use std::collections::{BTreeMap, HashMap};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use object::{Object, ObjectSection};
use perry_codegen::codegen::source_need::flow::{
    category_mask, Category, Class3, Ent, Need, Reason,
};
use perry_codegen::codegen::source_need::{self, StoredKey};

use super::CompilationContext;

pub(super) const KNOB: &str = "PERRY_SOURCE_NEED_REPORT";

pub(super) struct Session {
    path: PathBuf,
    need: Need,
    _scope: source_need::Scope,
}

/// The configurations the summary reports: reason bits treated as harmless.
fn configs() -> [(&'static str, u64); 4] {
    let export = Reason::Export.bit();
    [
        // The brief's rule: every listed escape counts, exports included.
        ("strict", 0),
        // Closed executable: an export is only what its importers do with it.
        ("whole-program", export),
        // Bound for any refinement of untracked callees, heap and builtins.
        (
            "trust-unknown-sinks",
            export | category_mask(Category::Unknown),
        ),
        // Only a definite string conversion counts.
        (
            "known-sinks-only",
            export | category_mask(Category::Unknown) | category_mask(Category::Hard),
        ),
    ]
}

pub(super) fn start(ctx: &CompilationContext, library: bool) -> Result<Option<Session>> {
    let Some(path) = std::env::var_os(KNOB) else {
        return Ok(None);
    };
    let path = PathBuf::from(path);
    let links = super::reach_report::import_links(ctx);
    let modules: Vec<_> = ctx
        .native_modules
        .iter()
        .map(|(p, m)| (p.to_string_lossy().into_owned(), m))
        .collect();
    let entry = ctx
        .entry_canonical
        .as_deref()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let t = std::time::Instant::now();
    let need = Need::compute(
        &modules,
        &links,
        &entry,
        library,
        &ctx.patched_builtins.dynamic_code_reach,
    );
    eprintln!(
        "[source-need] {} owners over {} nodes / {} edges in {:.1}s",
        need.entities.len(),
        need.nodes,
        need.edges,
        t.elapsed().as_secs_f64()
    );
    for (what, n) in &need.notes {
        eprintln!("[source-need] note {what}: {n}");
    }
    for sample in &need.samples {
        eprintln!("[source-need]   {sample}");
    }
    Ok(Some(Session {
        path,
        need,
        _scope: source_need::install(),
    }))
}

/// The tiny-program backend retains no function source: say so in the report.
pub(super) fn tiny(binary: &Path) -> Result<()> {
    if let Some(path) = std::env::var_os(KNOB) {
        std::fs::write(
            PathBuf::from(path).with_extension("summary.tsv"),
            format!(
                "config\tclass\tbytes\nnone\ttiny-backend-no-retained-source\t0\n# {}\n",
                binary.display()
            ),
        )?;
    }
    Ok(())
}

fn perry_src_bytes(binary: &Path) -> Result<Option<u64>> {
    let bytes = std::fs::read(binary)?;
    let object = object::File::parse(&*bytes)?;
    let total: u64 = object
        .sections()
        .filter(|s| {
            matches!(s.name(), Ok(".perry_src" | "__perry_src"))
                || s.name().is_ok_and(|n| n.ends_with("$perry_src"))
        })
        .map(|s| s.size())
        .sum();
    Ok((total > 0).then_some(total))
}

impl Session {
    pub fn finish(&self, binary: &Path) -> Result<()> {
        let stored = source_need::take_stored();
        let need = &self.need;
        let mut by_name: HashMap<&str, usize> = HashMap::new();
        let mut duplicate_names = 0usize;
        for (mi, name) in need.module_names.iter().enumerate() {
            if by_name.insert(name.as_str(), mi).is_some() {
                duplicate_names += 1;
            }
        }
        let configs = configs();
        let mut out = BufWriter::new(std::fs::File::create(&self.path)?);
        writeln!(
            out,
            "module\tkind\tlabel\tid\tsymbol\toffset\town_bytes\thash\tclass_strict\tclass_whole_program\tclass_trust_unknown\tclass_known_only\tfirst_reason\tfirst_detail\tcategory\treasons"
        )?;
        let mut mods = BufWriter::new(std::fs::File::create(
            self.path.with_extension("modules.tsv"),
        )?);
        writeln!(mods, "prefix\tmodule\tblob_bytes\tentries")?;
        // config -> [a, b, c, uncovered]
        let mut totals: BTreeMap<&str, Vec<u64>> = BTreeMap::new();
        let mut counts: BTreeMap<(&str, &str), u64> = BTreeMap::new();
        let mut unclassified = 0u64;
        let mut unjoined_modules = 0usize;
        let mut blob_total = 0u64;
        let mut physical_total = 0u64;
        for (prefix, m) in &stored {
            blob_total += m.blob_len as u64;
            physical_total += m.physical_len as u64;
            writeln!(
                mods,
                "{}\t{}\t{}\t{}",
                cell(prefix),
                cell(&m.module_name),
                m.blob_len,
                m.entries.len()
            )?;
            let mi = by_name.get(m.module_name.as_str()).copied();
            if mi.is_none() {
                unjoined_modules += 1;
            }
            let module = mi.map_or(m.module_name.as_str(), |mi| need.module_paths[mi].as_str());
            let mut ranges: Vec<Vec<(usize, usize, u8)>> = vec![Vec::new(); configs.len()];
            for entry in &m.entries {
                let ent = mi.map(|mi| match entry.key {
                    StoredKey::Func(f) => Ent::Func(mi, f),
                    StoredKey::Class(c) => Ent::Class(mi, c),
                });
                let e = ent.and_then(|ent| need.entity(ent));
                let classes: Vec<Class3> = configs
                    .iter()
                    .map(|(_, ignored)| {
                        e.map_or(Class3::MayStringify, |e| need.class_of(e, *ignored))
                    })
                    .collect();
                if e.is_none() {
                    unclassified += 1;
                }
                for (i, c) in classes.iter().enumerate() {
                    ranges[i].push((entry.offset, entry.len, *c as u8));
                    *counts.entry((configs[i].0, c.letter())).or_default() += 1;
                }
                let (id, kind_default) = match entry.key {
                    StoredKey::Func(f) => (f, "function"),
                    StoredKey::Class(c) => (c, "class"),
                };
                let (kind, label) = e.map_or((kind_default, "-".to_string()), |e| {
                    (need.entities[e].kind, need.entities[e].label.clone())
                });
                let (first, detail) = match e.and_then(|e| need.witness[e].clone()) {
                    Some((r, d)) => (r.name(), d),
                    None if e.is_none() => ("unclassified", String::new()),
                    None => ("-", String::new()),
                };
                let category = e.map_or("H", |e| {
                    let mask = need.mask[e];
                    if mask & category_mask(Category::Known) != 0 {
                        "K"
                    } else if mask & category_mask(Category::Hard) != 0 {
                        "H"
                    } else if mask != 0 {
                        "U"
                    } else {
                        "-"
                    }
                });
                writeln!(
                    out,
                    "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:016x}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                    cell(module),
                    kind,
                    cell(&label),
                    id,
                    cell(&entry.symbol),
                    entry.offset,
                    entry.len,
                    entry.hash,
                    classes[0].letter(),
                    classes[1].letter(),
                    classes[2].letter(),
                    classes[3].letter(),
                    first,
                    cell(&detail),
                    category,
                    e.map_or_else(String::new, |e| need.reasons(e).join(","))
                )?;
            }
            for (i, (name, _)) in configs.iter().enumerate() {
                let u = source_need::union_bytes(m.blob_len, &ranges[i], 3);
                let t = totals.entry(name).or_insert_with(|| vec![0; 4]);
                for (k, v) in u.iter().enumerate() {
                    t[k] += v;
                }
            }
        }
        out.flush()?;
        mods.flush()?;
        let section = perry_src_bytes(binary)?;
        let mut summary = String::from("config\tclass\tbytes\tentries\n");
        for (name, t) in &totals {
            for (k, letter) in ["a", "b", "c", "uncovered"].iter().enumerate() {
                let n = counts.get(&(*name, *letter)).copied().unwrap_or(0);
                summary.push_str(&format!("{name}\t{letter}\t{}\t{n}\n", t[k]));
            }
        }
        summary.push_str(&format!(
            "all\tblob_total\t{blob_total}\t{}\n",
            stored.len()
        ));
        summary.push_str(&format!(
            "all\tpools_physical\t{physical_total}\t{}\n",
            stored.len()
        ));
        summary.push_str(&format!(
            "all\tperry_src_section\t{}\t0\n",
            section.unwrap_or(0)
        ));
        summary.push_str(&format!("all\tunclassified_entries\t0\t{unclassified}\n"));
        summary.push_str(&format!("all\tunjoined_modules\t0\t{unjoined_modules}\n"));
        summary.push_str(&format!(
            "all\tduplicate_module_names\t0\t{duplicate_names}\n"
        ));
        std::fs::write(self.path.with_extension("summary.tsv"), &summary)?;
        let strict = &totals.get("strict").cloned().unwrap_or_else(|| vec![0; 4]);
        eprintln!(
            "[source-need] stored {blob_total} B decoded, {physical_total} B physical, in {} modules (.perry_src {}); strict a={} b={} c={}; unclassified entries {unclassified}; report {}",
            stored.len(),
            section.map_or("absent".to_string(), |s| s.to_string()),
            strict[0],
            strict[1],
            strict[2],
            self.path.display()
        );
        // Self-checks: every stored byte is covered by an owner, and the pools
        // this census saw fit the section the linker kept.
        if strict[3] != 0 {
            bail!(
                "source-need: {} stored bytes have no recorded owner",
                strict[3]
            );
        }
        if let Some(s) = section {
            if s < physical_total {
                bail!("source-need: recorded pools ({physical_total} B) exceed the linked .perry_src ({s} B)");
            }
        }
        Ok(())
    }
}

fn cell(s: &str) -> String {
    s.replace(['\t', '\n', '\r'], " ")
}
