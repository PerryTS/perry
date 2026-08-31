//! Re-encode LLVM's stack-map section into Perry's compact GC map.
//!
//! # Why this exists
//!
//! `gc.statepoint` metadata is the statepoint backend's *only* losing axis
//! against the shadow stack. Measured on `test-drizzle-pg`: generated `__text`
//! is 248 KB **smaller** under statepoints, but `__llvm_stackmaps` adds 3.9 MB,
//! so the binary loses by 3.5 MB overall.
//!
//! Almost none of those bytes carry information an AOT collector can use.
//! Measured composition of that section:
//!
//! * **60% of all location slots are `Constant`** — exactly three per record,
//!   `gc.statepoint`'s calling-convention / flags / num-deopt preamble.
//! * every root is recorded as a **(base, derived) pair**, and Perry has no
//!   interior pointers, so half of the remainder is the same slot twice;
//! * each record carries a 16-byte header whose 8-byte **patchpoint ID** only
//!   matters to a JIT that patches call sites, plus inter-record padding.
//!
//! The runtime already threw all of that away at startup (see
//! `perry-runtime/src/gc/roots/stack_maps.rs`): it kept `{dwarf_reg, offset}`
//! per distinct root and nothing else. This module simply stops shipping what
//! was always discarded.
//!
//! # Where the remaining win comes from
//!
//! Dropping the dead weight alone is ~11x. Two further facts about real
//! programs take it to ~32x:
//!
//! * roots within a record cluster in the frame, so **sorting by frame offset
//!   and delta-encoding** them makes most roots a single byte;
//! * **77% of records have exactly the live set of the record before them** —
//!   consecutive safepoints in a function usually share their roots — so a
//!   repeat flag replaces the whole payload.
//!
//! On drizzle: 4,214,384 B -> 131,402 B, which turns the 3.5 MB file-size loss
//! into a ~271 KB win and lets the statepoint backend lead on size, speed and
//! RSS simultaneously.
//!
//! # Why the rewrite happens on assembly rather than the object
//!
//! `clang -S` prints the stack map as ordinary directives with the function
//! addresses as **symbol names in plain text** (`.quad _main`). Rewriting there
//! needs one text parser. Rewriting the object instead would need Mach-O *and*
//! ELF relocation parsing (the addresses are external relocations), plus
//! `llvm-objcopy` to drop the old section, plus a second link pass.
//!
//! It costs almost nothing: `-S` takes the same time as `-c` (the codegen is
//! the cost; printing text is free), and assembling the result is ~0.02s.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{anyhow, Context, Result};

/// Magic at the start of every emitted blob.
const GC_MAP_MAGIC: &[u8; 4] = b"PGCM";
/// Format version. Bump on any layout change — the runtime rejects others.
///
/// v5: a `u32 stream_offset` per function follows the function table, so the
/// runtime can decode ONE function's records without decoding every function
/// before it. The record stream and the instruction-offset array are byte for
/// byte what v4 emitted; this is an added array and nothing else. It costs
/// +1.3% of the section, and it is what lets a collection read the 74 records
/// a `claude --help` run needs instead of all 2,078,970.
///
/// v4 (#7803): the record header word gained a has-derived bit and records
/// carry DERIVED (interior) pointer slots tied to their bases. v3 collapsed
/// every statepoint (base, derived) pair to one slot on the false premise
/// that Perry emits no interior pointers; the runtime decoder fails closed on
/// a version mismatch, so both sides bump together.
const GC_MAP_VERSION: u8 = 5;
/// Section the compact map is emitted into, and the label it is given.
const GC_MAP_LABEL: &str = "_perry_gc_map";
const MACHO_SECTION: &str = "__PERRY_GCMAP,__perry_gcmap";
/// `w` because the section holds **relocated function addresses**: without
/// SHF_WRITE the linker reports `relocation against \`main\` in read-only
/// section \`.perry_gcmap\`` and creates a DT_TEXTREL in a PIE, which is both
/// a hardening regression and a portability hazard.
///
/// `R` is SHF_GNU_RETAIN, the ELF analogue of Mach-O's `.no_dead_strip`.
/// Perry links with `-Wl,--gc-sections`, and nothing in the program
/// references this section — the collector finds it by name at runtime — so
/// without RETAIN the linker discards it and the binary ships with no GC map
/// at all. Measured: the section is present in the object (PROGBITS, SHF_ALLOC,
/// with relocations) and absent from the linked binary.
const ELF_SECTION: &str = ".perry_gcmap,\"awR\",@progbits";
/// COFF/PE. The name is SHORT on purpose: a PE image section header has an
/// 8-byte name field, and long names survive only in object files (as a `/nnn`
/// string-table offset) — the linker cannot put `.perry_gcmap` in the image, so
/// the runtime would never find it by name. `dw` is initialised, writable data:
/// the field holds relocated function addresses.
const COFF_SECTION: &str = ".pgcmap,\"dw\"";
/// What the runtime looks for in a PE image. Must match `COFF_SECTION`'s name
/// and stay within eight bytes.
#[cfg(test)]
pub(crate) const COFF_SECTION_NAME: &str = ".pgcmap";

/// LLVM stack-map v3 location kinds. Only these two describe a frame slot;
/// `Constant`/`ConstIndex` carry the statepoint preamble and `Register` cannot
/// be recovered at collection time (which is what made plain stack maps
/// unsound — see the experiment write-up).
const LOCATION_DIRECT: u8 = 2;
const LOCATION_INDIRECT: u8 = 3;

/// One safepoint: where it is in its function, and which frame slots are live.
///
/// `instruction_offset` is the **assembly expression**, not a number: at `-O3`
/// LLVM emits it as a label difference (`Ltmp9-_main`) that only the assembler
/// can evaluate. That is why the emitted map stores offsets in a fixed-width
/// `u32` array rather than folding them into the varint stream.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Record {
    instruction_offset: String,
    /// `(dwarf_reg, frame_offset)`, deduplicated and sorted by frame offset.
    roots: Vec<(u16, i32)>,
    /// #7803: DERIVED (interior) pointer slots, each tied to the base root it
    /// was derived from — `(index into `roots`, dwarf_reg, frame_offset)`.
    ///
    /// The v3 format collapsed every statepoint (base, derived) pair to one
    /// slot on the stated premise that "Perry has no interior pointers". The
    /// premise is false: the RS4GC prelude (`mem2reg,sccp`) hoists for-of
    /// element GEPs into values that live across the poll, and LLVM records
    /// them as derived pointers. Collapsing the pair made the runtime walker
    /// treat `&elements[i]` as an object start — misread as a garbage header
    /// by the pin-latch, and never rewritten as `base' + delta` when the
    /// array moves, which dangles the cursor. Deduplicated and sorted by
    /// frame offset, like `roots`.
    derived: Vec<(u32, u16, i32)>,
}

/// One function's safepoints, keyed by the symbol the linker will relocate.
#[derive(Debug, Clone)]
struct FunctionMap {
    symbol: String,
    stack_size: u64,
    records: Vec<Record>,
}

