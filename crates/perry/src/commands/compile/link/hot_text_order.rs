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
//! * ELF links through GNU ld or lld. Mach-O takes the same list as an
//!   `-order_file` of exact names (`hot_text_order_macho`), opt-in until it is
//!   measured there. COFF, wasm, Android and HarmonyOS keep today's layout.
//! * `--function-order` (a per-program recording) replaces the default order.
//! * `PERRY_HOT_TEXT_ORDER=off` keeps today's layout. The same script carries
//!   the default hot-data order (`hot_data_order`, `PERRY_HOT_DATA_ORDER`).
//! * lld refuses an output section inserted before itself ("unable to insert
//!   .text before .text"), so its script names the hot run's output section
//!   `.text.perry_hot` and leaves the rest of `.text` to the linker's own
//!   placement right behind it (`elf_linker`).

use object::read::archive::ArchiveFile;
use object::{Object, ObjectSection, ObjectSymbol, SymbolKind};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// The checked-in default order: one [`symbol_key`] per line, hottest first.
pub(super) const DEFAULT_ORDER: &str = include_str!("hot_text_order.txt");

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
/// and the rank each tier starts at (a `# tier` line starts a tier; the first
/// tier is the core every program runs).
pub(super) fn parse_order(text: &str) -> (HashMap<&str, u32>, Vec<u32>) {
    let mut ranks = HashMap::new();
    let mut tiers = Vec::new();
    for line in text.lines() {
        let key = line.trim();
        if key.starts_with("# tier ") {
            tiers.push(ranks.len() as u32);
        }
        if key.is_empty() || key.starts_with('#') {
            continue;
        }
        let next = ranks.len() as u32;
        ranks.entry(key).or_insert(next);
    }
    if tiers.first() != Some(&0) {
        tiers.insert(0, 0);
    }
    (ranks, tiers)
}

/// A section name may enter a linker pattern only as a literal.
fn literal_section(name: &str) -> bool {
    name.starts_with(".text.")
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.$".contains(&b))
}

/// What the archives hold of the list: the input sections in placement
/// order, and how many listed keys resolved (of all, and of the core tier).
#[derive(Debug, Default, PartialEq, Eq)]
struct Placement {
    sections: Vec<String>,
    listed: usize,
    resolved: usize,
    core_listed: usize,
    core_resolved: usize,
}

/// A function of an archive: its symbol, its input section and that section's
/// alignment.
type ArchiveFunction<'a> = (&'a str, &'a str, u64);

/// Place every function the list names: by tier, and by rank within a tier,
/// except that a tier's sections with less than the usual 16-byte alignment
/// (cold stubs and tiny functions) close the tier together, widest alignment
/// first. Spread among the others, each one changes the padding in front of
/// its successor; together they pack as tightly as the unordered layout packs
/// them, so the order does not grow `.text` by padding.
fn place<'a>(
    ranks: &HashMap<&str, u32>,
    tiers: &[u32],
    functions: impl Iterator<Item = ArchiveFunction<'a>>,
) -> Placement {
    let core = tiers.get(1).copied().unwrap_or(ranks.len() as u32);
    let mut placed: BTreeSet<(usize, std::cmp::Reverse<u64>, u32, String)> = BTreeSet::new();
    let mut hit: BTreeSet<u32> = BTreeSet::new();
    for (symbol, section, align) in functions {
        if !literal_section(section) {
            continue;
        }
        let key = symbol_key(symbol);
        if let Some(&rank) = ranks.get(key.as_str()) {
            hit.insert(rank);
            let tier = tiers.partition_point(|&start| start <= rank);
            placed.insert((
                tier,
                std::cmp::Reverse(align.clamp(1, 16)),
                rank,
                section.to_string(),
            ));
        }
    }
    // A section holds one function; two keys naming it keep the first place.
    let mut seen = BTreeSet::new();
    let sections = placed
        .into_iter()
        .filter(|(.., section)| seen.insert(section.clone()))
        .map(|(.., section)| section)
        .collect();
    Placement {
        sections,
        listed: ranks.len(),
        resolved: hit.len(),
        core_listed: core as usize,
        core_resolved: hit.range(..core).count(),
    }
}

/// The output section of the hot run under lld (see the module notes).
pub(super) const LLD_HOT_SECTION: &str = ".text.perry_hot";

/// The text rules and the JS text rule every ELF link passes.
#[cfg(test)]
fn script(placement: &Placement, lld: bool) -> String {
    let mut out = text_rules(placement, lld);
    out.push_str(perry_codegen::function_order::JS_TEXT_LINK_RULE);
    out
}

