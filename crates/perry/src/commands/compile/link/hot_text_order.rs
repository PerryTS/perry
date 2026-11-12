//! Default hot-text order for the runtime archives.
//!
//! About a quarter of the runtime text a program links ever runs, and the
//! archive lays it out in compilation order: the executed functions are spread
//! over nearly every 64 KiB window of `.text`, and the kernel maps a whole
//! window around each first touch. `hot_text_order.txt` lists the runtime
//! functions a corpus of ordinary programs executes, most widely used first
//! (`scripts/hot_text_order.py` records and builds it). The link places the
//! listed functions together at the front of `.text`, so the pages a program
//! touches are packed and everything that never runs stays unmapped behind
//! them.
//!
//! Nothing about the program changes: no IR, no object, no runtime call, no
//! table in the binary. The only effect is in the linker script the link
//! already passes for the JS text section: it now also names input sections
//! of the runtime archives.
//!
//! * The list is keyed hash-free ([`symbol_key`]): a Rust symbol's crate
//!   disambiguators and back references are dropped, so a version bump or a
//!   rebuilt archive variant resolves the same lines. A listed function that
//!   no longer exists is skipped; `scripts/hot_text_order.py check` is the gate
//!   that turns a stale list red.
//! * The names are resolved against the archives of this link (a function's
//!   own input section, as the archive names it) and the script is kept in the
//!   compile cache per archive identity, so later links only pass the file.
//! * ELF links through GNU ld or lld only. Mach-O (`-order_file` takes exact
//!   symbol names), COFF, wasm, Android and HarmonyOS keep today's layout.
//! * `--function-order` (a per-program recording) replaces the default order.
//! * `PERRY_HOT_TEXT_ORDER=off` keeps today's layout.

use object::read::archive::ArchiveFile;
use object::{Object, ObjectSection, ObjectSymbol, SymbolKind};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// The checked-in default order: one [`symbol_key`] per line, hottest first.
const DEFAULT_ORDER: &str = include_str!("hot_text_order.txt");

/// `PERRY_HOT_TEXT_ORDER=off` (or `0`) keeps today's layout (A/B measurement).
fn enabled() -> bool {
    !matches!(
        std::env::var("PERRY_HOT_TEXT_ORDER").as_deref(),
        Ok("off" | "0" | "false" | "no")
    )
}