/// How wide `.word` is for the target being assembled.
///
/// **`.word` is not a fixed size.** GNU `as` defines it as the target's natural
/// machine word: 2 bytes on x86 (where it dates to 16-bit) and 4 bytes on
/// AArch64, ARM, PowerPC, MIPS, SPARC and RISC-V. LLVM picks its own spelling
/// per target through `MCAsmInfo::Data32bitsDirective`, and the AArch64 **ELF**
/// backend picks `.word` — so an aarch64-linux stack map writes every 32-bit
/// field (`.word 2` for the function count, `.word .Ltmp0-fn` for each
/// instruction offset) with a directive that means something else on the host
/// this parser was written on.
///
/// Getting this wrong is not a parse error, it is a *wrong answer*: two bytes
/// of drift per field silently relocates every root that follows.
fn word_width_for(target: &str) -> usize {
    let arch = target.split('-').next().unwrap_or_default();
    // `x86_64h` (Haswell Mach-O) and the whole i?86 family included.
    if arch.starts_with("x86_64")
        || (arch.len() == 4 && arch.starts_with('i') && arch.ends_with("86"))
    {
        2
    } else {
        4
    }
}

/// Byte width contributed by each data directive LLVM emits in the block.
///
/// Every spelling any LLVM `MCAsmInfo` can choose for a fixed-width integer is
/// listed, not just the ones the host happens to emit — `Data32bitsDirective`
/// and friends are per-target strings, and a table written against one host is
/// exactly how a rewriter desynchronises on another.
fn directive_width(directive: &str, word_width: usize) -> Option<usize> {
    match directive {
        ".byte" | ".1byte" | ".dc.b" => Some(1),
        ".short" | ".2byte" | ".value" | ".hword" | ".dc.w" => Some(2),
        ".long" | ".4byte" | ".dc.l" => Some(4),
        ".quad" | ".8byte" | ".xword" | ".dc.a" => Some(8),
        ".word" => Some(word_width),
        _ => None,
    }
}

/// Directives that legitimately appear inside the stack-map block and
/// contribute **zero** bytes to it.
///
/// This exists because the alternative — skipping anything unrecognised — is
/// unsound in a way that cannot be noticed. The block is a byte stream decoded
/// by structural offset, so one ignored directive that *does* emit bytes shifts
/// everything after it; the decode then either fails somewhere unrelated or,
/// worse, succeeds against garbage. Anything not on this list and not in
/// `directive_width` is a refusal that names the directive.
/// Is this a GNU-as symbol assignment (`sym = expr`) rather than a directive?
///
/// Assemblers accept `.set sym, expr` and the bare `sym = expr` for the same
/// thing. Only the former starts with a `.`, so the directive dispatch sees the
/// SYMBOL as the mnemonic and refuses it. Both emit zero bytes.
///
/// Deliberately narrow: the name must be a single token that is not itself a
/// directive, and the `=` must not be part of a comparison inside a longer
/// expression. `.size sym, .-sym` and `.byte 1` are unaffected.
fn is_symbol_assignment(line: &str) -> bool {
    let Some((lhs, _rhs)) = line.split_once('=') else {
        return false;
    };
    // `==`, `>=`, `<=`, `!=` are expression operators, not an assignment.
    if lhs.ends_with(['=', '>', '<', '!']) {
        return false;
    }
    let name = lhs.trim();
    // ELF local labels start with `.L`, so "does not start with a dot" is the
    // wrong test -- it would reject `.Lperry_ic_8 = …`, which -O3 emits. Test
    // what actually matters instead: the LHS must not be a directive this
    // module already models. Anything else that is a single bare token before
    // an `=` is a symbol assignment.
    !name.is_empty()
        && !name.contains(char::is_whitespace)
        && directive_width(name, 8).is_none()
        && !is_zero_width_directive(name)
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '$' || c == '.')
}

fn is_zero_width_directive(directive: &str) -> bool {
    matches!(
        directive,
        ".globl"
            | ".global"
            | ".local"
            | ".weak"
            | ".hidden"
            | ".protected"
            | ".internal"
            | ".type"
            | ".size"
            | ".set"
            | ".equ"
            | ".file"
            | ".ident"
            | ".loc"
            | ".no_dead_strip"
            | ".private_extern"
            | ".addrsig"
            | ".addrsig_sym"
            | ".end"
    ) || directive.starts_with(".cfi_")
}

/// The assembled bytes of the stack-map block, plus the byte offsets at which
/// a `.quad` referenced a symbol instead of a literal.
struct RawBlock {
    start_line: usize,
    end_line: usize,
    bytes: Vec<u8>,
    symbols: HashMap<usize, String>,
    /// Zero-width lines found inside the block that describe some OTHER
    /// symbol — they must be re-emitted, not dropped with the map bytes.
    ///
    /// The block is delimited by section switches, but LLVM prints a
    /// symbol's *attributes* before it switches to that symbol's section.
    /// The ELF personality slot is the case that bit: `AsmPrinter`
    /// finalization emits the stack map, then `.hidden` + `.weak` for
    /// `DW.ref.perry_eh_personality`, and only then `.section
    /// .data.DW.ref.perry_eh_personality,"awG",…,comdat`. Swallowing those
    /// two lines assembles the COMDAT slot as a LOCAL symbol; every
    /// multi-object link (`ld -r` of split codegen units, or the final
    /// exe/dylib link) keeps one group and silently drops the other objects'
    /// CIE personality relocations (`.eh_frame` is exempt from the
    /// discarded-section diagnostic), so the first caught throw through a
    /// frame from any other object calls a garbage personality pointer and
    /// dies in `_Unwind_RaiseException`.
    carried: Vec<String>,
}

fn find_block_start(lines: &[&str]) -> Option<usize> {
    lines.iter().position(|line| {
        let t = line.trim_start();
        t.starts_with(".section")
            && (t.contains("__LLVM_STACKMAPS") || t.contains(".llvm_stackmaps"))
    })
}

