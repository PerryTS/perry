//! `--report-size`: attribute the final linked binary's size to the crates
//! that produced it, and surface concrete, actionable findings.
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
//! Deliberately does not attempt cargo-bsize's DWARF/LTO-provenance analysis
//! (type layout, source-line attribution, assembly instruction patterns):
//! those need either a `cargo build` rebuild with instrumentation flags or a
//! disassembler, neither of which fits Perry's own two-stage build
//! (static-archive compile, then a raw `cc`/`ld` link of those archives plus
//! LLVM-emitted object code). This is a symbol-table-only view of whatever
//! made it into the final link — code/data attribution, duplicate function
//! bodies, duplicate crate instances, generic-monomorphization cost, and a
//! few named cost patterns (panics, `Debug`/`Display` formatting, vtables).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use object::{Object, ObjectSection, ObjectSymbol, SectionKind, SymbolKind};
use serde::Serialize;

use crate::OutputFormat;

const REPORT_TOP_CRATES: usize = 20;
const REPORT_TOP_SYMBOLS: usize = 30;
const REPORT_TOP_FAMILIES: usize = 15;
const REPORT_TOP_DUPLICATES: usize = 15;
const REPORT_TOP_SUGGESTIONS: usize = 10;

#[derive(Default, Serialize)]
struct CrateTotals {
    code_bytes: u64,
    data_bytes: u64,
    symbol_count: usize,
}

#[derive(Serialize)]
struct RankedSymbol {
    demangled: String,
    crate_name: String,
    size: u64,
    exact: bool,
}

#[derive(Serialize)]
struct GenericFamily {
    crate_name: String,
    family: String,
    instantiations: usize,
    total_bytes: u64,
}

#[derive(Serialize)]
struct DuplicateBody {
    size: u64,
    copies: usize,
    wasted_bytes: u64,
    symbols: Vec<String>,
}

/// Same crate name compiled independently more than once (proof, not
/// inference — a distinct v0-mangling disambiguator hash per build, unlike
/// reading `Cargo.lock`, which only proves a version is *resolvable*).
///
/// This is a compile-time / intermediate-archive-size finding, not a
/// shipped-binary-size one: a successful link proves each hash's content is
/// linked at most once (the linker errors on a true duplicate-symbol
/// inclusion), so `total_bytes` is real, in-use code in the final binary —
/// not bytes recoverable by deduplicating it there.
#[derive(Serialize)]
struct DuplicateCrateInstance {
    crate_name: String,
    hashes: Vec<String>,
    total_bytes: u64,
}

#[derive(Serialize)]
struct PatternTotal {
    name: &'static str,
    bytes: u64,
    count: usize,
}

#[derive(Serialize)]
struct Suggestion {
    kind: &'static str,
    summary: String,
    estimated_bytes: u64,
}

#[derive(Serialize)]
struct SizeReport {
    binary: String,
    file_bytes: u64,
    code_section_bytes: u64,
    data_section_bytes: u64,
    code_attributed_bytes: u64,
    data_attributed_bytes: u64,
    symbol_count: usize,
    by_crate: Vec<(String, CrateTotals)>,
    largest: Vec<RankedSymbol>,
    generic_families: Vec<GenericFamily>,
    duplicate_bodies: Vec<DuplicateBody>,
    duplicate_crate_instances: Vec<DuplicateCrateInstance>,
    patterns: Vec<PatternTotal>,
    suggestions: Vec<Suggestion>,
}

/// Write `<exe_path>.size-report.md` and `<exe_path>.size-report.json`.
/// Best-effort and silent unless `--report-size` was actually passed: a
/// read/parse failure here must never fail the build, the report is a
/// diagnostic extra, not a build product.
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

    let md_path = report_path_for(exe_path, "md");
    let markdown = render_markdown(&report);
    if let Err(e) = fs::write(&md_path, markdown) {
        if let OutputFormat::Text = format {
            eprintln!("warning: failed to write size report: {e}");
        }
        return;
    }

    let json_path = report_path_for(exe_path, "json");
    match serde_json::to_string_pretty(&report) {
        Ok(json) => {
            if let Err(e) = fs::write(&json_path, json) {
                if let OutputFormat::Text = format {
                    eprintln!("warning: failed to write size-report.json: {e}");
                }
            }
        }
        Err(e) => {
            if let OutputFormat::Text = format {
                eprintln!("warning: failed to serialize size-report.json: {e}");
            }
        }
    }

    if let OutputFormat::Text = format {
        let unattributed = (report.code_section_bytes + report.data_section_bytes)
            .saturating_sub(report.code_attributed_bytes + report.data_attributed_bytes);
        println!("Wrote size report: {}", md_path.display());
        println!("Wrote size report data: {}", json_path.display());
        println!(
            "  {} attributed across {} symbols, {} unattributed (inlined/no-symbol bytes)",
            human_bytes(report.code_attributed_bytes + report.data_attributed_bytes),
            report.symbol_count,
            human_bytes(unattributed),
        );
        if !report.suggestions.is_empty() {
            println!("  Top suggestion: {}", report.suggestions[0].summary);
        }
    }
}

