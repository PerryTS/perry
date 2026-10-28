//! Compile-time fallback evidence, kept outside the executable.
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use object::{Object, ObjectSection, ObjectSymbol};
use perry_codegen::fallback_diag::{Facts, Receiver};
use sha2::{Digest, Sha256};

pub(super) fn facts_for(pf: &perry_wpfacts::ProgramFacts, path: &str) -> Arc<Facts> {
    if !perry_codegen::fallback_diag::enabled() {
        return Arc::default();
    }
    let analysis_refusal = pf.stats.aborted.then(|| {
        format!(
            "whole-program analysis aborted before publishing binding proofs (work={}, pops={})",
            pf.stats.work, pf.stats.pops
        )
    });
    let Some(mi) = pf.module_paths.iter().position(|p| p == path) else {
        return Arc::new(Facts {
            module_path: path.into(),
            analysis_refusal,
            ..Default::default()
        });
    };
    let binding = |b: &perry_wpfacts::BindingFacts| Receiver {
        classes: b.class_ids.clone(),
        primitive_mask: b.prims,
        other: b.other,
        top: b.top,
        cause: b.top_cause.clone(),
    };
    let mut out = Facts {
        module_path: path.into(),
        analysis_refusal,
        ..Default::default()
    };
    if let Some(b) = pf.bindings.get(mi) {
        out.locals = b.locals.iter().map(|(id, b)| (*id, binding(b))).collect();
        out.this_of = b.this_of.iter().map(|(id, b)| (*id, binding(b))).collect();
    }
    for t in &pf.templates {
        if let Some(id) = t.class_id {
            if let Some(why) = t.hard.as_ref().or(t.chain.as_ref()) {
                out.class_refusals.insert(id, why.clone());
            }
        }
    }
    let mut ambiguous = BTreeSet::new();
    for site in pf.sites.iter().filter(|s| s.module as usize == mi) {
        let reason = match &site.verdict {
            perry_wpfacts::Verdict::No(why) => why.clone(),
            perry_wpfacts::Verdict::StringLength => {
                "native primitive string-length proof recorded; string tag admission applies".into()
            }
            perry_wpfacts::Verdict::T0Mono => {
                "whole-program monomorphic proof recorded; generic guard/path emitted".into()
            }
            perry_wpfacts::Verdict::T0Set => {
                "whole-program family-set proof recorded; generic guard/path emitted".into()
            }
            perry_wpfacts::Verdict::E0 => {
                "proof behind the entry guard recorded; generic guard/path emitted".into()
            }
        };
        let key = (site.byte_offset, site.key.clone(), site.is_call);
        if ambiguous.contains(&key) {
            continue;
        }
        let recv = Receiver {
            top: site.top,
            cause: site.top.then(|| reason.clone()),
            ..Default::default()
        };
        // Duplicate source coordinates must not silently select one verdict.
        if let Some((_, old)) = out.sites.get(&key) {
            if old != &reason {
                ambiguous.insert(key.clone());
                out.sites.remove(&key);
                continue;
            }
        }
        out.sites.insert(key, (recv, reason));
    }
    Arc::new(out)
}

pub(super) fn emit(binary: &Path) -> Result<()> {
    if !perry_codegen::fallback_diag::enabled() {
        return Ok(());
    }
    let bytes = std::fs::read(binary)?;
    let file = object::File::parse(bytes.as_slice())?;
    let text = file
        .section_by_name(".text")
        .or_else(|| file.section_by_name("__text"))
        .context("fallback diagnostic image has no native text section")?;
    let text = text.data()?;
    let text_sha256: String = Sha256::digest(text)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let mut symbols = BTreeMap::new();
    for symbol in file.symbols() {
        if symbol.is_definition() && symbol.kind() == object::SymbolKind::Text {
            if let Ok(name) = symbol.name() {
                symbols.insert(
                    name.to_owned(),
                    serde_json::json!({"address": symbol.address(), "size": symbol.size()}),
                );
            }
        }
    }
    let mut sections = Vec::new();
    for section in file.sections() {
        if matches!(section.name(), Ok(".pseudo_probe" | "__pseudo_probe")) {
            let data = section.data()?;
            let hex: String = data.iter().map(|b| format!("{b:02x}")).collect();
            sections.push(serde_json::json!({"data_hex": hex}));
        }
    }
    let records = perry_codegen::fallback_diag::take_records();
    if !records.is_empty() && sections.is_empty() {
        bail!("generic fallbacks were recorded but the linked image has no LLVM pseudo-probes");
    }
    let sidecar = binary.as_os_str().to_os_string();
    let mut sidecar = sidecar;
    sidecar.push(".fallback.json");
    let sidecar = std::path::PathBuf::from(sidecar);
    let mut temp = sidecar.as_os_str().to_os_string();
    temp.push(format!(".{}.tmp", std::process::id()));
    let temp = std::path::PathBuf::from(temp);
    let value = serde_json::json!({"schema": 1, "mapping": "llvm22-pseudo-probes",
        "text_sha256": text_sha256, "text_size": text.len(),
        "records": records, "symbols": symbols, "probe_sections": sections});
    std::fs::write(&temp, serde_json::to_vec(&value)?)?;
    // Probe tables contain labels only, never target runtime state. Remove
    // them even when PERRY_KEEP_SYMBOLS retains the ordinary symbol table.
    if !sections.is_empty() {
        let mut removed = false;
        for tool in ["llvm-objcopy", "objcopy"] {
            let mut command = std::process::Command::new(tool);
            if std::env::var_os("PERRY_DEBUG_SYMBOLS").is_none() {
                command.arg("--strip-debug");
            }
            match command
                .args([
                    "--remove-section=.pseudo_probe",
                    "--remove-section=.pseudo_probe_desc",
                ])
                .arg(binary)
                .output()
            {
                Ok(out) if out.status.success() => {
                    removed = true;
                    break;
                }
                Ok(_) => continue,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e).context("removing fallback probe sections"),
            }
        }
        if !removed {
            bail!("fallback diagnostics require llvm-objcopy to remove probe metadata");
        }
    }
    std::fs::rename(temp, sidecar)?;
    Ok(())
}