fn parse_block(lines: &[&str], word_width: usize) -> Result<RawBlock, String> {
    let start_line = find_block_start(lines).ok_or_else(|| "no stack-map section".to_string())?;

    let mut bytes: Vec<u8> = Vec::new();
    let mut symbols: HashMap<usize, String> = HashMap::new();
    let mut carried: Vec<String> = Vec::new();
    let mut end_line = lines.len();

    for (index, raw) in lines.iter().enumerate().skip(start_line + 1) {
        // LLVM's assembly memory buffer may expose its terminating NUL as the
        // final line. It is not an assembler directive and emits no bytes.
        let line = raw.trim().trim_matches('\0').trim();
        // The block runs to the next section or to the Mach-O epilogue.
        //
        // The shorthand section directives are terminators too. Missing one
        // does not fail loudly: the parser would keep accumulating whatever
        // followed as if it were map bytes, `decode_v3` would finish the real
        // records and then try to read the trailing data as another map, and
        // the module would be REFUSED for a reason nowhere near the cause.
        if line.starts_with(".section")
            || line.starts_with(".subsections_via_symbols")
            || matches!(
                line.split_whitespace().next().unwrap_or_default(),
                ".text" | ".data" | ".bss" | ".rodata" | ".const" | ".cstring" | ".literal8"
            )
        {
            end_line = index;
            break;
        }
        if line.is_empty() || line.starts_with('#') || line.starts_with("//") || line.ends_with(':')
        {
            continue;
        }
        // GNU-as symbol assignment: `sym = expr`, the bare form of `.set`.
        // It defines a symbol and emits ZERO bytes. `.set`/`.equ` are already on
        // the zero-width list; this spelling is not a directive at all, so the
        // dispatch below would report the SYMBOL as an unrecognised directive.
        //
        // It only shows up at -O3, which is why the arm CI caught it and the -O2
        // paths never did: the optimiser materialises absolute-symbol aliases
        // (`perry_null_guard_zero = …`, `perry_class_keys__… = …`) and the ELF
        // asm printer emits them in this form. Mach-O output does not, so this
        // is invisible on the macOS arms.
        if is_symbol_assignment(line) {
            carry_if_foreign(&mut carried, line);
            continue;
        }

        let mut parts = line.splitn(2, char::is_whitespace);
        let directive = parts.next().unwrap_or_default();
        let operand = parts.next().unwrap_or_default();
        let operand = operand
            .split('#')
            .next()
            .unwrap_or_default()
            .split("//")
            .next()
            .unwrap_or_default()
            .trim();

        // Alignment is real content: LLVM aligns every record, and skipping
        // the padding desynchronises every offset that follows it.
        if directive == ".p2align" || directive == ".align" || directive == ".balign" {
            let first = operand.split(',').next().unwrap_or_default().trim();
            let value: u32 = first.parse().map_err(|_| {
                format!(
                    "line {}: unparseable alignment operand in `{line}`",
                    index + 1
                )
            })?;
            let align = if directive == ".p2align" {
                1usize << value
            } else {
                value as usize
            };
            while align > 1 && bytes.len() % align != 0 {
                bytes.push(0);
            }
            continue;
        }

        // `.zero`/`.space`/`.skip` are pure padding, but they are padding that
        // OCCUPIES BYTES — the one shape where "skip what we don't model" turns
        // a decode into silent garbage rather than an error.
        if directive == ".zero" || directive == ".space" || directive == ".skip" {
            let first = operand.split(',').next().unwrap_or_default().trim();
            let count: usize = first
                .parse()
                .map_err(|_| format!("line {}: unparseable fill count in `{line}`", index + 1))?;
            bytes.resize(bytes.len() + count, 0);
            continue;
        }

        if let Some(width) = directive_width(directive, word_width) {
            match parse_int(operand) {
                Some(value) => bytes.extend_from_slice(&value.to_le_bytes()[..width]),
                None => {
                    // A symbolic operand. Two kinds appear: the `.quad`
                    // function address, and — at `-O3` — the `.long`
                    // instruction offset as a label difference. Remember the
                    // expression and reserve the slot so every later
                    // structural offset stays correct.
                    symbols.insert(bytes.len(), operand.to_string());
                    bytes.extend_from_slice(&0u64.to_le_bytes()[..width]);
                }
            }
            continue;
        }

        if !is_zero_width_directive(directive) {
            return Err(format!(
                "line {}: unrecognised directive `{directive}` inside the stack-map block \
                 (`{line}`). Its byte width is unknown, and guessing it would shift every \
                 offset after it — decoding a root list from the wrong bytes rather than \
                 failing. Add it to `directive_width` (with its width) or to \
                 `is_zero_width_directive`.",
                index + 1
            ));
        }
        carry_if_foreign(&mut carried, line);
    }

    Ok(RawBlock {
        start_line,
        end_line,
        bytes,
        symbols,
        carried,
    })
}

/// A zero-width line inside the block emits no map bytes, so it can only be
/// describing a symbol. If that symbol is the map's own label it belongs to
/// the block being replaced (the replacement declares its own); anything
/// else — a symbol attribute LLVM printed ahead of its section switch, or an
/// absolute-symbol assignment — is unrelated to the map and must survive the
/// rewrite verbatim.
fn carry_if_foreign(carried: &mut Vec<String>, line: &str) {
    if !line.contains("__LLVM_StackMaps") {
        carried.push(line.to_string());
    }
}

fn parse_int(text: &str) -> Option<u64> {
    let text = text.trim();
    if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        return u64::from_str_radix(hex, 16).ok();
    }
    if let Some(negative) = text.strip_prefix('-') {
        return negative
            .parse::<u64>()
            .ok()
            .map(|v| (v as i64).wrapping_neg() as u64);
    }
    text.parse::<u64>().ok()
}

fn read_u16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn read_u32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn read_u64(bytes: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(bytes.get(at..at + 8)?.try_into().ok()?))
}

fn align_up(value: usize, alignment: usize) -> usize {
    value.div_ceil(alignment) * alignment
}

