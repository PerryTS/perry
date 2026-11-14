//! Default hot-data order for the runtime archives.
//!
//! The read-only data a program touches is spread over the runtime's
//! `.rodata` and `.data.rel.ro` the way its code was spread over `.text`
//! before the hot-text order (`hot_text_order`): a few hundred tables,
//! strings and vtables in archive order, and the kernel maps the whole
//! 64 KiB window around each first read. `hot_data_order.txt` lists the
//! runtime data sections a corpus of ordinary programs reads, most widely
//! read first (`scripts/hot_data_order.py` records them under lackey and
//! builds the list). The link places them at the front of `.rodata` and of
//! `.data.rel.ro`, in the same linker script as the hot-text run, so what a
//! program reads is packed and the rest stays unmapped.
//!
//! Layout only: no IR, no object, no call, no table changes; the binary holds
//! the same data bytes, in another order.
//!
//! * Each section is keyed without hashes ([`section_key`]): `r:` (`.rodata`)
//!   or `d:` (`.data.rel.ro`), then the hot-text key of the symbol the
//!   section is named after, or for an unnamed constant
//!   (`.rodata..Lanon.<hash>.<n>`, whose name changes with every build) `#`
//!   and a digest of its bytes and relocations. Merged sections
//!   (`.rodata.str1.1`, `.rodata.cst16`) are pooled by the linker and stay
//!   where it puts them. A key the archives no longer hold matches nothing;
//!   `scripts/hot_data_order.py check` is the gate.
//! * GNU ld: the `.rodata` rule must follow the page alignment that starts
//!   the read-only segment, and an `INSERT BEFORE .rodata` lands in front of
//!   it, so the rule repeats that alignment; the linker's own alignment then
//!   runs once more behind the rule (at most one page of padding before
//!   `.eh_frame_hdr`). That alignment exists in x86-64's default script
//!   (`-z separate-code`); elsewhere only `.data.rel.ro` is ordered.
//! * lld: the run is its own output section `.rodata.perry_hot` in front of
//!   `.rodata`, and the `.data.rel.ro` rule is inserted before `.dynamic`
//!   (lld refuses an output section inserted before itself and keeps RELRO by
//!   section name).
//! * Native glibc Linux executables only, the default function layout only.
//! * `PERRY_HOT_DATA_ORDER=off` keeps today's layout.

use object::read::archive::ArchiveFile;
use object::{
    Object, ObjectSection, ObjectSymbol, RelocationFlags, RelocationTarget, SectionFlags,
    SymbolKind,
};
use std::collections::{BTreeSet, HashMap};
use std::path::Path;

/// The checked-in default order: one [`section_key`] per line, hottest first.
pub(super) const DEFAULT_ORDER: &str = include_str!("hot_data_order.txt");

/// `PERRY_HOT_DATA_ORDER=off` (or `0`) keeps today's layout (A/B measurement).
pub(super) fn enabled() -> bool {
    !matches!(
        std::env::var("PERRY_HOT_DATA_ORDER").as_deref(),
        Ok("off" | "0" | "false" | "no")
    )
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv(mut h: u64, bytes: &[u8]) -> u64 {
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

const SHF_ALLOC: u64 = 0x2;
const SHF_MERGE: u64 = 0x10;

/// The output section letter (`r` = `.rodata`, `d` = `.data.rel.ro`) and the
/// symbol part of an orderable data section's name. Only literal names (a
/// linker pattern takes them as they are).
fn split_name(name: &str) -> Option<(char, &str)> {
    if !name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"_.$".contains(&b))
    {
        return None;
    }
    const PREFIXES: [(&str, char); 5] = [
        (".rodata.unlikely.", 'r'),
        (".rodata.hot.", 'r'),
        (".rodata.", 'r'),
        (".data.rel.ro.local.", 'd'),
        (".data.rel.ro.", 'd'),
    ];
    PREFIXES.iter().find_map(|(prefix, kind)| {
        let rest = name.strip_prefix(prefix)?;
        (!rest.is_empty()).then_some((*kind, rest))
    })
}

/// What a relocation of a data section points at: a named symbol (by its
/// hot-text key) or a section (by [`target_section_name`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Target {
    Symbol(String),
    Section(String),
}