/// The text rules: one `.text` rule that takes the placed sections, in
/// order, and then everything the default rule takes, in the default order
/// (so the default rule that follows it is left empty and `.text` stays one
/// output section). The two symbols bound the run for
/// `scripts/hot_text_order.py` (symbol-table entries only: a stripped binary
/// carries nothing of them).
fn text_rules(placement: &Placement, lld: bool) -> String {
    let mut out = format!(
        "/* perry default hot-text order: listed={} resolved={} core={}/{} sections={} */\nSECTIONS {{\n {} : {{\n  __perry_rt_hot_text_start = .;\n",
        placement.listed,
        placement.resolved,
        placement.core_resolved,
        placement.core_listed,
        placement.sections.len(),
        if lld { LLD_HOT_SECTION } else { ".text" }
    );
    for section in &placement.sections {
        out.push_str("  *(");
        out.push_str(section);
        out.push_str(")\n");
    }
    if lld {
        out.push_str("  __perry_rt_hot_text_end = .;\n }\n} INSERT BEFORE .text;\n");
        return out;
    }
    out.push_str(
        "  __perry_rt_hot_text_end = .;\n  *(.text.unlikely .text.*_unlikely .text.unlikely.*)\n  *(.text.exit .text.exit.*)\n  *(.text.startup .text.startup.*)\n  *(.text.hot .text.hot.*)\n  *(SORT(.text.sorted.*))\n  *(.text .stub .text.* .gnu.linkonce.t.*)\n }\n} INSERT BEFORE .text;\n",
    );
    out
}

/// Read the function symbols of every ELF member of `archive` with the name
/// and the alignment of the input section each one lives in.
fn archive_functions(archive: &Path, out: &mut Vec<(String, String, u64)>) -> Option<()> {
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
            let (Ok(name), Ok(section)) = (symbol.name(), object.section_by_index(index)) else {
                continue;
            };
            let Ok(section_name) = section.name() else {
                continue;
            };
            if section_name.starts_with(".text.") {
                out.push((name.to_string(), section_name.to_string(), section.align()));
            }
        }
    }
    Some(())
}