/// Decode every concatenated v3 map in the block.
///
/// The section is a *sequence* of maps, one per object the linker saw — a
/// decoder that reads only the first header silently drops the rest, so this
/// walks until the bytes are consumed.
fn decode_v3(block: &RawBlock) -> Result<Vec<FunctionMap>, String> {
    let bytes = &block.bytes;
    let mut out: Vec<FunctionMap> = Vec::new();
    let mut pos = 0usize;
    let mut maps = 0usize;

    let truncated = |what: &str, at: usize| {
        format!(
            "{what} runs past the end of the {} byte block (offset {at})",
            bytes.len()
        )
    };

    while pos + 16 <= bytes.len() {
        if bytes[pos] != 3 {
            // Inter-map alignment padding.
            pos += 1;
            continue;
        }
        maps += 1;
        let function_count =
            read_u32(bytes, pos + 4).ok_or_else(|| truncated("map header", pos))? as usize;
        let constant_count =
            read_u32(bytes, pos + 8).ok_or_else(|| truncated("map header", pos))? as usize;
        let record_count =
            read_u32(bytes, pos + 12).ok_or_else(|| truncated("map header", pos))? as usize;
        pos += 16;

        let mut heads = Vec::with_capacity(function_count);
        let mut expected = 0usize;
        for index in 0..function_count {
            let symbol = block.symbols.get(&pos).cloned().ok_or_else(|| {
                format!(
                    "map {maps} function[{index}]: the 8-byte function address at block offset \
                     {pos} is a literal ({:#x}), not a symbol reference. The rewriter re-emits \
                     that address as `.quad <symbol>` and has no way to name a function it was \
                     given only as a number.",
                    read_u64(bytes, pos).unwrap_or(0)
                )
            })?;
            let stack_size =
                read_u64(bytes, pos + 8).ok_or_else(|| truncated("function record", pos))?;
            let records = read_u64(bytes, pos + 16)
                .ok_or_else(|| truncated("function record", pos))?
                as usize;
            expected = expected
                .checked_add(records)
                .ok_or_else(|| "record count overflow".to_string())?;
            heads.push((symbol, stack_size, records));
            pos += 24;
        }
        if expected != record_count {
            return Err(format!(
                "map {maps}: the per-function record counts sum to {expected} but the map header \
                 declares {record_count}. The byte stream and the assembly directives that \
                 produced it have desynchronised — usually a directive inside the block whose \
                 width the rewriter models incorrectly."
            ));
        }
        pos = pos
            .checked_add(
                constant_count
                    .checked_mul(8)
                    .ok_or_else(|| "constant pool overflow".to_string())?,
            )
            .ok_or_else(|| "constant pool overflow".to_string())?;

        for (symbol, stack_size, count) in heads {
            let mut records = Vec::with_capacity(count);
            for index in 0..count {
                let record_start = pos;
                let instruction_offset = block
                    .symbols
                    .get(&(pos + 8))
                    .cloned()
                    .unwrap_or_else(|| read_u32(bytes, pos + 8).unwrap_or(0).to_string());
                let location_count = read_u16(bytes, pos + 14)
                    .ok_or_else(|| truncated(&format!("{symbol} record {index}"), pos))?
                    as usize;
                pos += 16;

                // Read every location first: the statepoint layout is
                // POSITIONAL — three constants (calling convention, flags,
                // deopt count), then that many deopt locations, then the GC
                // pointer locations in (base, derived) PAIRS — and pairing
                // cannot be recovered from a flat filter.
                let mut locations: Vec<(u8, u16, u16, i32)> = Vec::with_capacity(location_count);
                for location in 0..location_count {
                    let kind = *bytes.get(pos).ok_or_else(|| {
                        truncated(&format!("{symbol} record {index} location {location}"), pos)
                    })?;
                    let size = read_u16(bytes, pos + 2).ok_or_else(|| {
                        truncated(&format!("{symbol} record {index} location {location}"), pos)
                    })?;
                    let dwarf_reg = read_u16(bytes, pos + 4).ok_or_else(|| {
                        truncated(&format!("{symbol} record {index} location {location}"), pos)
                    })?;
                    let offset = read_u32(bytes, pos + 8).ok_or_else(|| {
                        truncated(&format!("{symbol} record {index} location {location}"), pos)
                    })? as i32;
                    locations.push((kind, size, dwarf_reg, offset));
                    pos += 12;
                }

                let is_root_slot = |&(kind, size, _, _): &(u8, u16, u16, i32)| {
                    matches!(kind, LOCATION_DIRECT | LOCATION_INDIRECT) && size == 8
                };
                let mut roots: Vec<(u16, i32)> = Vec::new();
                // `(base_reg, base_off, derived_reg, derived_off)` until the
                // roots list is final and indices can be resolved.
                let mut derived_pairs: Vec<(u16, i32, u16, i32)> = Vec::new();
                // The deopt count is the third constant's small value. A
                // malformed preamble (fewer than 3 locations, or a non-constant
                // where the count belongs) falls back to the v3 flat filter —
                // strictly the OLD behavior, never a new failure mode.
                const LOCATION_CONSTANT: u8 = 4;
                let gc_pairs_start = match locations.get(2) {
                    Some(&(LOCATION_CONSTANT, _, _, deopt_count)) if deopt_count >= 0 => {
                        Some(3usize + deopt_count as usize)
                    }
                    _ => None,
                };
                match gc_pairs_start {
                    Some(start)
                        if start <= locations.len() && (locations.len() - start) % 2 == 0 =>
                    {
                        for pair in locations[start..].chunks_exact(2) {
                            let (base, derived) = (&pair[0], &pair[1]);
                            if !is_root_slot(base) || !is_root_slot(derived) {
                                // A constant/register operand (e.g. a null
                                // base): keep whichever half IS a frame slot,
                                // as the flat filter always has.
                                for loc in pair.iter().filter(|l| is_root_slot(l)) {
                                    if !roots.contains(&(loc.2, loc.3)) {
                                        roots.push((loc.2, loc.3));
                                    }
                                }
                                continue;
                            }
                            let base_slot = (base.2, base.3);
                            let derived_slot = (derived.2, derived.3);
                            if !roots.contains(&base_slot) {
                                roots.push(base_slot);
                            }
                            if derived_slot != base_slot {
                                derived_pairs.push((
                                    base_slot.0,
                                    base_slot.1,
                                    derived_slot.0,
                                    derived_slot.1,
                                ));
                            }
                        }
                    }
                    _ => {
                        for loc in locations.iter().filter(|l| is_root_slot(l)) {
                            if !roots.contains(&(loc.2, loc.3)) {
                                roots.push((loc.2, loc.3));
                            }
                        }
                    }
                }

                pos = align_up(pos - record_start, 8) + record_start;
                let live_out_count = read_u16(bytes, pos + 2)
                    .ok_or_else(|| truncated(&format!("{symbol} record {index} live-outs"), pos))?
                    as usize;
                pos = pos
                    .checked_add(4)
                    .and_then(|p| p.checked_add(live_out_count.checked_mul(4)?))
                    .ok_or_else(|| truncated(&format!("{symbol} record {index} live-outs"), pos))?;
                pos = align_up(pos - record_start, 8) + record_start;
                if pos > bytes.len() {
                    return Err(truncated(&format!("{symbol} record {index}"), pos));
                }

                roots.sort_unstable_by_key(|(_, offset)| *offset);
                // Resolve derived pairs against the SORTED roots list, drop
                // duplicates, and drop any derived slot that is also a plain
                // root (a slot cannot be both an object start and an interior
                // pointer; preferring the root keeps v3's behavior for the
                // ambiguous shape rather than inventing a new one).
                let mut derived: Vec<(u32, u16, i32)> = Vec::new();
                for (base_reg, base_off, d_reg, d_off) in derived_pairs {
                    if roots.contains(&(d_reg, d_off)) {
                        continue;
                    }
                    let Some(base_index) =
                        roots.iter().position(|&slot| slot == (base_reg, base_off))
                    else {
                        continue;
                    };
                    // Slot-level dedup: one slot holds one value, so a second
                    // pairing for the same (reg, offset) — same base or not —
                    // must not produce a second rewrite of it.
                    if !derived.iter().any(|&(_, r, o)| r == d_reg && o == d_off) {
                        derived.push((base_index as u32, d_reg, d_off));
                    }
                }
                derived.sort_unstable_by_key(|&(_, _, offset)| offset);
                records.push(Record {
                    instruction_offset,
                    roots,
                    derived,
                });
            }
            out.push(FunctionMap {
                symbol,
                stack_size,
                records,
            });
        }
    }

    if out.is_empty() {
        return Err(format!(
            "walked {} bytes and found {maps} map header(s) but no function records",
            bytes.len()
        ));
    }
    Ok(out)
}