/// The hash-free name of a relocation's target section: a function or data
/// section by its symbol's key, an unnamed or pooled one by its kind (the
/// part before `..L`). Not its bytes: a constant points into its CGU's string
/// pool, which changes with every edited string of the CGU.
fn target_section_name(name: &str) -> String {
    for prefix in [".text.unlikely.", ".text.hot.", ".text."] {
        if let Some(rest) = name.strip_prefix(prefix) {
            if !rest.is_empty() && !rest.starts_with(".L") {
                return format!("{prefix}{}", super::hot_text_order::symbol_key(rest));
            }
        }
    }
    if let Some((kind, rest)) = split_name(name) {
        if !rest.starts_with(".L") {
            return format!("{kind}:{}", super::hot_text_order::symbol_key(rest));
        }
    }
    name.find("..L").map_or(name, |at| &name[..at]).to_string()
}

/// One relocation: offset, ELF type, addend, target.
pub(super) type Reloc = (u64, u32, i64, Target);

/// The hash-free identity of a data section (`scripts/hot_data_order.py`
/// holds the twin). A named section is keyed by its symbol; an unnamed
/// constant by an FNV-1a digest of its size, its bytes and its relocations
/// in offset order: each by offset and type, then a named target by its key
/// and the addend, a section target by its hash-free name alone (the addend
/// is an offset into a pool that moves with every edit of the pool).
pub(super) fn section_key(name: &str, data: &[u8], size: u64, relocs: &[Reloc]) -> Option<String> {
    let (kind, rest) = split_name(name)?;
    if !rest.starts_with(".L") {
        return Some(format!(
            "{kind}:{}",
            super::hot_text_order::symbol_key(rest)
        ));
    }
    let mut h = fnv(FNV_OFFSET, &size.to_le_bytes());
    h = fnv(h, data);
    let mut sorted: Vec<&Reloc> = relocs.iter().collect();
    sorted.sort_by_key(|r| r.0);
    for (offset, r_type, addend, target) in sorted {
        h = fnv(h, &offset.to_le_bytes());
        h = fnv(h, &r_type.to_le_bytes());
        let (tag, name) = match target {
            Target::Symbol(key) => {
                h = fnv(h, &addend.to_le_bytes());
                (b"S", key)
            }
            Target::Section(name) => (b"#", name),
        };
        h = fnv(h, tag);
        h = fnv(h, name.as_bytes());
        h = fnv(h, b"\0");
    }
    Some(format!("{kind}:#{h:016x}"))
}

/// An orderable data section of an archive: key, input section name,
/// alignment.
pub(super) type DataSection = (String, String, u64);

/// Every orderable data section of every ELF member of `archive`.
pub(super) fn archive_sections(archive: &Path, out: &mut Vec<DataSection>) -> Option<()> {
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
        member_sections(&object, out);
    }
    Some(())
}

fn member_sections(object: &object::File, out: &mut Vec<DataSection>) {
    let mut targets: HashMap<object::SectionIndex, String> = HashMap::new();
    for section in object.sections() {
        let SectionFlags::Elf { sh_flags } = section.flags() else {
            continue;
        };
        if sh_flags & SHF_ALLOC == 0 || sh_flags & SHF_MERGE != 0 {
            continue;
        }
        let Ok(name) = section.name() else { continue };
        if split_name(name).is_none() {
            continue;
        }
        let bytes = section.data().unwrap_or_default();
        let mut relocs = Vec::new();
        for (offset, reloc) in section.relocations() {
            let RelocationFlags::Elf { r_type } = reloc.flags() else {
                continue;
            };
            let target = match reloc.target() {
                RelocationTarget::Symbol(index) => match object.symbol_by_index(index) {
                    Ok(symbol) => {
                        let name = symbol.name().unwrap_or_default();
                        if !name.is_empty() && symbol.kind() != SymbolKind::Section {
                            Target::Symbol(super::hot_text_order::symbol_key(name))
                        } else if let Some(index) = symbol.section_index() {
                            Target::Section(
                                targets
                                    .entry(index)
                                    .or_insert_with(|| {
                                        object
                                            .section_by_index(index)
                                            .ok()
                                            .and_then(|s| s.name().ok().map(target_section_name))
                                            .unwrap_or_default()
                                    })
                                    .clone(),
                            )
                        } else {
                            Target::Section(String::new())
                        }
                    }
                    Err(_) => Target::Section(String::new()),
                },
                _ => Target::Section(String::new()),
            };
            relocs.push((offset, r_type, reloc.addend(), target));
        }
        if let Some(key) = section_key(name, bytes, section.size(), &relocs) {
            out.push((key, name.to_string(), section.align()));
        }
    }
}

