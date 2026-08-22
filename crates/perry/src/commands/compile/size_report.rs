//! `--report-size`: attribute the final linked binary's size to the crates
//! that produced it.
//!
//! Same core technique as `cargo-bsize` (see `../../../cargo-bsize/src/symbols.rs`
//! for the reference implementation this borrows from), applied directly to
//! the binary Perry actually ships instead of a `cargo build` rebuild:
//! `object` reads the symbol table out of the already-linked executable,
//! `rustc-demangle` recovers the Rust path, and the path's first `::`
//! segment is the attributed crate. ELF carries a real per-symbol size;
//! Mach-O does not, so its sizes come from sorting symbols by address
//! within a section and taking the distance to the next one (an upper
//! bound — it also counts any anonymous padding between them).
//!
//! Deliberately does not attempt cargo-bsize's monomorphization/trait/LTO
//! provenance analysis: that requires driving a `cargo build` with
//! instrumentation flags, which Perry's own two-stage build (static-archive
//! compile, then a raw `cc`/`ld` link of those archives plus LLVM-emitted
//! object code) has no hook for. This is a symbol-table-only view of
//! whatever made it into the final link.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use object::{Object, ObjectSection, ObjectSymbol, SectionKind, SymbolKind};

use crate::OutputFormat;

const REPORT_TOP_CRATES: usize = 20;
const REPORT_TOP_SYMBOLS: usize = 30;

#[derive(Default)]
struct CrateTotals {
    code_bytes: u64,
    data_bytes: u64,
    symbol_count: usize,
}

struct RankedSymbol {
    demangled: String,
    crate_name: String,
    size: u64,
    exact: bool,
}

struct SizeReport {
    file_bytes: u64,
    code_section_bytes: u64,
    data_section_bytes: u64,
    code_attributed_bytes: u64,
    data_attributed_bytes: u64,
    symbol_count: usize,
    by_crate: BTreeMap<String, CrateTotals>,
    largest: Vec<RankedSymbol>,
}

/// Write `<exe_path>.size-report.md`. Best-effort and silent unless
/// `--report-size` was actually passed: a read/parse failure here must
/// never fail the build, the report is a diagnostic extra, not a build
/// product.
pub(super) fn emit_size_report(format: OutputFormat, exe_path: &Path, requested: bool) {
    if !requested {
        return;
    }
    let report = match build_report(exe_path) {
        Ok(report) => report,
        Err(e) => {
            if let OutputFormat::Text = format {
                eprintln!("warning: failed to build size report: {e}");
            }
            return;
        }
    };
    let report_path = report_path_for(exe_path);
    let markdown = render_markdown(exe_path, &report);
    if let Err(e) = fs::write(&report_path, markdown) {
        if let OutputFormat::Text = format {
            eprintln!("warning: failed to write size report: {e}");
        }
        return;
    }
    if let OutputFormat::Text = format {
        let unattributed = (report.code_section_bytes + report.data_section_bytes)
            .saturating_sub(report.code_attributed_bytes + report.data_attributed_bytes);
        println!("Wrote size report: {}", report_path.display());
        println!(
            "  {} attributed across {} symbols, {} unattributed (inlined/no-symbol bytes)",
            human_bytes(report.code_attributed_bytes + report.data_attributed_bytes),
            report.symbol_count,
            human_bytes(unattributed),
        );
    }
}

fn report_path_for(exe_path: &Path) -> PathBuf {
    let mut s = exe_path.as_os_str().to_owned();
    s.push(".size-report.md");
    PathBuf::from(s)
}

