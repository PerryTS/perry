//! The default hot-text order on Mach-O (macOS, iOS, tvOS, watchOS,
//! visionOS): the same list as the ELF order (`hot_text_order`), resolved
//! against the Mach-O runtime archives of the link and passed to ld64 (or
//! ld64.lld) as an `-order_file`.
//!
//! ld64 lays out the atoms an order file names first in their section, in
//! file order, and takes exact symbol names, so the hash-free list keys are
//! resolved to the archives' own names before the link (the leading `_` of
//! Mach-O symbols does not enter the key). A key the archives no longer hold
//! writes no line; a name that the link does not pull in is ignored by the
//! linker. Nothing else changes: no IR, no object, no section.
//!
//! OFF by default: the gain on Apple targets (16 KiB pages, a different
//! fault-around) is not measured yet. `PERRY_HOT_TEXT_ORDER=on` (or `1`)
//! turns it on; unset or `off` keeps today's layout.

use object::read::archive::ArchiveFile;
use object::{Object, ObjectSymbol, SymbolKind};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

/// Opt-in only: `PERRY_HOT_TEXT_ORDER=on` (`1`, `true`, `yes`).
fn enabled() -> bool {
    enabled_by(std::env::var("PERRY_HOT_TEXT_ORDER").ok().as_deref())
}

fn enabled_by(value: Option<&str>) -> bool {
    matches!(value.map(str::trim), Some("on" | "1" | "true" | "yes"))
}

/// The defined text symbols of every Mach-O member of `archive`, as the
/// archive spells them (with the leading `_`).
fn archive_functions(archive: &Path, out: &mut Vec<String>) -> Option<()> {
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
        if object.format() != object::BinaryFormat::MachO {
            continue;
        }
        for symbol in object.symbols() {
            if symbol.kind() != SymbolKind::Text
                || symbol.is_undefined()
                || symbol.section_index().is_none()
            {
                continue;
            }
            if let Ok(name) = symbol.name() {
                out.push(name.to_string());
            }
        }
    }
    Some(())
}

/// The list key of a Mach-O symbol: the ELF key of its name without the
/// leading `_`. `None` for a name an order file cannot carry (assembler
/// locals, names with blanks or a `#`).
fn macho_key(symbol: &str) -> Option<String> {
    let name = symbol.strip_prefix('_')?;
    if name.is_empty() || symbol.bytes().any(|b| b.is_ascii_whitespace() || b == b'#') {
        return None;
    }
    Some(super::hot_text_order::symbol_key(name))
}

/// The order file's lines: every archive symbol the list names, by tier
/// and rank (one line per name), and how much of the list resolved.
#[derive(Debug, Default, PartialEq, Eq)]
struct Placement {
    names: Vec<String>,
    listed: usize,
    resolved: usize,
    core_listed: usize,
    core_resolved: usize,
}

fn place<'a>(
    ranks: &HashMap<&str, u32>,
    tiers: &[u32],
    symbols: impl Iterator<Item = &'a str>,
) -> Placement {
    let core = tiers.get(1).copied().unwrap_or(ranks.len() as u32);
    let mut placed: BTreeSet<(u32, String)> = BTreeSet::new();
    let mut hit: BTreeSet<u32> = BTreeSet::new();
    for symbol in symbols {
        let Some(key) = macho_key(symbol) else {
            continue;
        };
        if let Some(&rank) = ranks.get(key.as_str()) {
            hit.insert(rank);
            placed.insert((rank, symbol.to_string()));
        }
    }
    let mut seen = BTreeSet::new();
    let names = placed
        .into_iter()
        .filter(|(_, name)| seen.insert(name.clone()))
        .map(|(_, name)| name)
        .collect();
    Placement {
        names,
        listed: ranks.len(),
        resolved: hit.len(),
        core_listed: core as usize,
        core_resolved: hit.range(..core).count(),
    }
}

fn order_file(placement: &Placement) -> String {
    let mut out = format!(
        "# perry default hot-text order (ld64): listed={} resolved={} core={}/{} symbols={}\n",
        placement.listed,
        placement.resolved,
        placement.core_resolved,
        placement.core_listed,
        placement.names.len()
    );
    for name in &placement.names {
        out.push_str(name);
        out.push('\n');
    }
    out
}