/// The hash-free identity of a symbol. Plain C names (`js_*`, `mi_*`) are
/// their own key. For a Rust v0 name the crate disambiguators and the other
/// disambiguators (`s<n>_`) and every back reference (`B<n>_`) lose their
/// number, and a ThinLTO promotion suffix (`.llvm.<n>`) is dropped; the path,
/// the types and the identifiers stay. So one key names a function whatever
/// the crate hashes of the build are, and the closures of one function share
/// a key from the second closure on (their indices shift when one is added).
///
/// The scan is deliberately simple, not a demangler: where it misreads a
/// symbol it still maps equal symbols to equal keys, which is all the layout
/// needs. `scripts/hot_text_order.py` holds the same function.
pub(super) fn symbol_key(symbol: &str) -> String {
    let symbol = symbol.find(".llvm.").map_or(symbol, |at| &symbol[..at]);
    if !symbol.starts_with("_R") || !symbol.is_ascii() {
        return symbol.to_string();
    }
    let b = symbol.as_bytes();
    let mut out = String::with_capacity(symbol.len());
    out.push_str("_R");
    let mut i = 2;
    while i < b.len() {
        let c = b[i];
        match c {
            // A vendor suffix ends the path.
            b'.' | b'$' => {
                out.push_str(&symbol[i..]);
                break;
            }
            // An identifier: decimal length, optional `_`, then the bytes.
            b'0'..=b'9' => {
                let mut j = i;
                let mut len = 0usize;
                while j < b.len() && b[j].is_ascii_digit() {
                    len = len
                        .saturating_mul(10)
                        .saturating_add((b[j] - b'0') as usize);
                    j += 1;
                }
                if j < b.len() && b[j] == b'_' {
                    j += 1;
                }
                let end = j.saturating_add(len).min(b.len());
                out.push_str(&symbol[i..end]);
                i = end;
            }
            // A back reference or a disambiguator: drop its base-62 number.
            b'B' | b's' => {
                let mut j = i + 1;
                while j < b.len() && b[j].is_ascii_alphanumeric() {
                    j += 1;
                }
                out.push(c as char);
                if j < b.len() && b[j] == b'_' {
                    out.push('_');
                    i = j + 1;
                } else {
                    i += 1;
                }
            }
            // A const generic: its type letter, then hex digits and `_`.
            b'K' if i + 1 < b.len() && b[i + 1].is_ascii_lowercase() && b[i + 1] != b'p' => {
                let mut j = i + 2;
                while j < b.len() && (b[j].is_ascii_hexdigit() || b[j] == b'n') {
                    j += 1;
                }
                if j < b.len() && b[j] == b'_' {
                    out.push_str(&symbol[i..=j]);
                    i = j + 1;
                } else {
                    out.push('K');
                    i += 1;
                }
            }
            // A lifetime or a binder: a short base-62 number, kept.
            b'L' | b'G' => {
                let mut j = i + 1;
                while j < b.len() && j - i <= 6 && b[j].is_ascii_alphanumeric() {
                    j += 1;
                }
                if j < b.len() && b[j] == b'_' {
                    out.push_str(&symbol[i..=j]);
                    i = j + 1;
                } else {
                    out.push(c as char);
                    i += 1;
                }
            }
            _ => {
                out.push(c as char);
                i += 1;
            }
        }
    }
    out
}

/// The list: key -> rank (0 is placed first; the first occurrence decides),
/// and how many keys the first tier holds (a `# tier` line starts a tier).
fn parse_order(text: &str) -> (HashMap<&str, u32>, u32) {
    let mut ranks = HashMap::new();
    let mut tiers = 0;
    let mut core = None;
    for line in text.lines() {
        let key = line.trim();
        if key.starts_with("# tier ") {
            tiers += 1;
            if tiers == 2 {
                core = Some(ranks.len() as u32);
            }
        }
        if key.is_empty() || key.starts_with('#') {
            continue;
        }
        let next = ranks.len() as u32;
        ranks.entry(key).or_insert(next);
    }
    let core = core.unwrap_or(ranks.len() as u32);
    (ranks, core)
}

/// A section name may enter a linker pattern only as a literal.
fn literal_section(name: &str) -> bool {
    name.starts_with(".text.")
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.$".contains(&b))
}

/// What the archives hold of the list: `(rank, input section)` in placement
/// order, and how many listed keys resolved.
#[derive(Debug, Default, PartialEq, Eq)]
struct Placement {
    sections: Vec<String>,
    listed: usize,
    resolved: usize,
    core_listed: usize,
    core_resolved: usize,
}

/// Rank every `(function symbol, its input section)` pair the list names.
fn place<'a>(
    ranks: &HashMap<&str, u32>,
    core: u32,
    symbols: impl Iterator<Item = (&'a str, &'a str)>,
) -> Placement {
    let mut placed: BTreeSet<(u32, String)> = BTreeSet::new();
    let mut hit: BTreeSet<u32> = BTreeSet::new();
    for (symbol, section) in symbols {
        if !literal_section(section) {
            continue;
        }
        let key = symbol_key(symbol);
        if let Some(&rank) = ranks.get(key.as_str()) {
            hit.insert(rank);
            placed.insert((rank, section.to_string()));
        }
    }
    // A section holds one function; two keys naming it keep the first rank.
    let mut seen = BTreeSet::new();
    let sections = placed
        .into_iter()
        .filter(|(_, section)| seen.insert(section.clone()))
        .map(|(_, section)| section)
        .collect();
    Placement {
        sections,
        listed: ranks.len(),
        resolved: hit.len(),
        core_listed: core as usize,
        core_resolved: hit.range(..core).count(),
    }
}