fn build_report(exe_path: &Path) -> anyhow::Result<SizeReport> {
    let data = fs::read(exe_path)?;
    let file = object::File::parse(&*data)?;
    let file_bytes = data.len() as u64;

    let mut code_section_bytes = 0u64;
    let mut data_section_bytes = 0u64;
    let mut code_syms: Vec<(u64, u64, &str)> = Vec::new(); // (section_index, address, name)
    let mut data_syms: Vec<(u64, u64, &str)> = Vec::new();
    let mut sizes: BTreeMap<(u64, u64), u64> = BTreeMap::new(); // (section_index, address) -> real size

    for section in file.sections() {
        let size = section.size();
        match section.kind() {
            SectionKind::Text => code_section_bytes += size,
            SectionKind::Data | SectionKind::ReadOnlyData | SectionKind::UninitializedData => {
                data_section_bytes += size
            }
            _ => {}
        }
    }

    for symbol in file.symbols() {
        let (Some(name), Ok(section_index)) = (
            symbol.name().ok().filter(|n| !n.is_empty()),
            symbol.section().index().ok_or(()),
        ) else {
            continue;
        };
        let kind = symbol.kind();
        let bucket = match kind {
            SymbolKind::Text => &mut code_syms,
            SymbolKind::Data => &mut data_syms,
            _ => continue,
        };
        bucket.push((section_index.0 as u64, symbol.address(), name));
        if symbol.size() != 0 {
            sizes.insert((section_index.0 as u64, symbol.address()), symbol.size());
        }
    }

    // Mach-O symbols carry no size: sort by (section, address) and take the
    // distance to the next symbol in the same section as an upper-bound size
    // for any address that didn't already get a real ELF size above.
    let section_end: BTreeMap<u64, u64> = file
        .sections()
        .map(|s| (s.index().0 as u64, s.address() + s.size()))
        .collect();
    let mut ranked = Vec::new();
    for syms in [&mut code_syms, &mut data_syms] {
        syms.sort_by_key(|&(section, address, _)| (section, address));
        for i in 0..syms.len() {
            let (section, address, name) = syms[i];
            let exact = sizes.contains_key(&(section, address));
            let size = sizes.get(&(section, address)).copied().unwrap_or_else(|| {
                let next_addr = syms
                    .get(i + 1)
                    .filter(|&&(next_section, ..)| next_section == section)
                    .map(|&(_, addr, _)| addr)
                    .or_else(|| section_end.get(&section).copied())
                    .unwrap_or(address);
                next_addr.saturating_sub(address)
            });
            if size == 0 {
                continue;
            }
            ranked.push((section, name, size, exact));
        }
    }

    let mut by_crate: BTreeMap<String, CrateTotals> = BTreeMap::new();
    let mut largest: Vec<RankedSymbol> = Vec::new();
    let mut code_attributed_bytes = 0u64;
    let mut data_attributed_bytes = 0u64;
    let code_section_indices: std::collections::HashSet<u64> = file
        .sections()
        .filter(|s| s.kind() == SectionKind::Text)
        .map(|s| s.index().0 as u64)
        .collect();

    for (section, name, size, exact) in ranked {
        let demangled = demangle(name);
        let crate_name = crate_of(&demangled);
        let totals = by_crate.entry(crate_name.clone()).or_default();
        totals.symbol_count += 1;
        if code_section_indices.contains(&section) {
            totals.code_bytes += size;
            code_attributed_bytes += size;
        } else {
            totals.data_bytes += size;
            data_attributed_bytes += size;
        }
        largest.push(RankedSymbol {
            demangled,
            crate_name,
            size,
            exact,
        });
    }

    largest.sort_by(|a, b| b.size.cmp(&a.size));
    largest.truncate(REPORT_TOP_SYMBOLS);
    let symbol_count: usize = by_crate.values().map(|t| t.symbol_count).sum();

    Ok(SizeReport {
        file_bytes,
        code_section_bytes,
        data_section_bytes,
        code_attributed_bytes,
        data_attributed_bytes,
        symbol_count,
        by_crate,
        largest,
    })
}

/// Demangle a Rust symbol name. `rustc_demangle` returns non-Rust input
/// unchanged — the normal case for libc/system symbols — and `crate_of`
/// below buckets those as `native/other`.
fn demangle(name: &str) -> String {
    rustc_demangle::demangle(name).to_string()
}

/// The crate a demangled Rust path belongs to — its first `::`-delimited
/// segment, with the compiler's disambiguating hash suffix (`::h<16 hex>`)
/// dropped. Non-Rust names (no `::`, or containing characters a Rust path
/// segment can't) bucket as `native/other`.
fn crate_of(demangled: &str) -> String {
    // `<Type as Trait>::method` / `<Type>::method` associated-fn forms put
    // the crate name one level in; strip the leading `<` before reading it.
    let trimmed = demangled.trim_start_matches('<');
    let ident_end = trimmed
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(trimmed.len());
    let candidate = &trimmed[..ident_end];
    let starts_like_ident = candidate
        .chars()
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_');
    // v0 mangling embeds the crate's disambiguating hash right after its
    // name as `crate_name[16 hex digits]`; legacy mangling has none and
    // goes straight to `::`. Either is proof `candidate` is a crate name
    // rather than an identifier that merely starts the same way a mangled
    // name would (a bare C symbol, `main`, `<str>`-as-printed generic self
    // types with no path after them, …).
    let rest = &trimmed[ident_end..];
    if starts_like_ident && (rest.starts_with('[') || rest.starts_with("::")) {
        candidate.to_string()
    } else {
        "native/other".to_string()
    }
}