fn push_varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push((value as u8 & 0x7F) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

fn zigzag(value: i32) -> u64 {
    ((value << 1) ^ (value >> 31)) as u32 as u64
}

/// The two DWARF register numbers the compact format's short base tags stand
/// for. They are **aarch64** numbers *by definition of the format*, not by
/// assumption about the target: tag 0 means "DWARF 29" and tag 1 means
/// "DWARF 31" on every architecture, and the runtime decoder
/// (`gc/roots/stack_maps.rs`) maps them back to the same two constants.
///
/// This was the suspected cause of #7321 — the module names its bases in
/// aarch64 terms throughout — and it is not. On x86-64 every root comes back
/// with DWARF 7 (RSP; measured 56 of 56 on `01_nursery_churn`), which matches
/// neither constant, so it takes the explicit-register tag and round-trips
/// exactly; `verify_roundtrip` now proves that on every compile. The runtime's
/// `chain_walkable` test (`reg ∈ {29, 31}`) is correspondingly false there, so
/// it uses the platform unwinder — which is the correct walker for x86-64,
/// where no fp-chain walker is compiled in.
///
/// The cost of the mismatch is size, not correctness: an x86-64 root spends one
/// extra byte on its register number (403 compact bytes rather than ~347 on
/// that probe). Making the tags mean "the target's FP/SP" would recover it, but
/// it would put the compiler's idea of the target and the runtime's
/// `target_arch` in a position where disagreeing corrupts every root's base —
/// a size win is not worth that, so the tags stay literal.
const DWARF_REG_SP_AARCH64: u16 = 31;
/// Frame pointer, the other base the single-bit encoding can express.
const DWARF_REG_FP_AARCH64: u16 = 29;

/// Emit one root list in the shared tag/delta encoding (see the header-word
/// comment in [`encode_stream`]). Used for both the base roots and the
/// derived slots — the derived list restarts its own delta chain.
fn encode_slots(stream: &mut Vec<u8>, slots: impl Iterator<Item = (u16, i32)>) {
    let mut previous: Option<i32> = None;
    for (reg, offset) in slots {
        let tag = match reg {
            DWARF_REG_FP_AARCH64 => 0u64,
            DWARF_REG_SP_AARCH64 => 1,
            _ => 2,
        };
        let delta = match previous {
            None => offset,
            Some(prev) => offset.wrapping_sub(prev),
        };
        push_varint(stream, (zigzag(delta) << 2) | tag);
        if tag == 2 {
            push_varint(stream, u64::from(reg));
        }
        previous = Some(offset);
    }
}

/// The varint root stream, plus where each function's records start in it.
///
/// `function_offsets[i]` is not derived from anything: it is `bytes.len()` at
/// the moment `encode_stream` begins function `i`, so it IS that function's
/// position by construction. Nothing recomputes it, and a wrong value is not
/// expressible without changing the one line that records it.
///
/// It exists because a decoder that wants one function's records has no other
/// way to find them — the stream is varint-chained, so reaching function `i`
/// means decoding functions `0..i` first. `verify_roundtrip` proves the
/// offsets against the sequential decode on every compile, for every function.
#[derive(Clone)]
struct CompactStream {
    bytes: Vec<u8>,
    /// One entry per function, in `functions` order.
    function_offsets: Vec<u32>,
}

fn encode_stream(functions: &[FunctionMap]) -> Result<CompactStream, String> {
    let mut stream = Vec::new();
    let mut function_offsets = Vec::with_capacity(functions.len());
    for function in functions {
        // Recorded BEFORE any of this function's bytes are pushed. `u32`
        // because that is the width a map format can afford to spend on it
        // per function; a blob that outgrows it must say so here rather than
        // silently wrap and point a decoder at another function's records.
        function_offsets.push(u32::try_from(stream.len()).map_err(|_| {
            format!(
                "{}: the compact stream reached {} bytes, past the u32 a per-function offset can \
                 hold. A wrapped offset would point a decoder at another function's records.",
                function.symbol,
                stream.len()
            )
        })?);
        let mut previous_record: Option<(&Vec<(u16, i32)>, &Vec<(u32, u16, i32)>)> = None;
        for record in &function.records {
            if previous_record == Some((&record.roots, &record.derived)) {
                // Repeat flag: the live set (bases AND deriveds) is the
                // previous record's.
                push_varint(&mut stream, 1);
                continue;
            }
            // v4 header word: (root_count << 2) | (has_derived << 1) | 0.
            // Bit 0 stays the repeat flag, so a v3-shaped record (no
            // deriveds) costs the same bytes it did.
            let has_derived = u64::from(!record.derived.is_empty());
            push_varint(
                &mut stream,
                ((record.roots.len() as u64) << 2) | (has_derived << 1),
            );

            // Deltas are zigzagged rather than emitted raw. `decode_v3` sorts
            // roots so they are non-negative in practice, but a raw negative
            // delta sign-extends into a 10-byte varint and silently bloats the
            // map — the format must not depend on an ordering invariant held
            // somewhere else.
            // Base is a 2-bit tag, not a single FP/SP bit: LLVM also uses a
            // callee-saved register (x19 on aarch64) as a frame base pointer
            // in functions with dynamic stack allocation — measured 66 root
            // slots in one real module. A bit cannot express that, and the
            // format must not be the reason a root is unrepresentable.
            //   0 = frame pointer, 1 = stack pointer, 2 = explicit DWARF
            //   register number as a following varint.
            encode_slots(&mut stream, record.roots.iter().copied());
            if !record.derived.is_empty() {
                push_varint(&mut stream, record.derived.len() as u64);
                // Base indices first (into the sorted roots list), then the
                // slots themselves in the shared encoding with a fresh delta
                // chain.
                for &(base_index, _, _) in &record.derived {
                    push_varint(&mut stream, u64::from(base_index));
                }
                encode_slots(
                    &mut stream,
                    record.derived.iter().map(|&(_, reg, off)| (reg, off)),
                );
            }
            previous_record = Some((&record.roots, &record.derived));
        }
    }
    Ok(CompactStream {
        bytes: stream,
        function_offsets,
    })
}

fn read_varint(bytes: &[u8], mut at: usize) -> Option<(u64, usize)> {
    let mut value = 0u64;
    let mut shift = 0u32;
    loop {
        let byte = *bytes.get(at)?;
        at += 1;
        value |= u64::from(byte & 0x7F).checked_shl(shift)?;
        if byte & 0x80 == 0 {
            return Some((value, at));
        }
        shift += 7;
        if shift > 63 {
            return None;
        }
    }
}

fn unzigzag(value: u32) -> i32 {
    ((value >> 1) as i32) ^ -((value & 1) as i32)
}

/// Decode `stream` exactly as `perry-runtime`'s `parse_gc_map` does and assert
/// it reproduces the live set of every record.
///
/// This is the check that the *encoding* did not lose roots, and unlike the
/// walker cross-check (`PERRY_STACKMAP_WALKER=verify`) it needs no
/// architecture-specific stack walker, so it holds on every target. It is the
/// half of "did we parse correctly" that a decode-into-something-plausible
/// cannot fake: a repeat flag mis-set, a delta that sign-extends the wrong way,
/// or a base tag written for one architecture and read on another all produce a
/// stream that decodes fine and describes different memory.
///
/// Always on. It walks bytes already in cache and is far below the noise floor
/// of the LLVM run that produced them, and an assertion that has to be switched
/// on is one that is off when it matters.
fn decode_slots(
    stream: &[u8],
    mut cursor: usize,
    count: usize,
    where_: &dyn Fn() -> String,
) -> Result<(Vec<(u16, i32)>, usize), String> {
    let mut slots = Vec::with_capacity(count);
    let mut last: Option<i32> = None;
    for slot in 0..count {
        let (value, next) = read_varint(stream, cursor)
            .ok_or_else(|| format!("{}: truncated slot {slot}", where_()))?;
        cursor = next;
        let dwarf_reg = match value & 3 {
            0 => DWARF_REG_FP_AARCH64,
            1 => DWARF_REG_SP_AARCH64,
            2 => {
                let (reg, next) = read_varint(stream, cursor).ok_or_else(|| {
                    format!("{}: truncated explicit register for slot {slot}", where_())
                })?;
                cursor = next;
                u16::try_from(reg)
                    .map_err(|_| format!("{}: slot {slot} register {reg} exceeds u16", where_()))?
            }
            tag => {
                return Err(format!(
                    "{}: slot {slot} has reserved base tag {tag}",
                    where_()
                ))
            }
        };
        let delta = unzigzag((value >> 2) as u32);
        let offset = match last {
            None => delta,
            Some(previous_offset) => previous_offset.wrapping_add(delta),
        };
        last = Some(offset);
        slots.push((dwarf_reg, offset));
    }
    Ok((slots, cursor))
}

fn verify_roundtrip(functions: &[FunctionMap], compact: &CompactStream) -> Result<(), String> {
    let stream = &compact.bytes[..];
    let mut cursor = 0usize;
    for (function_index, function) in functions.iter().enumerate() {
        // The per-function offset must BE the position the sequential decode
        // reaches. Checked here rather than in a test because this runs on
        // every compile over every function of the binary being produced —
        // 72,669 of them for claude-code — so an offset that is ever wrong
        // fails the compile instead of pointing a decoder at another
        // function's live set. It cannot fire while nothing emits or reads
        // these offsets, which is the point of checking it now.
        let recorded = compact
            .function_offsets
            .get(function_index)
            .copied()
            .ok_or_else(|| {
                format!(
                    "{}: the encoder recorded {} function offsets for {} functions",
                    function.symbol,
                    compact.function_offsets.len(),
                    functions.len()
                )
            })?;
        if recorded as usize != cursor {
            return Err(format!(
                "{}: the encoder recorded this function's records starting at stream offset \
                 {recorded}, but decoding every earlier function reaches {cursor}. A decoder \
                 that trusted the recorded offset would read another function's live set.",
                function.symbol
            ));
        }
        let mut previous: Option<(Vec<(u16, i32)>, Vec<(u32, u16, i32)>)> = None;
        for (index, record) in function.records.iter().enumerate() {
            let where_ = || format!("{} record {index}", function.symbol);
            let (header, next) = read_varint(stream, cursor)
                .ok_or_else(|| format!("{}: truncated record header", where_()))?;
            cursor = next;
            let decoded = if header & 1 == 1 {
                previous
                    .clone()
                    .ok_or_else(|| format!("{}: repeat flag with no previous live set", where_()))?
            } else {
                let count = (header >> 2) as usize;
                let has_derived = header & 2 != 0;
                let (roots, next) = decode_slots(stream, cursor, count, &where_)?;
                cursor = next;
                let derived = if has_derived {
                    let (derived_count, next) = read_varint(stream, cursor)
                        .ok_or_else(|| format!("{}: truncated derived count", where_()))?;
                    cursor = next;
                    let mut bases = Vec::with_capacity(derived_count as usize);
                    for entry in 0..derived_count {
                        let (base_index, next) = read_varint(stream, cursor).ok_or_else(|| {
                            format!("{}: truncated derived base index {entry}", where_())
                        })?;
                        cursor = next;
                        if base_index as usize >= roots.len() {
                            return Err(format!(
                                "{}: derived entry {entry} names base {base_index} of {} roots",
                                where_(),
                                roots.len()
                            ));
                        }
                        bases.push(base_index as u32);
                    }
                    let (slots, next) =
                        decode_slots(stream, cursor, derived_count as usize, &where_)?;
                    cursor = next;
                    bases
                        .into_iter()
                        .zip(slots)
                        .map(|(base, (reg, off))| (base, reg, off))
                        .collect()
                } else {
                    Vec::new()
                };
                (roots, derived)
            };
            if decoded.0 != record.roots || decoded.1 != record.derived {
                return Err(format!(
                    "{}: the compact stream decodes to {decoded:?} but the stack map recorded \
                     roots {:?} derived {:?}. Re-encoding changed this safepoint's live set, so \
                     the collector would scan different words than LLVM described.",
                    where_(),
                    record.roots,
                    record.derived
                ));
            }
            previous = Some(decoded);
        }
    }
    if cursor != stream.len() {
        return Err(format!(
            "the compact stream has {} trailing byte(s) after the last record — the encoder and \
             the runtime's decoder disagree about the layout",
            stream.len() - cursor
        ));
    }
    Ok(())
}

/// Assemble the emitted directives for one compact blob.
///
/// Layout (little-endian), mirrored by the runtime decoder:
///
/// ```text
///   0  "PGCM"
///   4  u8 version, u8 reserved, u16 flags
///   8  u32 function_count
///  12  u32 total_len          -- lets the runtime walk concatenated blobs
///  16  function_count x { u64 address, u32 stack_size, u32 record_count }
///      function_count x u32 stream_offset   -- v5; see below
///      record_count_total x u32 instruction_offset
///      varint root stream (see `encode_stream`)
/// ```
///
/// The v5 `stream_offset` array is where each function's records begin in the
/// varint stream. Without it the stream is only readable from the front — it
/// is delta- and repeat-chained — so a collector that wanted one frame's live
/// set had to decode the whole section. It is placed AFTER the function table
/// rather than inside it so the table's relocated addresses stay 8-byte
/// aligned and the entry keeps its v4 shape.
///
/// The function table starts at 16 so every relocated address is 8-byte
/// aligned, and the offset array that follows it is 4-byte aligned.
///
/// Instruction offsets are a fixed-width array rather than part of the varint
/// stream because at `-O3` they are **label differences the assembler
/// evaluates** (`Ltmp9-_main`), so their values do not exist at rewrite time.
/// That costs ~4 bytes per record — 18.7x compaction instead of 31.8x — and
/// buys not having to assemble twice just to learn numbers the assembler is
/// about to compute anyway.
/// `ptr64` selects the width of the relocated function-address field. It is the
/// target's pointer width, not a constant: `arm64_32` (watchOS) is ILP32, so an
/// 8-byte address slot there would need a relocation ld64 has no reason to
/// produce, and the runtime would be reading two pointers as one. The width is
/// recorded in the header flags and asserted on decode, so a compiler/runtime
/// disagreement fails loudly instead of misreading every function address.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ObjectFormat {
    MachO,
    Elf,
    Coff,
}