/// The cache identity of a resolved script: the list and each archive's path,
/// size and modification time.
fn fingerprint(archives: &[&Path], text: bool, data: Option<bool>) -> Option<u64> {
    // FNV-1a: stable across processes and toolchains, unlike `DefaultHasher`.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |bytes: &[u8]| {
        for b in bytes.iter().chain(std::iter::once(&0xff)) {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    feed(DEFAULT_ORDER.as_bytes());
    // Today's name for today's script: the data order and the text switch
    // enter only when the data order is on.
    if let Some(rodata) = data {
        feed(super::hot_data_order::DEFAULT_ORDER.as_bytes());
        feed(&[u8::from(text), u8::from(rodata)]);
    }
    for archive in archives {
        let meta = std::fs::metadata(archive).ok()?;
        let modified = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
        feed(archive.as_os_str().as_encoded_bytes());
        feed(&meta.len().to_le_bytes());
        feed(&modified.as_nanos().to_le_bytes());
    }
    Some(h)
}

/// The link script of a native ELF link that takes the default orders: the
/// hot-text run of `archives` (the runtime archives the link may pull from),
/// when `data_order` the hot-data runs (`hot_data_order`; `rodata` where the
/// `.rodata` rule can be placed), and the JS text rule. `None` keeps today's
/// script: both switches are off, the program has its own order, or an
/// archive cannot be read. Never an error: the orders are layout only.
pub(super) fn link_script(
    cache: &Path,
    archives: &[&Path],
    lld: bool,
    data_order: bool,
    rodata: bool,
    verbose: u8,
) -> Option<PathBuf> {
    let text = enabled();
    let data = data_order && super::hot_data_order::enabled();
    if !(text || data) || archives.is_empty() {
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
    let rodata = rodata || lld;
    let fingerprint = fingerprint(archives, text, data.then_some(rodata))?;
    let dir = cache.join("function-order");
    let flavor = if lld { ".lld" } else { "" };
    let path = dir.join(format!("hot-text-{fingerprint:016x}.v1{flavor}.ld"));
    if !path.is_file() {
        let mut out = String::new();
        if text {
            let (ranks, tiers) = parse_order(DEFAULT_ORDER);
            let mut functions = Vec::new();
            for archive in archives {
                archive_functions(archive, &mut functions)?;
            }
            let placement = place(
                &ranks,
                &tiers,
                functions
                    .iter()
                    .map(|(n, s, a)| (n.as_str(), s.as_str(), *a)),
            );
            if !placement.sections.is_empty() {
                out.push_str(&text_rules(&placement, lld));
            }
        }
        if data {
            let placement = super::hot_data_order::placement(archives)?;
            if !placement.is_empty() {
                out.push_str(&super::hot_data_order::rules(&placement, lld, rodata));
            }
        }
        if out.is_empty() {
            return None;
        }
        out.push_str(perry_codegen::function_order::JS_TEXT_LINK_RULE);
        // Concurrent links share the cache: write a sibling, then rename.
        let tmp = path.with_extension(format!("tmp{}", std::process::id()));
        if std::fs::create_dir_all(&dir).is_err()
            || std::fs::write(&tmp, out).is_err()
            || std::fs::rename(&tmp, &path).is_err()
        {
            let _ = std::fs::remove_file(&tmp);
            return None;
        }
    }
    if verbose > 0 {
        // The summary comments say how much of each list the archives hold.
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        for summary in text
            .lines()
            .filter(|l| l.starts_with("/* perry default hot-"))
        {
            let tag = if summary.contains("hot-data") {
                "hot-data"
            } else {
                "hot-text"
            };
            eprintln!("[{tag}] {}: {summary}", path.display());
        }
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
        let (ranks, tiers) = parse_order(
            "# tier core\n_RNvNtCs_13perry_runtime1b3two\n# tier common\njs_one\n\ngone_function\n_RNvNtCs_13perry_runtime1b3two\n",
        );
        assert_eq!((ranks.len(), tiers.as_slice()), (3, &[0, 1][..]));
        let symbols = [
            ("js_unlisted", ".text.js_unlisted", 16),
            ("js_one", ".text.js_one", 16),
            (
                "_RNvNtCsAAAAAAAAAAA_13perry_runtime1b3two",
                ".text._RNvNtCsAAAAAAAAAAA_13perry_runtime1b3two",
                16,
            ),
            // The same function in a second archive built with another hash.
            (
                "_RNvNtCsBBBBBBBBBB_13perry_runtime1b3two",
                ".text.unlikely._RNvNtCsBBBBBBBBBB_13perry_runtime1b3two",
                16,
            ),
            // Not a function section of its own: cannot be placed.
            ("js_one", ".text", 16),
            ("js_one", ".text.a*b", 16),
        ];
        let placement = place(&ranks, &tiers, symbols.iter().copied());
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
        // lld: the same run in its own output section, inserted before the
        // linker's `.text`, which keeps the default patterns itself.
        let lld = script(&placement, true);
        assert!(
            lld.contains("SECTIONS {\n .text.perry_hot : {\n  __perry_rt_hot_text_start = .;\n")
        );
        assert!(!lld.contains(" .text : {"));
        assert!(!lld.contains("*(.text .stub .text.* .gnu.linkonce.t.*)"));
        assert!(
            lld.find("*(.text._RNvNtCsAAAAAAAAAAA_").unwrap()
                < lld.find("*(.text.js_one)").unwrap()
        );
        assert!(lld.ends_with(perry_codegen::function_order::JS_TEXT_LINK_RULE));
        let script = script(&placement, false);
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
    fn a_tiers_low_aligned_sections_close_it_widest_first() {
        let (ranks, tiers) = parse_order("# tier core\na\nb\nc\nd\n# tier common\ne\nf\n");
        assert_eq!(tiers, [0, 4]);
        let symbols = [
            ("a", ".text.a", 1),
            ("b", ".text.b", 16),
            ("c", ".text.c", 8),
            ("d", ".text.d", 32),
            ("e", ".text.e", 4),
            ("f", ".text.f", 16),
        ];
        let placement = place(&ranks, &tiers, symbols.iter().copied());
        assert_eq!(
            placement.sections,
            [".text.b", ".text.d", ".text.c", ".text.a", ".text.f", ".text.e"]
        );
        assert_eq!((placement.core_listed, placement.core_resolved), (4, 4));
    }

    #[test]
    fn the_checked_in_order_is_a_list_of_keys() {
        let (ranks, tiers) = parse_order(DEFAULT_ORDER);
        assert!(ranks.len() > 500, "the default order is not a stub");
        assert!(
            tiers.len() > 1 && tiers[1] > 100 && (tiers[1] as usize) < ranks.len(),
            "tiers: {tiers:?}"
        );
        for key in ranks.keys() {
            assert_eq!(symbol_key(key), *key, "not a hash-free key: {key}");
            assert!(
                literal_section(&format!(".text.{key}")),
                "not a section-safe name: {key}"
            );
        }
    }
}