fn report_path_for(exe_path: &Path, ext: &str) -> PathBuf {
    let mut s = exe_path.as_os_str().to_owned();
    s.push(format!(".size-report.{ext}"));
    PathBuf::from(s)
}

struct RawSymbol<'a> {
    section: u64,
    address: u64,
    name: &'a str,
    size: u64,
    exact: bool,
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
        let bucket = match symbol.kind() {
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
    let mut raw: Vec<RawSymbol> = Vec::new();
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
            raw.push(RawSymbol {
                section,
                address,
                name,
                size,
                exact,
            });
        }
    }

    let code_section_indices: std::collections::HashSet<u64> = file
        .sections()
        .filter(|s| s.kind() == SectionKind::Text)
        .map(|s| s.index().0 as u64)
        .collect();

    let mut by_crate: BTreeMap<String, CrateTotals> = BTreeMap::new();
    let mut largest_all: Vec<RankedSymbol> = Vec::new();
    let mut code_attributed_bytes = 0u64;
    let mut data_attributed_bytes = 0u64;
    let mut family_totals: BTreeMap<(String, String), (usize, u64)> = BTreeMap::new();
    let mut crate_hashes: BTreeMap<String, std::collections::BTreeSet<String>> = BTreeMap::new();
    let mut crate_hash_bytes: BTreeMap<(String, String), u64> = BTreeMap::new();
    let mut body_bytes: BTreeMap<&[u8], Vec<(String, u64)>> = BTreeMap::new(); // exact bytes -> [(symbol, size)]
    let mut pattern_totals: BTreeMap<&'static str, (u64, usize)> = BTreeMap::new();

    for sym in &raw {
        let demangled = demangle(sym.name);
        let (crate_name, hash) = crate_and_hash(&demangled);
        let is_code = code_section_indices.contains(&sym.section);

        let totals = by_crate.entry(crate_name.clone()).or_default();
        totals.symbol_count += 1;
        if is_code {
            totals.code_bytes += sym.size;
            code_attributed_bytes += sym.size;
        } else {
            totals.data_bytes += sym.size;
            data_attributed_bytes += sym.size;
        }

        if let Some(hash) = &hash {
            crate_hashes
                .entry(crate_name.clone())
                .or_default()
                .insert(hash.clone());
            *crate_hash_bytes
                .entry((crate_name.clone(), hash.clone()))
                .or_insert(0) += sym.size;
        }

        if crate_name != "native/other" {
            let family = generic_family(&demangled);
            if family != demangled {
                let entry = family_totals
                    .entry((crate_name.clone(), family))
                    .or_insert((0, 0));
                entry.0 += 1;
                entry.1 += sym.size;
            }
        }

        for (pattern_name, matches) in PATTERNS {
            if matches(&demangled) {
                let entry = pattern_totals.entry(pattern_name).or_insert((0, 0));
                entry.0 += sym.size;
                entry.1 += 1;
            }
        }

        if let Ok(section) = file.section_by_index(object::SectionIndex(sym.section as usize)) {
            if let Ok(Some(bytes)) = section.data_range(sym.address, sym.size) {
                // Keyed on the exact byte slice (`&[u8]` is `Ord`), not a
                // hash of it — a duplicate-body finding is a claim serious
                // enough that a hash collision must not be able to fabricate
                // one.
                body_bytes
                    .entry(bytes)
                    .or_default()
                    .push((demangled.clone(), sym.size));
            }
        }

        largest_all.push(RankedSymbol {
            demangled,
            crate_name,
            size: sym.size,
            exact: sym.exact,
        });
    }

    largest_all.sort_by_key(|a| std::cmp::Reverse(a.size));
    let symbol_count: usize = by_crate.values().map(|t| t.symbol_count).sum();

    let mut generic_families: Vec<GenericFamily> = family_totals
        .into_iter()
        .filter(|(_, (count, _))| *count > 1)
        .map(
            |((crate_name, family), (instantiations, total_bytes))| GenericFamily {
                crate_name,
                family,
                instantiations,
                total_bytes,
            },
        )
        .collect();
    generic_families.sort_by_key(|a| std::cmp::Reverse(a.total_bytes));
    generic_families.truncate(REPORT_TOP_FAMILIES);

    let mut duplicate_bodies: Vec<DuplicateBody> = body_bytes
        .into_values()
        .filter(|group| group.len() > 1)
        .map(|group| {
            let size = group[0].1;
            let copies = group.len();
            DuplicateBody {
                size,
                copies,
                wasted_bytes: size * (copies as u64 - 1),
                symbols: group.into_iter().map(|(name, _)| name).collect(),
            }
        })
        .collect();
    duplicate_bodies.sort_by_key(|a| std::cmp::Reverse(a.wasted_bytes));
    duplicate_bodies.truncate(REPORT_TOP_DUPLICATES);

    let mut duplicate_crate_instances: Vec<DuplicateCrateInstance> = crate_hashes
        .into_iter()
        .filter(|(_, hashes)| hashes.len() > 1)
        .map(|(crate_name, hashes)| {
            let total_bytes = hashes
                .iter()
                .map(|h| {
                    crate_hash_bytes
                        .get(&(crate_name.clone(), h.clone()))
                        .copied()
                        .unwrap_or(0)
                })
                .sum();
            DuplicateCrateInstance {
                crate_name,
                hashes: hashes.into_iter().collect(),
                total_bytes,
            }
        })
        .collect();
    duplicate_crate_instances.sort_by_key(|a| std::cmp::Reverse(a.total_bytes));

    let mut patterns: Vec<PatternTotal> = pattern_totals
        .into_iter()
        .map(|(name, (bytes, count))| PatternTotal { name, bytes, count })
        .collect();
    patterns.sort_by_key(|a| std::cmp::Reverse(a.bytes));

    let suggestions = build_suggestions(
        &duplicate_crate_instances,
        &generic_families,
        &duplicate_bodies,
        &patterns,
    );

    let by_crate_sorted: Vec<(String, CrateTotals)> = {
        let mut v: Vec<(String, CrateTotals)> = by_crate.into_iter().collect();
        v.sort_by_key(|a| std::cmp::Reverse(a.1.code_bytes + a.1.data_bytes));
        v
    };

    largest_all.truncate(REPORT_TOP_SYMBOLS);

    Ok(SizeReport {
        binary: exe_path.file_name().map_or_else(
            || exe_path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        ),
        file_bytes,
        code_section_bytes,
        data_section_bytes,
        code_attributed_bytes,
        data_attributed_bytes,
        symbol_count,
        by_crate: by_crate_sorted,
        largest: largest_all,
        generic_families,
        duplicate_bodies,
        duplicate_crate_instances,
        patterns,
        suggestions,
    })
}