fn render_markdown(exe_path: &Path, report: &SizeReport) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "# Size report: {}\n\n",
        exe_path.file_name().map_or_else(
            || exe_path.display().to_string(),
            |n| n.to_string_lossy().into_owned()
        )
    ));
    out.push_str(&format!(
        "- Total file size: {}\n",
        human_bytes(report.file_bytes)
    ));
    out.push_str(&format!(
        "- Code sections: {} ({} attributed to {} symbols)\n",
        human_bytes(report.code_section_bytes),
        human_bytes(report.code_attributed_bytes),
        report.symbol_count,
    ));
    out.push_str(&format!(
        "- Data sections: {} ({} attributed)\n\n",
        human_bytes(report.data_section_bytes),
        human_bytes(report.data_attributed_bytes),
    ));
    out.push_str(
        "Built from the linked binary's own symbol table (`object` + `rustc-demangle`), \
         the same technique [cargo-bsize](https://github.com/boshen/cargo-bsize) uses on a \
         `cargo build` rebuild — applied here directly to what Perry actually links, since \
         Perry's static-archive-then-`cc`/`ld` build has no `cargo build` for cargo-bsize to \
         drive. Sizes for symbols without a real size (Mach-O) are an upper bound: the \
         distance to the next symbol in the same section, which also counts any anonymous \
         padding between them.\n\n",
    );

    out.push_str("## By crate\n\n");
    out.push_str("| Code | Data | Symbols | Crate |\n|---|---|---|---|\n");
    let mut crates: Vec<(&String, &CrateTotals)> = report.by_crate.iter().collect();
    crates
        .sort_by(|a, b| (b.1.code_bytes + b.1.data_bytes).cmp(&(a.1.code_bytes + a.1.data_bytes)));
    for (name, totals) in crates.into_iter().take(REPORT_TOP_CRATES) {
        out.push_str(&format!(
            "| {} | {} | {} | `{}` |\n",
            human_bytes(totals.code_bytes),
            human_bytes(totals.data_bytes),
            totals.symbol_count,
            name,
        ));
    }

    out.push_str("\n## Largest symbols\n\n");
    out.push_str("| Size | Crate | Symbol |\n|---|---|---|\n");
    for sym in &report.largest {
        out.push_str(&format!(
            "| {}{} | `{}` | `{}` |\n",
            human_bytes(sym.size),
            if sym.exact { "" } else { " (≤)" },
            sym.crate_name,
            sym.demangled,
        ));
    }

    out
}

fn human_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    let bytes_f = bytes as f64;
    if bytes_f >= MIB {
        format!("{:.1} MiB", bytes_f / MIB)
    } else if bytes_f >= KIB {
        format!("{:.1} KiB", bytes_f / KIB)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_of_extracts_the_first_path_segment() {
        assert_eq!(
            crate_of("perry_runtime::gc::copying::run_copied_minor_attempt"),
            "perry_runtime"
        );
        assert_eq!(crate_of("core::ptr::drop_in_place"), "core");
    }

    #[test]
    fn crate_of_strips_the_v0_disambiguator_hash() {
        // v0 mangling's real shape: `crate_name[16-hex-digit-hash]::path`.
        assert_eq!(
            crate_of("perry_runtime[bf1fb6611b4368e2]::object::class_meta_registry::PARENT_DENSE"),
            "perry_runtime"
        );
    }

    #[test]
    fn crate_of_reads_the_crate_out_of_an_associated_fn_receiver() {
        // `<Type>::method` — the crate name sits one level inside the `<`.
        assert_eq!(
            crate_of("<perry_runtime[bf1fb6611b4368e2]::gc::cycle::GcCycleState>::step"),
            "perry_runtime"
        );
    }

    #[test]
    fn crate_of_buckets_non_rust_names_as_native_other() {
        assert_eq!(crate_of("_CCRandomGenerateBytes"), "native/other");
        assert_eq!(crate_of("__NSGetArgc"), "native/other");
        assert_eq!(crate_of("main"), "native/other");
    }

    #[test]
    fn human_bytes_picks_the_right_unit() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(2048), "2.0 KiB");
        assert_eq!(human_bytes(3 * 1024 * 1024), "3.0 MiB");
    }

    #[test]
    fn report_path_appends_the_suffix() {
        assert_eq!(
            report_path_for(Path::new("/tmp/hello")),
            PathBuf::from("/tmp/hello.size-report.md")
        );
    }
}