/// The `-order_file` of a Mach-O link that takes the default order (opt-in,
/// see the module notes), kept in the compile cache per archive identity.
/// `None` keeps today's link: the switch is not on, the program has its own
/// order, or an archive cannot be read.
pub(super) fn order_file_path(cache: &Path, archives: &[&Path], verbose: u8) -> Option<PathBuf> {
    if !enabled() || archives.is_empty() {
        return None;
    }
    if !matches!(
        perry_codegen::program_function_layout(),
        perry_codegen::FunctionLayout::Default
    ) {
        return None;
    }
    // The text-only identity: the ELF script of the same archives carries
    // another extension.
    let fingerprint = super::hot_text_order::fingerprint(archives, true, None)?;
    let dir = cache.join("function-order");
    let path = dir.join(format!("hot-text-{fingerprint:016x}.v1.ld64.txt"));
    if !path.is_file() {
        let (ranks, tiers) =
            super::hot_text_order::parse_order(super::hot_text_order::DEFAULT_ORDER);
        let mut symbols = Vec::new();
        for archive in archives {
            archive_functions(archive, &mut symbols)?;
        }
        let placement = place(&ranks, &tiers, symbols.iter().map(String::as_str));
        if placement.names.is_empty() {
            return None;
        }
        let tmp = path.with_extension(format!("tmp{}", std::process::id()));
        if std::fs::create_dir_all(&dir).is_err()
            || std::fs::write(&tmp, order_file(&placement)).is_err()
            || std::fs::rename(&tmp, &path).is_err()
        {
            let _ = std::fs::remove_file(&tmp);
            return None;
        }
    }
    if verbose > 0 {
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        eprintln!(
            "[hot-text] {}: {}",
            path.display(),
            text.lines().next().unwrap_or_default()
        );
    }
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal 64-bit Mach-O relocatable object (arm64) with one
    /// `__TEXT,__text` section that defines `symbols` (external, in order,
    /// 4 bytes each) and one undefined symbol `_undefined_callee`.
    fn macho_object(symbols: &[&str]) -> Vec<u8> {
        fn u32le(out: &mut Vec<u8>, v: u32) {
            out.extend_from_slice(&v.to_le_bytes());
        }
        fn u64le(out: &mut Vec<u8>, v: u64) {
            out.extend_from_slice(&v.to_le_bytes());
        }
        fn name16(out: &mut Vec<u8>, name: &str) {
            let mut b = [0u8; 16];
            b[..name.len()].copy_from_slice(name.as_bytes());
            out.extend_from_slice(&b);
        }
        let text_size = 4 * symbols.len() as u64;
        let seg_cmd = 72 + 80;
        let symtab_cmd = 24;
        let header = 32;
        let text_off = (header + seg_cmd + symtab_cmd) as u64;
        let sym_off = text_off + text_size;
        let mut strtab = vec![0u8];
        let mut nlist = Vec::new();
        let mut entries: Vec<(&str, u8, u8, u64)> = symbols
            .iter()
            .enumerate()
            .map(|(i, s)| (*s, 0x0f, 1, 4 * i as u64))
            .collect();
        entries.push(("_undefined_callee", 0x01, 0, 0));
        for (name, n_type, n_sect, value) in &entries {
            u32le(&mut nlist, strtab.len() as u32);
            nlist.push(*n_type);
            nlist.push(*n_sect);
            nlist.extend_from_slice(&0u16.to_le_bytes());
            u64le(&mut nlist, *value);
            strtab.extend_from_slice(name.as_bytes());
            strtab.push(0);
        }
        let str_off = sym_off + nlist.len() as u64;
        let mut out = Vec::new();
        // mach_header_64: MH_MAGIC_64, CPU_TYPE_ARM64, subtype, MH_OBJECT,
        // ncmds, sizeofcmds, MH_SUBSECTIONS_VIA_SYMBOLS, reserved.
        for v in [
            0xfeed_facf,
            0x0100_000c,
            0,
            1,
            2,
            (seg_cmd + symtab_cmd) as u32,
            0x2000,
            0,
        ] {
            u32le(&mut out, v);
        }
        // LC_SEGMENT_64 with one section.
        u32le(&mut out, 0x19);
        u32le(&mut out, seg_cmd as u32);
        name16(&mut out, "");
        for v in [0, text_size, text_off, text_size] {
            u64le(&mut out, v);
        }
        for v in [7, 7, 1, 0] {
            u32le(&mut out, v);
        }
        name16(&mut out, "__text");
        name16(&mut out, "__TEXT");
        u64le(&mut out, 0);
        u64le(&mut out, text_size);
        // offset, align (2^2), reloff, nreloc, flags (pure instructions), reserved x3.
        for v in [text_off as u32, 2, 0, 0, 0x8000_0400, 0, 0, 0] {
            u32le(&mut out, v);
        }
        // LC_SYMTAB.
        for v in [
            0x2,
            symtab_cmd as u32,
            sym_off as u32,
            entries.len() as u32,
            str_off as u32,
            strtab.len() as u32,
        ] {
            u32le(&mut out, v);
        }
        assert_eq!(out.len() as u64, text_off);
        out.extend(std::iter::repeat(0xd5).take(text_size as usize));
        out.extend_from_slice(&nlist);
        out.extend_from_slice(&strtab);
        out
    }

    /// A System V archive of `members` (name, bytes).
    fn archive(members: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let mut out = b"!<arch>\n".to_vec();
        for (name, bytes) in members {
            let header = format!(
                "{:<16}{:<12}{:<6}{:<6}{:<8}{:<10}`\n",
                format!("{name}/"),
                0,
                0,
                0,
                644,
                bytes.len()
            );
            assert_eq!(header.len(), 60);
            out.extend_from_slice(header.as_bytes());
            out.extend_from_slice(bytes);
            if bytes.len() % 2 == 1 {
                out.push(b'\n');
            }
        }
        out
    }

    const RT: &str = "_RNvNtNtCsXbwMRDrjNy_13perry_runtime2gc6policy25gc_budgeted_start_or_step";
    const RT_OTHER_HASH: &str =
        "_RNvNtNtCsklhtP6PwTF_13perry_runtime2gc6policy25gc_budgeted_start_or_step";

    #[test]
    fn mach_o_keys_drop_the_underscore_and_the_hashes() {
        assert_eq!(
            macho_key("_js_string_concat").as_deref(),
            Some("js_string_concat")
        );
        assert_eq!(
            macho_key(&format!("_{RT}")),
            macho_key(&format!("_{RT_OTHER_HASH}"))
        );
        assert_eq!(
            macho_key(&format!("_{RT}")).as_deref(),
            Some("_RNvNtNtCs_13perry_runtime2gc6policy25gc_budgeted_start_or_step")
        );
        // Assembler locals and names an order file cannot carry.
        assert_eq!(macho_key("ltmp0"), None);
        assert_eq!(macho_key("_"), None);
        assert_eq!(macho_key("_a b"), None);
        assert!(enabled_by(Some("on")) && enabled_by(Some("1")));
        assert!(!enabled_by(None) && !enabled_by(Some("off")) && !enabled_by(Some("")));
    }

    #[test]
    fn the_archive_resolves_the_list_to_exact_names_and_a_stale_key_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("libperry_runtime.a");
        let rt = format!("_{RT}");
        std::fs::write(
            &path,
            archive(&[
                ("a.o", macho_object(&["_js_unlisted", &rt])),
                ("b.o", macho_object(&["_js_one"])),
                // An ELF member (a stray object) is skipped.
                ("c.o", b"\x7fELF\x02\x01\x01".to_vec()),
            ]),
        )
        .unwrap();
        let mut symbols = Vec::new();
        archive_functions(&path, &mut symbols).expect("archive parses");
        symbols.sort();
        assert_eq!(
            symbols,
            [rt.as_str(), "_js_one", "_js_unlisted"],
            "defined text only"
        );

        // The list names the runtime function by its hash-free key, a function
        // that no longer exists, and `js_one` in the second tier.
        let list = "# tier core\n_RNvNtNtCs_13perry_runtime2gc6policy25gc_budgeted_start_or_step\ngone_function\n# tier common\njs_one\n";
        let (ranks, tiers) = super::super::hot_text_order::parse_order(list);
        let placement = place(&ranks, &tiers, symbols.iter().map(String::as_str));
        assert_eq!(placement.names, [rt.as_str(), "_js_one"]);
        assert_eq!((placement.listed, placement.resolved), (3, 2));
        assert_eq!((placement.core_listed, placement.core_resolved), (2, 1));
        let file = order_file(&placement);
        assert_eq!(
            file,
            format!("# perry default hot-text order (ld64): listed=3 resolved=2 core=1/2 symbols=2\n{rt}\n_js_one\n")
        );
        // Every symbol line carries the Mach-O underscore; the stale key
        // wrote nothing.
        assert!(file.lines().skip(1).all(|l| l.starts_with('_')));
        assert!(!file.contains("gone_function"));
    }

    #[test]
    fn the_checked_in_list_resolves_on_mach_o_names() {
        let (ranks, tiers) =
            super::super::hot_text_order::parse_order(super::super::hot_text_order::DEFAULT_ORDER);
        // Every listed key, spelled as a Mach-O symbol, maps back to itself.
        let names: Vec<String> = ranks.keys().map(|k| format!("_{k}")).collect();
        let placement = place(&ranks, &tiers, names.iter().map(String::as_str));
        assert_eq!(placement.resolved, ranks.len());
        assert_eq!(placement.core_resolved, placement.core_listed);
        assert!(placement.names.iter().all(|n| n.starts_with('_')));
    }
}