fn format_for(target: &str) -> ObjectFormat {
    if target.contains("apple") || target.contains("darwin") {
        ObjectFormat::MachO
    } else if target.contains("windows") || target.contains("msvc") {
        ObjectFormat::Coff
    } else {
        ObjectFormat::Elf
    }
}

fn emit_asm(
    functions: &[FunctionMap],
    compact: &CompactStream,
    format: ObjectFormat,
    ptr64: bool,
) -> String {
    let stream = &compact.bytes[..];
    let record_total: usize = functions.iter().map(|f| f.records.len()).sum();
    let addr_bytes = if ptr64 { 8 } else { 4 };
    let entry_bytes = addr_bytes + 8; // address + u32 stack_size + u32 records
    let total_len =
        16 + functions.len() * entry_bytes + functions.len() * 4 + record_total * 4 + stream.len();
    let mut out = String::new();
    out.push_str(&format!(
        "\t.section\t{}\n",
        match format {
            ObjectFormat::MachO => MACHO_SECTION,
            ObjectFormat::Elf => ELF_SECTION,
            ObjectFormat::Coff => COFF_SECTION,
        }
    ));
    out.push_str("\t.p2align\t3\n");
    out.push_str(&format!("{GC_MAP_LABEL}:\n"));
    out.push_str(&format!(
        "\t.ascii\t\"{}\"\n",
        std::str::from_utf8(GC_MAP_MAGIC).expect("magic is ASCII")
    ));
    out.push_str(&format!("\t.byte\t{GC_MAP_VERSION}\n"));
    out.push_str("\t.byte\t0\n");
    // Header flags, bit 0: the function-address field is 8 bytes wide.
    out.push_str(&format!("\t.short\t{}\n", u16::from(ptr64)));
    out.push_str(&format!("\t.long\t{}\n", functions.len()));
    out.push_str(&format!("\t.long\t{total_len}\n"));
    for function in functions {
        out.push_str(&format!(
            "\t{}\t{}\n",
            if ptr64 { ".quad" } else { ".long" },
            function.symbol
        ));
        out.push_str(&format!("\t.long\t{}\n", function.stack_size as u32));
        out.push_str(&format!("\t.long\t{}\n", function.records.len()));
    }
    // v5: where each function's records begin in the varint stream.
    for offset in &compact.function_offsets {
        out.push_str(&format!("\t.long\t{offset}\n"));
    }
    for function in functions {
        for record in &function.records {
            out.push_str(&format!("\t.long\t{}\n", record.instruction_offset));
        }
    }
    for chunk in stream.chunks(32) {
        let bytes: Vec<String> = chunk.iter().map(|b| b.to_string()).collect();
        out.push_str(&format!("\t.byte\t{}\n", bytes.join(",")));
    }
    out
}