/// The list: key -> rank and the rank each tier starts at (as the hot-text
/// list).
fn parse_order(text: &str) -> (HashMap<&str, u32>, Vec<u32>) {
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

/// The placed input sections of each output section, and how much of the
/// list the archives hold.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Placement {
    pub(super) rodata: Vec<String>,
    pub(super) relro: Vec<String>,
    listed: usize,
    resolved: usize,
    core_listed: usize,
    core_resolved: usize,
}

impl Placement {
    pub(super) fn is_empty(&self) -> bool {
        self.rodata.is_empty() && self.relro.is_empty()
    }
}

/// Place every section the list names: by tier; inside a tier the widest
/// alignment first and then by rank, so the run packs with as little padding
/// as the unordered layout.
fn place<'a>(
    ranks: &HashMap<&str, u32>,
    tiers: &[u32],
    sections: impl Iterator<Item = (&'a str, &'a str, u64)>,
) -> Placement {
    let core = tiers.get(1).copied().unwrap_or(ranks.len() as u32);
    let mut placed: BTreeSet<(usize, std::cmp::Reverse<u64>, u32, String)> = BTreeSet::new();
    let mut hit: BTreeSet<u32> = BTreeSet::new();
    for (key, name, align) in sections {
        if let Some(&rank) = ranks.get(key) {
            hit.insert(rank);
            let tier = tiers.partition_point(|&start| start <= rank);
            placed.insert((
                tier,
                std::cmp::Reverse(align.clamp(1, 64)),
                rank,
                name.to_string(),
            ));
        }
    }
    let mut seen = BTreeSet::new();
    let mut placement = Placement {
        listed: ranks.len(),
        resolved: hit.len(),
        core_listed: core as usize,
        core_resolved: hit.range(..core).count(),
        ..Placement::default()
    };
    for (.., name) in placed {
        if !seen.insert(name.clone()) {
            continue;
        }
        match split_name(&name) {
            Some(('r', _)) => placement.rodata.push(name),
            Some(_) => placement.relro.push(name),
            None => {}
        }
    }
    placement
}

/// The default `.rodata` and `.data.rel.ro` input patterns of GNU ld's script.
const RODATA_DEFAULT: &str = "  *(.rodata .rodata.* .gnu.linkonce.r.*)\n";
const RELRO_DEFAULT: &str = "  *(.data.rel.ro.local* .gnu.linkonce.d.rel.ro.local.*)\n  *(.data.rel.ro .data.rel.ro.* .gnu.linkonce.d.rel.ro.*)\n";
/// The page alignment GNU ld's x86-64 script puts in front of `.rodata`.
const RODATA_SEGMENT_START: &str = " . = ALIGN(CONSTANT (MAXPAGESIZE));\n . = SEGMENT_START(\"rodata-segment\", ALIGN(CONSTANT (MAXPAGESIZE)) + (. & (CONSTANT (MAXPAGESIZE) - 1)));\n";

fn run(out: &mut String, name: &str, sections: &[String]) {
    out.push_str(&format!("  __perry_rt_hot_{name}_start = .;\n"));
    for section in sections {
        out.push_str("  *(");
        out.push_str(section);
        out.push_str(")\n");
    }
    out.push_str(&format!("  __perry_rt_hot_{name}_end = .;\n"));
}

/// The linker-script rules of the data runs (without the JS text rule),
/// starting with a one-line summary comment. `rodata` is false where GNU
/// ld's script has no page alignment in front of `.rodata` to repeat.
pub(super) fn rules(placement: &Placement, lld: bool, rodata: bool) -> String {
    let rodata = rodata && !placement.rodata.is_empty();
    let mut out = format!(
        "/* perry default hot-data order: listed={} resolved={} core={}/{} rodata={} relro={} */\n",
        placement.listed,
        placement.resolved,
        placement.core_resolved,
        placement.core_listed,
        if rodata { placement.rodata.len() } else { 0 },
        placement.relro.len(),
    );
    if rodata {
        if lld {
            out.push_str("SECTIONS {\n .rodata.perry_hot : {\n");
            run(&mut out, "rodata", &placement.rodata);
            out.push_str(" }\n} INSERT BEFORE .rodata;\n");
        } else {
            out.push_str("SECTIONS {\n");
            out.push_str(RODATA_SEGMENT_START);
            out.push_str(" .rodata : {\n");
            run(&mut out, "rodata", &placement.rodata);
            out.push_str(RODATA_DEFAULT);
            out.push_str(" }\n} INSERT BEFORE .rodata;\n");
        }
    }
    if !placement.relro.is_empty() {
        out.push_str("SECTIONS {\n .data.rel.ro : {\n");
        run(&mut out, "relro", &placement.relro);
        out.push_str(RELRO_DEFAULT);
        out.push_str(if lld {
            " }\n} INSERT BEFORE .dynamic;\n"
        } else {
            " }\n} INSERT BEFORE .data.rel.ro;\n"
        });
    }
    out
}

