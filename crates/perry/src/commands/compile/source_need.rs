//! Retained function source: which stored `Function.prototype.toString` text
//! can any execution read? The classification ([`Need`]) runs over the final
//! HIR before codegen, on every compile. [`drop_unread_source`] then removes
//! the text of every owner no execution can stringify (classes (a) never a
//! value and (b) a value that reaches no string sink, under the strict rule),
//! before the object-cache key hashes the HIR.
//!
//! `PERRY_SOURCE_NEED_REPORT=<path>` is the compiler-only census: codegen
//! reports the byte range each remaining owner occupies in its module's
//! retained-source pools (decoded bytes); this joins both after the final link
//! and checks the pools against the linked `.perry_src` section. It bypasses
//! object caches so every module reports.
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
/// Diagnostic: keep every text [`drop_unread_source`] would remove (the
/// census soundness check runs the program with all source present).
pub(super) const RETAIN_KNOB: &str = "PERRY_RETAIN_ALL_SOURCE";

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

/// Classify every retained-source owner of the program. `library`: the
/// output's exports are reachable by unknown code.
pub(super) fn compute(ctx: &CompilationContext, library: bool) -> Need {
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
    Need::compute(
        &modules,
        &links,
        &entry,
        library,
        &ctx.patched_builtins.dynamic_code_reach,
    )
}

/// What [`drop_unread_source`] removed.
#[derive(Default)]
pub(super) struct Dropped {
    pub owners: usize,
    pub bytes: usize,
}

/// Remove the retained text of every owner whose strict class is (a) or (b):
/// no execution can read it. A function keeps its strictness bit (an empty
/// text, see `retained_source_pool`); a class has none. Owners the census did
/// not see keep their text.
pub(super) fn drop_unread_source(ctx: &mut CompilationContext, need: &Need) -> Dropped {
    let mut dropped = Dropped::default();
    if std::env::var_os(RETAIN_KNOB).is_some() {
        return dropped;
    }
    let unread = |ent: Ent| {
        need.entity(ent)
            .is_some_and(|e| need.class_of(e, 0) != Class3::MayStringify)
    };
    // `Need` indexes modules in `native_modules` order (a BTreeMap).
    for (mi, m) in ctx.native_modules.values_mut().enumerate() {
        for (fid, source) in m.closure_source_text.iter_mut() {
            if source.text.as_str().is_empty() || !unread(Ent::Func(mi, *fid)) {
                continue;
            }
            if let Some(empty) = source.text.slice(0..0) {
                dropped.owners += 1;
                dropped.bytes += source.text.as_str().len();
                source.text = empty;
            }
        }
        m.class_source_text.retain(|cid, text| {
            if !unread(Ent::Class(mi, *cid)) {
                return true;
            }
            dropped.owners += 1;
            dropped.bytes += text.as_str().len();
            false
        });
    }
    dropped
}

pub(super) fn start(need: Need, dropped: &Dropped, seconds: f64) -> Result<Option<Session>> {
    let Some(path) = std::env::var_os(KNOB) else {
        return Ok(None);
    };
    let path = PathBuf::from(path);
    eprintln!(
        "[source-need] {} owners over {} nodes / {} edges in {:.1}s; dropped {} owners, {} B of text",
        need.entities.len(),
        need.nodes,
        need.edges,
        seconds,
        dropped.owners,
        dropped.bytes
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
