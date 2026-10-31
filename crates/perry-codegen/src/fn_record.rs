//! Function metadata records: one per body that publishes a `JsFunctionInfo`,
//! tied to the body's own input section so the link keeps a record exactly
//! when it keeps the body (DCE layer 0).
//!
//! A record is the info's self-relative offset. Readers walk the image's
//! records section between its linker-defined bounds, which hold only the
//! records of bodies the link kept; no table names the bodies.
//!
//! | object format | the record lives and dies with its body through |
//! |---|---|
//! | ELF | `SHF_LINK_ORDER` to the body's section (`!associated`) |
//! | Mach-O | `live_support`: kept iff it references a live atom; the record also names the code, so it is kept exactly when the code is |
//! | COFF | the body's COMDAT, as an associative section (attached right before emission, `inprocess::function_layout`) |
//! | wasm | nothing (wasm-ld has no such section kind): every record stays, as the old table did |
//!
//! Every image also defines one zero record (the sentinel) and a bounds
//! descriptor that each module's metadata owner names (a self-relative
//! offset, so it costs registration nothing):
//! the sentinel guarantees the section exists in an image whose every record
//! was removed, and readers skip a zero record (also COFF's `$a` marker and
//! any alignment padding).

/// The records section's object-format flavour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RecordFormat {
    Elf,
    MachO,
    Coff,
    Wasm,
}

impl RecordFormat {
    pub(crate) fn of(triple: &str) -> Self {
        if triple.contains("apple") {
            Self::MachO
        } else if triple.contains("windows") {
            Self::Coff
        } else if triple.starts_with("wasm") {
            Self::Wasm
        } else {
            Self::Elf
        }
    }
}

/// `body`'s record as an IR global name. Records carry no symbol into the
/// link: ELF/COFF/wasm records are `private`; a Mach-O record needs a symbol
/// to be its own atom, and an `l`-prefixed one is linker-private (ld64 drops
/// it from the output).
pub(crate) fn record_global(format: RecordFormat, body: &str) -> String {
    match format {
        RecordFormat::MachO => format!("@\"\\01l_{body}$fnrec\""),
        _ => format!("@{body}$fnrec"),
    }
}

/// The body a record line belongs to, for the codegen-unit splitter: a record
/// must be emitted in the unit that defines its body (ELF links the record's
/// section to the body's section, which only exists in that object), and it
/// is emitted exactly as written (nothing outside that unit names it).
pub(crate) fn record_body(line: &str) -> Option<&str> {
    let name = line.strip_prefix('@')?.split_once(" = ")?.0;
    let name = match name.strip_prefix("\"\\01l_") {
        Some(quoted) => quoted.strip_suffix('"')?,
        None => name,
    };
    name.strip_suffix("$fnrec")
}

/// Keep `lines`' records through the optimizer: nothing in the IR names a
/// record, and GlobalDCE would delete it. `llvm.compiler.used` protects it
/// from the compiler only; the link still drops it with its body.
pub(crate) fn push_compiler_used<'a>(ir: &mut String, lines: impl Iterator<Item = &'a str>) {
    let names: Vec<&str> = lines
        .filter(|line| record_body(line).is_some())
        .filter_map(|line| line.split_once(" = ").map(|(name, _)| name))
        .collect();
    if names.is_empty() {
        return;
    }
    let list: Vec<String> = names.iter().map(|name| format!("ptr {name}")).collect();
    ir.push_str(&format!(
        "@llvm.compiler.used = appending global [{} x ptr] [{}], section \"llvm.metadata\"\n",
        names.len(),
        list.join(", ")
    ));
}

/// The body of a record global's IR name (`<body>$fnrec`), for the passes
/// that see the final LLVM module rather than its text.
pub(crate) fn body_of_record_name(name: &str) -> Option<&str> {
    name.strip_prefix("\u{1}l_").unwrap_or(name).strip_suffix("$fnrec")
}

/// Bounds descriptor every registering module's owner names.
pub(crate) const BOUNDS_SYMBOL: &str = "perry_fnrec_bounds";
const SENTINEL_SYMBOL: &str = "perry_fnrec_sentinel";

fn section(format: RecordFormat) -> &'static str {
    match format {
        RecordFormat::Elf | RecordFormat::Wasm => "perry_fnrec",
        RecordFormat::MachO => "__TEXT,__perry_fnrec,regular,live_support",
        RecordFormat::Coff => ".perryfn$m",
    }
}

/// `target - from` as an `i32` constant; both operands are pointer constants
/// (`@sym` or a constant `getelementptr`).
fn rel(target: &str, from: &str) -> String {
    format!("i32 trunc (i64 sub (i64 ptrtoint (ptr {target} to i64), i64 ptrtoint (ptr {from} to i64)) to i32)")
}