fn build_suggestions(
    duplicate_crate_instances: &[DuplicateCrateInstance],
    generic_families: &[GenericFamily],
    duplicate_bodies: &[DuplicateBody],
    patterns: &[PatternTotal],
) -> Vec<Suggestion> {
    let mut out = Vec::new();

    for dup in duplicate_crate_instances {
        out.push(Suggestion {
            kind: "duplicate-compile-crate-instance",
            summary: format!(
                "`{}` is compiled independently {} times ({}) — once each inside \
                 `perry-runtime`'s and `perry-stdlib`'s separate `cargo build` invocations, not \
                 a Cargo.lock version conflict. This is redundant COMPILE work and bloats the \
                 intermediate `.a` archives; it is NOT necessarily {} of recoverable shipped-\
                 binary size — a successful link proves each hash's content is linked at most \
                 once (the linker errors on a true duplicate-symbol inclusion), so every byte \
                 attributed here is real, in-use code in this binary, not waste sitting twice in \
                 it. Extending Perry's existing archive-dedup pass (today scoped to \
                 `dedup_runtime_for_tier3`/`dedup_stdlib_for_tier3`) to the default build path \
                 would speed up incremental/auto-optimize builds and shrink the intermediate \
                 archives; whether it also shrinks a given shipped binary depends on whether that \
                 binary's link happens to need both hash-variants — a separate, per-binary claim \
                 this report does not make.",
                dup.crate_name,
                dup.hashes.len(),
                dup.hashes.join(", "),
                human_bytes(dup.total_bytes),
            ),
            // Deliberately not `dup.total_bytes`: that is real, in-use code
            // in THIS binary (see summary), not a recoverable-bytes claim —
            // giving it a nonzero estimate here would misrank it against
            // suggestions that genuinely shrink the shipped binary.
            estimated_bytes: 0,
        });
    }

    for family in generic_families {
        if family.total_bytes < 2048 {
            continue;
        }
        out.push(Suggestion {
            kind: "generic-monomorphization",
            summary: format!(
                "`{}::{}` is monomorphized {} times, {} total — consider a dynamic-dispatch (`dyn Trait`) or type-erased path if the call sites don't need static dispatch",
                family.crate_name,
                family.family,
                family.instantiations,
                human_bytes(family.total_bytes),
            ),
            estimated_bytes: family.total_bytes,
        });
    }

    for dup in duplicate_bodies {
        if dup.wasted_bytes < 512 {
            continue;
        }
        out.push(Suggestion {
            kind: "duplicate-function-body",
            summary: format!(
                "{} symbols share one identical body ({} each, {} wasted): {}",
                dup.copies,
                human_bytes(dup.size),
                human_bytes(dup.wasted_bytes),
                dup.symbols.first().map(String::as_str).unwrap_or(""),
            ),
            estimated_bytes: dup.wasted_bytes,
        });
    }

    for pattern in patterns {
        if pattern.bytes < 8192 {
            continue;
        }
        let advice = match pattern.name {
            "panic-path" => {
                "panic=abort is already the default for compiled programs; this is what remains \
                 after that (formatted panic messages, bounds-check panic sites) — reducing \
                 `.unwrap()`/indexing in hot paths shrinks it further"
            }
            "fmt-debug-display" => {
                "Debug/Display formatting code for types that are never actually printed can be \
                 dropped by removing the derive or gating it behind a debug-only feature"
            }
            "vtable" => {
                "trait-object dispatch tables — expected if `dyn Trait` is used deliberately"
            }
            _ => "",
        };
        out.push(Suggestion {
            kind: pattern.name,
            summary: format!(
                "{} across {} symbols in `{}`: {advice}",
                human_bytes(pattern.bytes),
                pattern.count,
                pattern.name,
            ),
            estimated_bytes: pattern.bytes,
        });
    }

    out.sort_by_key(|a| std::cmp::Reverse(a.estimated_bytes));
    out.truncate(REPORT_TOP_SUGGESTIONS);
    out
}