/// The linker script: one `.text` rule that takes the placed sections, in
/// order, and then everything the default rule takes, in the default order
/// (so the default rule that follows it is left empty and `.text` stays one
/// output section); then the JS text rule every ELF link passes. The two
/// symbols bound the run for `scripts/hot_text_order.py` (symbol-table entries
/// only: a stripped binary carries nothing of them).
fn script(placement: &Placement) -> String {
    let mut out = format!(
        "/* perry default hot-text order: listed={} resolved={} core={}/{} sections={} */\nSECTIONS {{\n .text : {{\n  __perry_rt_hot_text_start = .;\n",
        placement.listed,
        placement.resolved,
        placement.core_resolved,
        placement.core_listed,
        placement.sections.len()
    );
    for section in &placement.sections {
        out.push_str("  *(");
        out.push_str(section);
        out.push_str(")\n");
    }
    out.push_str(
        "  __perry_rt_hot_text_end = .;\n  *(.text.unlikely .text.*_unlikely .text.unlikely.*)\n  *(.text.exit .text.exit.*)\n  *(.text.startup .text.startup.*)\n  *(.text.hot .text.hot.*)\n  *(SORT(.text.sorted.*))\n  *(.text .stub .text.* .gnu.linkonce.t.*)\n }\n} INSERT BEFORE .text;\n",
    );
    out.push_str(perry_codegen::function_order::JS_TEXT_LINK_RULE);
    out
}

/// Read the function symbols of every ELF member of `archive` with the name
/// of the input section each one lives in.
fn archive_functions(archive: &Path, out: &mut Vec<(String, String)>) -> Option<()> {
    let data = std::fs::read(archive).ok()?;
    let file = ArchiveFile::parse(&*data).ok()?;
    for member in file.members() {
        let Ok(member) = member else { continue };
        let Ok(bytes) = member.data(&*data) else {
            continue;
        };
        let Ok(object) = object::File::parse(bytes) else {
            continue;
        };
        if object.format() != object::BinaryFormat::Elf {
            continue;
        }
        for symbol in object.symbols() {
            if symbol.kind() != SymbolKind::Text || symbol.is_undefined() {
                continue;
            }
            let Some(index) = symbol.section_index() else {
                continue;
            };
            let (Ok(name), Ok(section)) = (
                symbol.name(),
                object.section_by_index(index).and_then(|s| s.name()),
            ) else {
                continue;
            };
            if section.starts_with(".text.") {
                out.push((name.to_string(), section.to_string()));
            }
        }
    }
    Some(())
}