/// `body`'s record definition.
pub(crate) fn record_line(format: RecordFormat, body: &str) -> String {
    let rec = record_global(format, body);
    let info = format!("@{}", crate::fn_info::info_symbol(body));
    let section = section(format);
    match format {
        RecordFormat::Elf => format!(
            "{rec} = private constant {}, section \"{section}\", align 4, !associated !{{ptr @{body}}}",
            rel(&info, &rec)
        ),
        RecordFormat::Wasm => format!(
            "{rec} = private constant {}, section \"{section}\", align 4",
            rel(&info, &rec)
        ),
        // The code word is what `live_support` keeps the record by.
        RecordFormat::MachO => format!(
            "{rec} = internal constant {{ i32, i32 }} {{ {}, {} }}, section \"{section}\", align 4",
            rel(&info, &rec),
            rel(
                &format!("@{body}"),
                &format!("getelementptr inbounds ({{ i32, i32 }}, ptr {rec}, i32 0, i32 1)")
            ),
        ),
        RecordFormat::Coff => format!(
            "{rec} = private constant {}, section \"{section}\", align 4",
            rel(&info, &rec)
        ),
    }
}

/// The image-level lines: `(symbol, line)`. External declarations are plain
/// lines; definitions are coalesced to one per image.
pub(crate) fn image_lines(format: RecordFormat) -> Vec<(String, String)> {
    let section = section(format);
    let bounds = |start: &str, stop: &str| {
        (
            BOUNDS_SYMBOL.to_string(),
            format!(
                "@{BOUNDS_SYMBOL} = linkonce_odr hidden constant {{ ptr, ptr, ptr }} {{ ptr {start}, ptr {stop}, ptr @{SENTINEL_SYMBOL} }}, align 8"
            ),
        )
    };
    match format {
        RecordFormat::Elf | RecordFormat::Wasm => vec![
            (
                "__start_perry_fnrec".into(),
                "@__start_perry_fnrec = external hidden global i8".into(),
            ),
            (
                "__stop_perry_fnrec".into(),
                "@__stop_perry_fnrec = external hidden global i8".into(),
            ),
            (
                SENTINEL_SYMBOL.into(),
                format!("@{SENTINEL_SYMBOL} = linkonce_odr hidden constant i32 0, section \"{section}\", align 4"),
            ),
            bounds("@__start_perry_fnrec", "@__stop_perry_fnrec"),
        ],
        // ld64 synthesizes `section$start$SEG$SECT` only under that exact
        // name (the leading \01 stops the `_` prefix).
        RecordFormat::MachO => vec![
            (
                "\"\\01section$start$__TEXT$__perry_fnrec\"".into(),
                "@\"\\01section$start$__TEXT$__perry_fnrec\" = external hidden global i8".into(),
            ),
            (
                "\"\\01section$end$__TEXT$__perry_fnrec\"".into(),
                "@\"\\01section$end$__TEXT$__perry_fnrec\" = external hidden global i8".into(),
            ),
            (
                SENTINEL_SYMBOL.into(),
                format!("@{SENTINEL_SYMBOL} = linkonce_odr hidden constant {{ i32, i32 }} zeroinitializer, section \"{section}\", align 4"),
            ),
            bounds(
                "@\"\\01section$start$__TEXT$__perry_fnrec\"",
                "@\"\\01section$end$__TEXT$__perry_fnrec\"",
            ),
        ],
        // The linker sorts `.perryfn$a` < `$m` < `$z` within `.perryfn`: the
        // two markers bound the records (the `$a` marker is itself a zero
        // record, and so is the sentinel).
        RecordFormat::Coff => vec![
            (
                "perry_fnrec_a".into(),
                "@perry_fnrec_a = linkonce_odr hidden constant i32 0, section \".perryfn$a\", align 4".into(),
            ),
            (
                "perry_fnrec_z".into(),
                "@perry_fnrec_z = linkonce_odr hidden constant i32 0, section \".perryfn$z\", align 4".into(),
            ),
            (
                SENTINEL_SYMBOL.into(),
                format!("@{SENTINEL_SYMBOL} = linkonce_odr hidden constant i32 0, section \"{section}\", align 4"),
            ),
            bounds("@perry_fnrec_a", "@perry_fnrec_z"),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_names_its_body_for_the_unit_splitter() {
        for format in [RecordFormat::Elf, RecordFormat::MachO, RecordFormat::Coff, RecordFormat::Wasm] {
            let line = record_line(format, "__perry_wrap_m__f");
            assert_eq!(record_body(&line), Some("__perry_wrap_m__f"), "{line}");
        }
        assert_eq!(record_body("@__perry_wrap_m__f$info = internal constant i32 0"), None);
    }

    #[test]
    fn elf_records_are_tied_to_their_body_and_macho_records_name_the_code() {
        let elf = record_line(RecordFormat::Elf, "b");
        assert!(elf.ends_with(", !associated !{ptr @b}"), "{elf}");
        let macho = record_line(RecordFormat::MachO, "b");
        assert!(macho.contains("live_support"), "{macho}");
        assert!(macho.contains("ptrtoint (ptr @b to i64), i64 ptrtoint (ptr getelementptr inbounds ({ i32, i32 }, ptr @\"\\01l_b$fnrec\", i32 0, i32 1) to i64)"), "{macho}");
        let coff = record_line(RecordFormat::Coff, "b");
        assert!(coff.contains("section \".perryfn$m\""), "{coff}");
    }
}