/// Statistics for the caller to log — a compaction that silently did nothing
/// must be distinguishable from one that ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GcMapStats {
    original_bytes: usize,
    compact_bytes: usize,
    functions: usize,
    records: usize,
    roots: usize,
}

/// Rewrite the LLVM stack-map block in `asm` into the compact map.
///
/// Returns `None` when there is no stack-map block to rewrite (the common case
/// for a module without safepoints) or when the block does not parse.
///
/// Those two are NOT the same to the caller: no block is fine, while a block
/// that fails to parse is a hard error in `compact_and_assemble`. Keeping
/// LLVM's section in that case would look conservative and would in fact lose
/// the module's roots, because the runtime reads only the compact section.
fn compact_stack_map_asm(asm: &str, target: &str) -> Result<Option<(String, GcMapStats)>, String> {
    let lines: Vec<&str> = asm.lines().collect();
    if find_block_start(&lines).is_none() {
        return Ok(None);
    }
    // watchOS is ILP32: the relocated function-address field follows the
    // target's pointer width rather than assuming 8 bytes.
    let ptr64 = !target.starts_with("arm64_32");
    let block = parse_block(&lines, word_width_for(target))?;
    let functions = decode_v3(&block)?;
    let stream = encode_stream(&functions)?;
    verify_roundtrip(&functions, &stream)?;

    let stats = GcMapStats {
        original_bytes: block.bytes.len(),
        compact_bytes: 16
            + functions.len() * (if ptr64 { 16 } else { 12 })
            + functions.len() * 4
            + functions.iter().map(|f| f.records.len()).sum::<usize>() * 4
            + stream.bytes.len(),
        functions: functions.len(),
        records: functions.iter().map(|f| f.records.len()).sum(),
        roots: functions
            .iter()
            .flat_map(|f| f.records.iter())
            .map(|r| r.roots.len())
            .sum(),
    };

    let replacement = emit_asm(&functions, &stream, format_for(target), ptr64);
    let mut out = String::with_capacity(asm.len());
    for line in &lines[..block.start_line] {
        // `.no_dead_strip` names the block's label from outside it. It is also
        // the only thing keeping a section nothing references from being
        // discarded, so retarget it instead of dropping it — without it the
        // map is stripped and the collector finds no roots at all.
        if line.contains(".no_dead_strip") && line.contains("__LLVM_StackMaps") {
            out.push_str(&format!("\t.no_dead_strip\t{GC_MAP_LABEL}\n"));
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    out.push_str(&replacement);
    // Re-emit the symbol lines the block swallowed, in their original order,
    // exactly where they stood: after the map, before the section switch
    // that ends the block. They are position-independent (attributes and
    // assignments), so the only thing that matters is that they are present.
    for line in &block.carried {
        if !is_symbol_assignment(line) {
            out.push('\t');
        }
        out.push_str(line);
        out.push('\n');
    }
    for line in &lines[block.end_line..] {
        if line.contains("__LLVM_StackMaps") {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    Ok(Some((out, stats)))
}

/// Decode the stack map in `asm` down to exactly what the collector reads at
/// run time: per function symbol, one entry per safepoint listing that
/// safepoint's deduplicated `(dwarf_reg, frame_offset)` roots.
///
/// The seam `native_root_coverage` asserts through (#7502). It deliberately
/// runs `encode_stream` + `verify_roundtrip` rather than handing back
/// `decode_v3`'s output, so a test's "this safepoint has N roots" is a claim
/// about the **compact map the binary ships**, not about an intermediate the
/// encoder could still drop on the floor. Absence of a block is an `Err`, not
/// an empty `Ok`: "there were no roots" and "there was no map" must not be the
/// same answer to a caller that is about to assert a root count is zero.
#[cfg(all(test, feature = "llvm-inprocess"))]
#[allow(clippy::type_complexity)]
pub(crate) fn decode_stack_map_roots(
    asm: &str,
    target: &str,
) -> Result<Vec<(String, Vec<Vec<(u16, i32)>>)>, String> {
    let lines: Vec<&str> = asm.lines().collect();
    if find_block_start(&lines).is_none() {
        return Err("assembly carries no stack-map section".to_string());
    }
    let block = parse_block(&lines, word_width_for(target))?;
    let functions = decode_v3(&block)?;
    let stream = encode_stream(&functions)?;
    verify_roundtrip(&functions, &stream)?;
    Ok(functions
        .into_iter()
        .map(|f| {
            (
                f.symbol,
                f.records.into_iter().map(|r| r.roots).collect::<Vec<_>>(),
            )
        })
        .collect())
}

/// Rewrite the stack map in `asm_path` into Perry's compact form, then
/// assemble it to `obj_path`.
///
/// A module with no stack-map block is assembled unchanged — there is nothing
/// to compact.
///
/// A module that HAS a block which does not parse is a hard error, not a
/// fallback. Keeping LLVM's section there looks conservative and is not: the
/// runtime reads only `__perry_gcmap`, so that module's records would be
/// present in the binary, unread, and its roots invisible to the collector —
/// while other modules still emit a valid section, so even the "section
/// present but undecodable" guard in the runtime stays quiet. Silent lost
/// roots are precisely what this backend exists to make impossible.
pub fn compact_and_assemble(
    clang: &Path,
    target: &str,
    asm_path: &Path,
    obj_path: &Path,
    codegen_args: &[String],
) -> Result<()> {
    let asm = fs::read_to_string(asm_path)
        .with_context(|| format!("Failed to read assembly at {}", asm_path.display()))?;

    // Only the two object formats whose section syntax this module emits, and
    // whose section the runtime knows how to find, can be rewritten.
    //
    // Assembling unchanged on anything else looks like a graceful degradation
    // and is the opposite: the object would carry LLVM's `__llvm_stackmaps`
    // and no `__perry_gcmap`, the runtime reads only the compact section, and
    // the collector finds no native roots at all — the exact outcome the hard
    // error below exists to prevent, reached with no diagnostic. The mode is
    // opt-in, so refusing loudly costs nothing.
    // Architectures whose frame bases this runtime can resolve. x86-64 joined
    // aarch64 once SP-relative roots stopped going through
    // `_Unwind_GetGR(SP)` — not a supported query, and the garbage it returned
    // is what the collector wrote through — and started deriving the base from
    // `_Unwind_GetCFA` instead.
    //
    // Still a deny-list rather than an allow-anything: a target whose bases the
    // runtime cannot resolve must fail the compile, because the alternative is
    // a binary that segfaults during collection with no diagnostic.
    // `arm64_32` (watchOS) is excluded deliberately, and before `arm64`: it has
    // 32-bit pointers, while the map stores function addresses as `u64` and the
    // runtime does `usize` arithmetic on them. The runtime's loader is gated to
    // 64-bit Apple for the same reason, so admitting it here would emit a map
    // nothing reads — roots lost silently on the platform hardest to debug.
    let arch_supported = target.starts_with("aarch64")
        || target.starts_with("arm64")
        || target.starts_with("x86_64");
    // No pointer-width refusal here on purpose. watchOS `arm64_32` is ILP32,
    // and the emitter handles that by following the target's width for the
    // function-address field (see `ptr64` in `compact_stack_map_asm`) rather
    // than assuming 8 bytes — so a narrow pointer is a supported width, not an
    // excluded target. This spot used to recompute that predicate and never
    // read it, which read like a guard that had been defeated.
    if !arch_supported {
        return Err(anyhow!(
            "perry: native GC roots (PERRY_RS4GC) are not supported for target \
             `{target}` — its roots are recorded against frame bases this \
             runtime cannot resolve, and the collector would segfault rather \
             than report anything. Tracked for #7173."
        ));
    }
    // Windows x86-64 is enabled (#7354): the runtime walks native frames there
    // with `RtlVirtualUnwind` (`gc/roots/stack_maps.rs`), verified on a real
    // Windows host against the pinned oracle with non-zero walk telemetry.
    //
    // ARM64 Windows stays refused. It passes the `arch_supported` check above
    // (aarch64) and it is COFF, but the runtime's Windows walker is x86-64
    // only — the `CONTEXT` layout and the unwinder's register model differ on
    // ARM64 — so that combination still has NO walker and falls to the stub
    // that visits nothing. Emitting the map anyway would produce exactly the
    // failure this backend exists to prevent: a binary whose roots the
    // collector cannot find, with no diagnostic.
    if matches!(format_for(target), ObjectFormat::Coff) && !target.starts_with("x86_64") {
        return Err(anyhow!(
            "perry: native GC roots (PERRY_RS4GC) are not enabled for target \
             `{target}` yet — the COFF section and its PE lookup exist, but the \
             runtime's Windows stack walker is x86-64 only, so no frame would \
             ever be visited and the collector would free live objects. \
             Tracked for #7173."
        ));
    }

    let compacted = compact_stack_map_asm(&asm, target).map_err(|reason| {
        anyhow!(
            "perry: this module emits an LLVM stack map that the compact-map \
             rewriter could not parse, so its GC roots would be invisible to \
             the collector (the runtime reads only the compact section). \
             Refusing to emit a binary that would lose roots silently.\n\
             \n\
             reason: {reason}\n\
             target: {target}\n\
             assembly left at: {}",
            asm_path.display()
        )
    })?;
    if let Some((rewritten, stats)) = compacted {
        fs::write(asm_path, rewritten).with_context(|| {
            format!(
                "Failed to write compacted assembly at {}",
                asm_path.display()
            )
        })?;
        log::debug!(
            "perry-codegen: gc map {} -> {} bytes ({} functions, {} records, {} roots)",
            stats.original_bytes,
            stats.compact_bytes,
            stats.functions,
            stats.records,
            stats.roots,
        );
        // The statepoint report's safepoint counts come from here and nowhere
        // else. Perry does not choose which calls become safepoints — RS4GC
        // does, inside LLVM — so this parse of the emitted assembly is the
        // only place the real numbers exist.
        crate::statepoint_report::note_gc_map(stats.functions, stats.records, stats.roots);
    }

    assemble(clang, target, asm_path, obj_path, codegen_args)
}

/// The subset of the codegen's clang argv that selects the target MACHINE.
///
/// The assembler must be told the same machine the code generator was told, or
/// it rejects instructions the generator legitimately emitted. Optimisation and
/// output flags are excluded: they mean nothing to an assembler, and forwarding
/// them wholesale would be a second way for the two invocations to disagree.
fn cpu_selection_flags(codegen_args: &[String]) -> Vec<&String> {
    codegen_args
        .iter()
        .filter(|a| a.starts_with("-mcpu=") || a.starts_with("-march=") || a.starts_with("-mtune="))
        .collect()
}

fn assemble(
    clang: &Path,
    target: &str,
    asm_path: &Path,
    obj_path: &Path,
    codegen_args: &[String],
) -> Result<()> {
    // Mirror the codegen's CPU selection onto the assembler.
    //
    // Perry compiles with `-mcpu=native`, so on a host with SVE (Graviton, and
    // any aarch64 server part) LLVM emits SVE instructions -- `mov z1.d, #…`.
    // Assembling that text with a clang that was given no `-mcpu` fails with
    // `instruction requires: sve or sme`, because the assembler defaults to the
    // portable baseline while the code generator did not.
    //
    // The two invocations describe the SAME machine and must agree. Forwarding
    // only the CPU-selection flags keeps that contract without dragging along
    // optimisation or output flags, which mean nothing to an assembler.
    let cpu_flags = cpu_selection_flags(codegen_args);
    let output = Command::new(clang)
        .arg("-c")
        .arg(asm_path)
        .arg("-o")
        .arg(obj_path)
        .args(&cpu_flags)
        .arg("-target")
        .arg(target)
        .output()
        .with_context(|| format!("Failed to invoke {}", clang.display()))?;
    if !output.status.success() {
        return Err(anyhow!(
            "assembling the compacted stack map failed (status={}).\n\
             assembly left at: {}\n\
             \n\
             stderr:\n{}",
            output.status,
            asm_path.display(),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let _ = fs::remove_file(asm_path);
    Ok(())
}

#[cfg(test)]
#[path = "gc_map_tests.rs"]
mod tests;