/// The cache identity of a resolved script: the list and each archive's path,
/// size and modification time.
fn fingerprint(archives: &[&Path]) -> Option<u64> {
    // FNV-1a: stable across processes and toolchains, unlike `DefaultHasher`.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |bytes: &[u8]| {
        for b in bytes.iter().chain(std::iter::once(&0xff)) {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    feed(DEFAULT_ORDER.as_bytes());
    for archive in archives {
        let meta = std::fs::metadata(archive).ok()?;
        let modified = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
        feed(archive.as_os_str().as_encoded_bytes());
        feed(&meta.len().to_le_bytes());
        feed(&modified.as_nanos().to_le_bytes());
    }
    Some(h)
}

/// The link script of a native ELF link that takes the default order: the
/// hot run of `archives` (the runtime archives the link may pull from) and
/// the JS text rule. `None` keeps today's script: the switch is off, the
/// program has its own order, or an archive cannot be read. Never an error:
/// the order is layout only.
pub(super) fn link_script(cache: &Path, archives: &[&Path], verbose: u8) -> Option<PathBuf> {
    if !enabled() || archives.is_empty() {
        return None;
    }
    // A recording build is laid out by its own instrumentation, and a
    // recorded order is the program's own choice.
    if !matches!(
        perry_codegen::program_function_layout(),
        perry_codegen::FunctionLayout::Default
    ) {
        return None;
    }
    let fingerprint = fingerprint(archives)?;
    let dir = cache.join("function-order");
    let path = dir.join(format!("hot-text-{fingerprint:016x}.v1.ld"));
    if !path.is_file() {
        let (ranks, core) = parse_order(DEFAULT_ORDER);
        let mut functions = Vec::new();
        for archive in archives {
            archive_functions(archive, &mut functions)?;
        }
        let placement = place(
            &ranks,
            core,
            functions.iter().map(|(n, s)| (n.as_str(), s.as_str())),
        );
        if placement.sections.is_empty() {
            return None;
        }
        // Concurrent links share the cache: write a sibling, then rename.
        let tmp = path.with_extension(format!("tmp{}", std::process::id()));
        if std::fs::create_dir_all(&dir).is_err()
            || std::fs::write(&tmp, script(&placement)).is_err()
            || std::fs::rename(&tmp, &path).is_err()
        {
            let _ = std::fs::remove_file(&tmp);
            return None;
        }
    }
    if verbose > 0 {
        // The script's first line says how much of the list the archives hold.
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let summary = text.lines().next().unwrap_or_default();
        eprintln!("[hot-text] {}: {summary}", path.display());
    }
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The vectors `scripts/hot_text_order.py selftest` checks too.
    const KEYS: &[(&str, &str)] = &[
        ("js_string_concat", "js_string_concat"),
        ("mi_process_attach", "mi_process_attach"),
        (
            "_RNvNtNtCsXbwMRDrjNy_13perry_runtime2gc6policy25gc_budgeted_start_or_step",
            "_RNvNtNtCs_13perry_runtime2gc6policy25gc_budgeted_start_or_step",
        ),
        (
            "_RNvNtNtCsklhtP6PwTF_13perry_runtime2gc6policy25gc_budgeted_start_or_step",
            "_RNvNtNtCs_13perry_runtime2gc6policy25gc_budgeted_start_or_step",
        ),
        (
            "_RINvMs_NtNtNtCsXbwMRDrjNy_13perry_runtime2gc5roots15runtime_handlesNtB5_18RuntimeHandleScope16root_raw_mut_ptrNtNtBb_6object12ObjectHeaderEBb_",
            "_RINvMs_NtNtNtCs_13perry_runtime2gc5roots15runtime_handlesNtB_18RuntimeHandleScope16root_raw_mut_ptrNtNtB_6object12ObjectHeaderEB_",
        ),
        (
            "_RNCINvNtNtCsXbwMRDrjNy_13perry_runtime5arena12owned_stores22walk_from_space_ownersNCNvB4_50finalize_dead_copied_minor_from_space_owned_storess0_0E0B8_",
            "_RNCINvNtNtCs_13perry_runtime5arena12owned_stores22walk_from_space_ownersNCNvB_50finalize_dead_copied_minor_from_space_owned_storess_0E0B_",
        ),
        (
            "_RNvNtNtCsXbwMRDrjNy_13perry_runtime5array15header_gc_slots25store_array_slot_resolved.llvm.2236164344119935801",
            "_RNvNtNtCs_13perry_runtime5array15header_gc_slots25store_array_slot_resolved",
        ),
        (
            "_RINvNtNtNtCsXbwMRDrjNy_13perry_runtime6object6shapes12layout_probe9owned_getKb1_Kb0_EB8_",
            "_RINvNtNtNtCs_13perry_runtime6object6shapes12layout_probe9owned_getKb1_Kb0_EB_",
        ),
        (
            "_RNvMs4_NtNtCsklhtP6PwTF_5alloc11collections9vec_dequeINtB5_8VecDequeNtNtCsXbwMRDrjNy_13perry_runtime3map16CompactionRecordE4growB18_",
            "_RNvMs_NtNtCs_5alloc11collections9vec_dequeINtB_8VecDequeNtNtCs_13perry_runtime3map16CompactionRecordE4growB_",
        ),
    ];

    #[test]
    fn keys_drop_hashes_and_back_references_and_nothing_else() {
        for (symbol, key) in KEYS {
            assert_eq!(symbol_key(symbol), *key, "{symbol}");
            assert_eq!(symbol_key(key), *key, "a key is its own key: {key}");
        }
        // Different functions keep different keys.
        let keys: BTreeSet<&str> = KEYS.iter().map(|(_, key)| *key).collect();
        assert_eq!(keys.len(), KEYS.len() - 1, "only the rehashed pair shares");
    }

    #[test]
    fn listed_functions_are_placed_in_list_order_whatever_the_hashes() {
        let (ranks, core) = parse_order(
            "# tier core\n_RNvNtCs_13perry_runtime1b3two\n# tier common\njs_one\n\ngone_function\n_RNvNtCs_13perry_runtime1b3two\n",
        );
        assert_eq!((ranks.len(), core), (3, 1));
        let symbols = [
            ("js_unlisted", ".text.js_unlisted"),
            ("js_one", ".text.js_one"),
            (
                "_RNvNtCsAAAAAAAAAAA_13perry_runtime1b3two",
                ".text._RNvNtCsAAAAAAAAAAA_13perry_runtime1b3two",
            ),
            // The same function in a second archive built with another hash.
            (
                "_RNvNtCsBBBBBBBBBB_13perry_runtime1b3two",
                ".text.unlikely._RNvNtCsBBBBBBBBBB_13perry_runtime1b3two",
            ),
            // Not a function section of its own: cannot be placed.
            ("js_one", ".text"),
            ("js_one", ".text.a*b"),
        ];
        let placement = place(&ranks, core, symbols.iter().copied());
        assert_eq!(
            placement.sections,
            [
                ".text._RNvNtCsAAAAAAAAAAA_13perry_runtime1b3two",
                ".text.unlikely._RNvNtCsBBBBBBBBBB_13perry_runtime1b3two",
                ".text.js_one",
            ]
        );
        assert_eq!((placement.listed, placement.resolved), (3, 2));
        assert_eq!((placement.core_listed, placement.core_resolved), (1, 1));
        let script = script(&placement);
        assert!(script.contains("listed=3 resolved=2 core=1/1 sections=3"));
        assert!(script.ends_with(perry_codegen::function_order::JS_TEXT_LINK_RULE));
        // The rule takes the rest of `.text` after the run, so the default
        // rule behind it is empty and the JS text follows all of it.
        let rest = script
            .find("*(.text .stub .text.* .gnu.linkonce.t.*)")
            .unwrap();
        assert!(script.find("__perry_rt_hot_text_end").unwrap() < rest);
        assert!(rest < script.find("} INSERT BEFORE .text;").unwrap());
        let one = script.find("*(.text.js_one)").unwrap();
        let two = script.find("*(.text._RNvNtCsAAAAAAAAAAA_").unwrap();
        assert!(two < one, "{script}");
        assert!(script.find("__perry_rt_hot_text_start").unwrap() < two);
        assert!(script.find("__perry_rt_hot_text_end").unwrap() > one);
    }

    #[test]
    fn the_checked_in_order_is_a_list_of_keys() {
        let (ranks, core) = parse_order(DEFAULT_ORDER);
        assert!(ranks.len() > 500, "the default order is not a stub");
        assert!(core > 100 && (core as usize) < ranks.len(), "tiers: {core}");
        for key in ranks.keys() {
            assert_eq!(symbol_key(key), *key, "not a hash-free key: {key}");
            assert!(
                literal_section(&format!(".text.{key}")),
                "not a section-safe name: {key}"
            );
        }
    }
}