/// The placement of the default order against `archives`; `None` when an
/// archive cannot be read.
pub(super) fn placement(archives: &[&Path]) -> Option<Placement> {
    let (ranks, tiers) = parse_order(DEFAULT_ORDER);
    let mut sections = Vec::new();
    for archive in archives {
        archive_sections(archive, &mut sections)?;
    }
    Some(place(
        &ranks,
        &tiers,
        sections
            .iter()
            .map(|(k, n, a)| (k.as_str(), n.as_str(), *a)),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The vectors `scripts/hot_data_order.py selftest` checks too.
    #[test]
    fn keys_name_the_symbol_or_digest_an_unnamed_constant() {
        let cases: &[(&str, &[u8], u64, Vec<Reloc>, Option<&str>)] = &[
            (
                ".rodata._RNvNtNtCsXbwMRDrjNy_13perry_runtime2gc5types18GC_TYPE_INFO",
                b"",
                4096,
                vec![],
                Some("r:_RNvNtNtCs_13perry_runtime2gc5types18GC_TYPE_INFO"),
            ),
            (
                ".data.rel.ro.local._mi_theap_empty",
                b"",
                8096,
                vec![],
                Some("d:_mi_theap_empty"),
            ),
            (
                ".rodata.unlikely._RNvNtCsAB_5crate3TAB",
                b"",
                4,
                vec![],
                Some("r:_RNvNtCs_5crate3TAB"),
            ),
            (
                ".rodata..Lanon.066df2a3b4d7718a1445f540d0b35fcf.717",
                b"length",
                6,
                vec![],
                Some("r:#0cc57c9be00b204b"),
            ),
            (
                ".data.rel.ro..Lanon.066df2a3b4d7718a1445f540d0b35fcf.12",
                &[0; 16],
                16,
                vec![
                    (8, 1, 0, Target::Symbol("js_string_concat".into())),
                    (0, 1, 3, Target::Section(".rodata".into())),
                ],
                Some("d:#b9adc86cf57ea99a"),
            ),
            (".rodata", b"x", 1, vec![], None),
            (".rodata.perry_fn_info_x::y", b"", 1, vec![], None),
        ];
        for (name, data, size, relocs, want) in cases {
            assert_eq!(
                section_key(name, data, *size, relocs).as_deref(),
                *want,
                "{name}"
            );
        }
        // The anon index and hash in the name do not reach the key.
        assert_eq!(
            section_key(".rodata..Lanon.aaaa.1", b"length", 6, &[]),
            section_key(".rodata..Lanon.bbbb.2", b"length", 6, &[])
        );
        // A pointer into a pool: the offset moves when the pool changes, the
        // key does not.
        let into_pool = |addend| vec![(0, 1, addend, Target::Section(".rodata".into()))];
        assert_eq!(
            section_key(".data.rel.ro..Lanon.a.1", &[0; 8], 8, &into_pool(16)),
            section_key(".data.rel.ro..Lanon.b.9", &[0; 8], 8, &into_pool(48))
        );
        assert_eq!(target_section_name(".rodata..Lanon.abc.5"), ".rodata");
        assert_eq!(target_section_name(".rodata.str1.1"), "r:str1.1");
        assert_eq!(
            target_section_name(".text._RNvCsAB_5crate1f"),
            ".text._RNvCs_5crate1f"
        );
        assert_eq!(
            target_section_name(".data.rel.ro..Lanon.a.1"),
            ".data.rel.ro"
        );
        assert_ne!(
            section_key(".rodata..Lanon.aaaa.1", b"length", 6, &[]),
            section_key(".data.rel.ro..Lanon.aaaa.1", b"length", 6, &[])
        );
    }

    #[test]
    fn listed_sections_are_placed_by_tier_then_alignment_and_a_stale_key_is_harmless() {
        let (ranks, tiers) =
            parse_order("# tier core\nr:b\nd:#00000000000000aa\nr:a\n# tier common\nr:c\nr:gone\n");
        assert_eq!(tiers, [0, 3]);
        let sections = [
            ("r:a", ".rodata.a", 1),
            ("r:b", ".rodata.b", 8),
            ("d:#00000000000000aa", ".data.rel.ro..Lanon.x.1", 8),
            // The same constant in a second member: both are placed.
            ("d:#00000000000000aa", ".data.rel.ro..Lanon.y.7", 8),
            ("r:c", ".rodata.c", 64),
            ("r:unlisted", ".rodata.unlisted", 8),
        ];
        let placement = place(&ranks, &tiers, sections.iter().copied());
        assert_eq!(placement.rodata, [".rodata.b", ".rodata.a", ".rodata.c"]);
        assert_eq!(
            placement.relro,
            [".data.rel.ro..Lanon.x.1", ".data.rel.ro..Lanon.y.7"]
        );
        assert_eq!((placement.listed, placement.resolved), (5, 4));
        assert_eq!((placement.core_listed, placement.core_resolved), (3, 3));

        let gnu = rules(&placement, false, true);
        assert!(gnu.starts_with(
            "/* perry default hot-data order: listed=5 resolved=4 core=3/3 rodata=3 relro=2 */\n"
        ));
        // The rule repeats the segment's page alignment, takes the run, then
        // every default pattern, so the default rule behind it is empty.
        let seg = gnu.find("SEGMENT_START(\"rodata-segment\"").unwrap();
        let start = gnu.find("__perry_rt_hot_rodata_start").unwrap();
        let rest = gnu.find(RODATA_DEFAULT).unwrap();
        assert!(seg < start && start < gnu.find("*(.rodata.b)").unwrap());
        assert!(gnu.find("*(.rodata.c)").unwrap() < gnu.find("__perry_rt_hot_rodata_end").unwrap());
        assert!(gnu.find("__perry_rt_hot_rodata_end").unwrap() < rest);
        assert!(gnu.contains("} INSERT BEFORE .rodata;\n"));
        assert!(gnu.contains(RELRO_DEFAULT));
        assert!(gnu.ends_with("} INSERT BEFORE .data.rel.ro;\n"));
        // lld: the run in its own output section; RELRO before .dynamic.
        let lld = rules(&placement, true, true);
        assert!(lld.contains("SECTIONS {\n .rodata.perry_hot : {\n"));
        assert!(!lld.contains("SEGMENT_START") && !lld.contains(RODATA_DEFAULT));
        assert!(lld.ends_with("} INSERT BEFORE .dynamic;\n"));
        // Without the segment alignment to repeat: .data.rel.ro only.
        let relro_only = rules(&placement, false, false);
        assert!(!relro_only.contains(".rodata"), "{relro_only}");
        assert!(relro_only.contains("rodata=0 relro=2"));
        assert!(relro_only.contains("__perry_rt_hot_relro_start"));
    }

    #[test]
    fn only_allocated_unmerged_literal_sections_are_orderable() {
        assert_eq!(split_name(".rodata.x"), Some(('r', "x")));
        assert_eq!(split_name(".rodata..Lanon.a.1"), Some(('r', ".Lanon.a.1")));
        assert_eq!(split_name(".data.rel.ro.local.y"), Some(('d', "y")));
        assert_eq!(
            split_name(".data.rel.ro..Lanon.a.2"),
            Some(('d', ".Lanon.a.2"))
        );
        assert_eq!(split_name(".rodata"), None);
        assert_eq!(split_name(".rodata."), None);
        assert_eq!(split_name(".data.rel.ro"), None);
        assert_eq!(split_name(".text.f"), None);
        assert_eq!(split_name(".rodata.a b"), None);
    }

    #[test]
    fn the_checked_in_order_is_a_list_of_keys() {
        let (ranks, tiers) = parse_order(DEFAULT_ORDER);
        assert!(ranks.len() > 200, "the default order is not a stub");
        assert!(tiers.len() > 1, "tiers: {tiers:?}");
        for key in ranks.keys() {
            let (kind, rest) = key.split_once(':').expect("kind:key");
            assert!(matches!(kind, "r" | "d"), "{key}");
            assert!(
                rest.starts_with('#') && rest.len() == 17
                    || super::super::hot_text_order::symbol_key(rest) == rest,
                "not a hash-free key: {key}"
            );
        }
    }
}