type PatternMatcher = (&'static str, fn(&str) -> bool);

/// Named cost patterns the size-reduction literature keeps naming. These
/// overlap by design (a function can be both a panic path and behind a
/// generic) — each just points at code worth looking at, not a partition.
const PATTERNS: &[PatternMatcher] = &[
    ("panic-path", |d| {
        d.contains("core::panicking") || d.contains("::panic_fmt") || d.contains("panic::Location")
    }),
    ("fmt-debug-display", |d| {
        d.contains("as core::fmt::Debug>::fmt") || d.contains("as core::fmt::Display>::fmt")
    }),
    ("vtable", |d| {
        d.contains("{vtable}") || d.contains("vtable_for")
    }),
];

/// Demangle a Rust symbol name. `rustc_demangle` returns non-Rust input
/// unchanged — the normal case for libc/system symbols — and `crate_of`
/// below buckets those as `native/other`.
fn demangle(name: &str) -> String {
    rustc_demangle::demangle(name).to_string()
}

/// The crate a demangled Rust path belongs to (its first `::`-delimited
/// segment) and, when present, the v0-mangling disambiguator hash right
/// after it (`crate_name[16 hex digits]`). Two symbols from the SAME crate
/// NAME but DIFFERENT hashes are proof two separate builds of that crate
/// both made it into the final link — see `DuplicateCrateInstance`.
///
/// `<Type as Trait>::method` / `<Type>::method` associated-fn forms put the
/// crate name one level in; the leading `<` is stripped before reading it.
/// Non-Rust names (no `::`, or containing characters a Rust path segment
/// can't) bucket as `native/other`.
fn crate_and_hash(demangled: &str) -> (String, Option<String>) {
    let trimmed = demangled.trim_start_matches('<');
    let ident_end = trimmed
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(trimmed.len());
    let candidate = &trimmed[..ident_end];
    let starts_like_ident = candidate
        .chars()
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_');
    let rest = &trimmed[ident_end..];
    if starts_like_ident && rest.starts_with('[') {
        let hash = rest.find(']').map(|end| rest[1..end].to_string());
        (candidate.to_string(), hash)
    } else if starts_like_ident && rest.starts_with("::") {
        (candidate.to_string(), None)
    } else {
        ("native/other".to_string(), None)
    }
}

/// The crate a demangled Rust path belongs to — thin wrapper over
/// `crate_and_hash` for call sites that don't need the hash.
#[cfg(test)]
fn crate_of(demangled: &str) -> String {
    crate_and_hash(demangled).0
}

/// Collapse every identifier-attached `<...>` generic-argument list into a
/// single `<_>` placeholder, so `foo::<ConcreteA>` and `foo::<ConcreteB>` —
/// two monomorphizations of the same generic code — group under one family
/// key. A bare (non-attached) `<` — `<Type as Trait>::method`'s receiver
/// wrapper — is passed through unchanged rather than treated as an argument
/// list, so `<HashMap<K, V> as Trait>::method` still blanks to
/// `<HashMap<_> as Trait>::method`, not to nothing.
fn generic_family(demangled: &str) -> String {
    let chars: Vec<char> = demangled.chars().collect();
    let mut result = String::with_capacity(demangled.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        // `Name<Args>` (preceded by an identifier char or the v0 hash's `]`)
        // or turbofish `path::<Args>` (preceded by `:`) — an argument list,
        // blanked to a single placeholder. A BARE `<` (preceded by nothing,
        // whitespace, or another bracket — `<Type as Trait>::method`'s
        // receiver wrapper) is not an argument list itself, so it is pushed
        // through like any other character; scanning then continues INSIDE
        // it and still finds — and blanks — any attached generic args the
        // wrapped type carries (`<HashMap<_> as Trait>::method`, not the
        // wrapped type's name disappearing along with its own arguments).
        let attached = i > 0
            && (chars[i - 1].is_alphanumeric()
                || chars[i - 1] == '_'
                || chars[i - 1] == ']'
                || chars[i - 1] == ':');
        if c == '<' && attached {
            let mut depth = 1i32;
            let mut j = i + 1;
            while j < chars.len() && depth > 0 {
                match chars[j] {
                    '<' => depth += 1,
                    '>' => depth -= 1,
                    _ => {}
                }
                j += 1;
            }
            result.push_str("<_>");
            i = j;
            continue;
        }
        result.push(c);
        i += 1;
    }
    result
}

fn render_markdown(report: &SizeReport) -> String {
    let mut out = String::new();
    out.push_str(&format!("# Size report: {}\n\n", report.binary));
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
         the same core technique [cargo-bsize](https://github.com/boshen/cargo-bsize) uses on a \
         `cargo build` rebuild — applied here directly to what Perry actually links, since \
         Perry's static-archive-then-`cc`/`ld` build has no `cargo build` for cargo-bsize to \
         drive. Sizes for symbols without a real size (Mach-O) are an upper bound: the \
         distance to the next symbol in the same section, which also counts any anonymous \
         padding between them. Machine-readable data alongside this file: \
         `<output>.size-report.json`.\n\n",
    );

    if !report.suggestions.is_empty() {
        out.push_str("## Suggestions\n\n");
        for s in &report.suggestions {
            out.push_str(&format!(
                "- **{}** (~{}): {}\n",
                s.kind,
                human_bytes(s.estimated_bytes),
                s.summary
            ));
        }
        out.push('\n');
    }

    out.push_str("## By crate\n\n");
    out.push_str("| Code | Data | Symbols | Crate |\n|---|---|---|---|\n");
    for (name, totals) in report.by_crate.iter().take(REPORT_TOP_CRATES) {
        out.push_str(&format!(
            "| {} | {} | {} | `{}` |\n",
            human_bytes(totals.code_bytes),
            human_bytes(totals.data_bytes),
            totals.symbol_count,
            name,
        ));
    }

    if !report.duplicate_crate_instances.is_empty() {
        out.push_str("\n## Duplicate crate instances\n\n");
        out.push_str(
            "Same crate name compiled independently more than once (proven from the symbol \
             table's own disambiguator hash, not inferred from `Cargo.lock`). This is a \
             compile-time / intermediate-archive-size finding: a successful link proves each \
             hash's content is linked at most once, so the `Total` column is real, in-use code \
             in this binary — not bytes recoverable by deduplicating it here.\n\n",
        );
        out.push_str("| Total | Copies | Crate |\n|---|---|---|\n");
        for dup in &report.duplicate_crate_instances {
            out.push_str(&format!(
                "| {} | {} | `{}` |\n",
                human_bytes(dup.total_bytes),
                dup.hashes.len(),
                dup.crate_name,
            ));
        }
    }

    if !report.generic_families.is_empty() {
        out.push_str("\n## Generic monomorphization\n\n");
        out.push_str("| Total | Instantiations | Crate | Family |\n|---|---|---|---|\n");
        for f in &report.generic_families {
            out.push_str(&format!(
                "| {} | {} | `{}` | `{}` |\n",
                human_bytes(f.total_bytes),
                f.instantiations,
                f.crate_name,
                f.family,
            ));
        }
    }

    if !report.duplicate_bodies.is_empty() {
        out.push_str("\n## Duplicate function/data bodies\n\n");
        out.push_str("| Wasted | Copies | Each | Example symbol |\n|---|---|---|---|\n");
        for dup in &report.duplicate_bodies {
            out.push_str(&format!(
                "| {} | {} | {} | `{}` |\n",
                human_bytes(dup.wasted_bytes),
                dup.copies,
                human_bytes(dup.size),
                dup.symbols.first().map(String::as_str).unwrap_or(""),
            ));
        }
    }

    if !report.patterns.is_empty() {
        out.push_str("\n## Named cost patterns\n\n");
        out.push_str("| Bytes | Symbols | Pattern |\n|---|---|---|\n");
        for p in &report.patterns {
            out.push_str(&format!(
                "| {} | {} | {} |\n",
                human_bytes(p.bytes),
                p.count,
                p.name,
            ));
        }
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
    fn crate_and_hash_extracts_the_v0_disambiguator() {
        assert_eq!(
            crate_and_hash(
                "perry_runtime[bf1fb6611b4368e2]::object::class_meta_registry::PARENT_DENSE"
            ),
            (
                "perry_runtime".to_string(),
                Some("bf1fb6611b4368e2".to_string())
            )
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
    fn generic_family_collapses_a_turbofish_instantiation() {
        assert_eq!(
            generic_family(
                "perry_hir::walker::expr_ref::walk_expr_children::<perry_codegen::Visitor>"
            ),
            "perry_hir::walker::expr_ref::walk_expr_children::<_>"
        );
    }

    #[test]
    fn generic_family_groups_two_different_concrete_instantiations_together() {
        let a = generic_family("core::ptr::drop_in_place::<alloc::vec::Vec<u8>>");
        let b = generic_family(
            "core::ptr::drop_in_place::<alloc::vec::Vec<perry_hir::ir::expr::Expr>>",
        );
        assert_eq!(a, b);
        assert_eq!(a, "core::ptr::drop_in_place::<_>");
    }

    #[test]
    fn generic_family_is_unchanged_for_a_non_generic_symbol() {
        let name = "perry_runtime::gc::copying::run_copied_minor_attempt";
        assert_eq!(generic_family(name), name);
    }

    #[test]
    fn generic_family_preserves_the_receiver_type_under_a_bare_wrapper() {
        // The real bug this pins: a naive depth-0-only emit swallowed the
        // whole `<hashbrown::...::HashMap<...> as Trait>` receiver, leaving
        // a family key that started with a bare `>` and had lost which type
        // and method it even was.
        assert_eq!(
            generic_family(
                "<hashbrown::map::HashMap<u32, perry_hir::ir::expr::Expr> as hashbrown::map::HashMapExt>::reserve_rehash"
            ),
            "<hashbrown::map::HashMap<_> as hashbrown::map::HashMapExt>::reserve_rehash"
        );
    }

    #[test]
    fn human_bytes_picks_the_right_unit() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(2048), "2.0 KiB");
        assert_eq!(human_bytes(3 * 1024 * 1024), "3.0 MiB");
    }

    #[test]
    fn report_path_appends_the_extension() {
        assert_eq!(
            report_path_for(Path::new("/tmp/hello"), "md"),
            PathBuf::from("/tmp/hello.size-report.md")
        );
        assert_eq!(
            report_path_for(Path::new("/tmp/hello"), "json"),
            PathBuf::from("/tmp/hello.size-report.json")
        );
    }
}
