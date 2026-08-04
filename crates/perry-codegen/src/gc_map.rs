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
const GC_MAP_VERSION: u8 = 3;
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
    let mut end_line = lines.len();

    for (index, raw) in lines.iter().enumerate().skip(start_line + 1) {
        let line = raw.trim();
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
    }

    Ok(RawBlock {
        start_line,
        end_line,
        bytes,
        symbols,
    })
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

                let mut roots: Vec<(u16, i32)> = Vec::new();
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
                    // Keep exactly what the collector keeps: 8-byte frame
                    // slots, with the base/derived pair collapsed to one.
                    if matches!(kind, LOCATION_DIRECT | LOCATION_INDIRECT) && size == 8 {
                        if !roots.contains(&(dwarf_reg, offset)) {
                            roots.push((dwarf_reg, offset));
                        }
                    }
                    pos += 12;
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
                records.push(Record {
                    instruction_offset,
                    roots,
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

fn encode_stream(functions: &[FunctionMap]) -> Vec<u8> {
    let mut stream = Vec::new();
    for function in functions {
        let mut previous_roots: Option<&Vec<(u16, i32)>> = None;
        for record in &function.records {
            if previous_roots == Some(&record.roots) {
                // Repeat flag: the live set is the previous record's.
                push_varint(&mut stream, 1);
                continue;
            }
            push_varint(&mut stream, (record.roots.len() as u64) << 1);

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
            let mut previous: Option<i32> = None;
            for (reg, offset) in &record.roots {
                let tag = match *reg {
                    DWARF_REG_FP_AARCH64 => 0u64,
                    DWARF_REG_SP_AARCH64 => 1,
                    _ => 2,
                };
                let delta = match previous {
                    None => *offset,
                    Some(prev) => offset.wrapping_sub(prev),
                };
                push_varint(&mut stream, (zigzag(delta) << 2) | tag);
                if tag == 2 {
                    push_varint(&mut stream, u64::from(*reg));
                }
                previous = Some(*offset);
            }
            previous_roots = Some(&record.roots);
        }
    }
    stream
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
fn verify_roundtrip(functions: &[FunctionMap], stream: &[u8]) -> Result<(), String> {
    let mut cursor = 0usize;
    for function in functions {
        let mut previous: Option<Vec<(u16, i32)>> = None;
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
                let count = (header >> 1) as usize;
                let mut roots = Vec::with_capacity(count);
                let mut last: Option<i32> = None;
                for root in 0..count {
                    let (value, next) = read_varint(stream, cursor)
                        .ok_or_else(|| format!("{}: truncated root {root}", where_()))?;
                    cursor = next;
                    let dwarf_reg = match value & 3 {
                        0 => DWARF_REG_FP_AARCH64,
                        1 => DWARF_REG_SP_AARCH64,
                        2 => {
                            let (reg, next) = read_varint(stream, cursor).ok_or_else(|| {
                                format!("{}: truncated explicit register for root {root}", where_())
                            })?;
                            cursor = next;
                            u16::try_from(reg).map_err(|_| {
                                format!("{}: root {root} register {reg} exceeds u16", where_())
                            })?
                        }
                        tag => {
                            return Err(format!(
                                "{}: root {root} has reserved base tag {tag}",
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
                    roots.push((dwarf_reg, offset));
                }
                roots
            };
            if decoded != record.roots {
                return Err(format!(
                    "{}: the compact stream decodes to {decoded:?} but the stack map recorded \
                     {:?}. Re-encoding changed this safepoint's live set, so the collector would \
                     scan different words than LLVM described.",
                    where_(),
                    record.roots
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
///   4  u8 version, u8 reserved, u16 reserved
///   8  u32 function_count
///  12  u32 total_len          -- lets the runtime walk concatenated blobs
///  16  function_count x { u64 address, u32 stack_size, u32 record_count }
///      record_count_total x u32 instruction_offset
///      varint root stream (see `encode_stream`)
/// ```
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
fn emit_asm(functions: &[FunctionMap], stream: &[u8], elf: bool, ptr64: bool) -> String {
    let record_total: usize = functions.iter().map(|f| f.records.len()).sum();
    let addr_bytes = if ptr64 { 8 } else { 4 };
    let entry_bytes = addr_bytes + 8; // address + u32 stack_size + u32 records
    let total_len = 16 + functions.len() * entry_bytes + record_total * 4 + stream.len();
    let mut out = String::new();
    if elf {
        out.push_str(&format!("\t.section\t{ELF_SECTION}\n"));
    } else {
        out.push_str(&format!("\t.section\t{MACHO_SECTION}\n"));
    }
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
fn compact_stack_map_asm(
    asm: &str,
    elf: bool,
    target: &str,
) -> Result<Option<(String, GcMapStats)>, String> {
    let lines: Vec<&str> = asm.lines().collect();
    if find_block_start(&lines).is_none() {
        return Ok(None);
    }
    // watchOS is ILP32: the relocated function-address field follows the
    // target's pointer width rather than assuming 8 bytes.
    let ptr64 = !target.starts_with("arm64_32");
    let block = parse_block(&lines, word_width_for(target))?;
    let functions = decode_v3(&block)?;
    let stream = encode_stream(&functions);
    verify_roundtrip(&functions, &stream)?;

    let stats = GcMapStats {
        original_bytes: block.bytes.len(),
        compact_bytes: 16
            + functions.len() * (if ptr64 { 16 } else { 12 })
            + functions.iter().map(|f| f.records.len()).sum::<usize>() * 4
            + stream.len(),
        functions: functions.len(),
        records: functions.iter().map(|f| f.records.len()).sum(),
        roots: functions
            .iter()
            .flat_map(|f| f.records.iter())
            .map(|r| r.roots.len())
            .sum(),
    };

    let replacement = emit_asm(&functions, &stream, elf, ptr64);
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
    for line in &lines[block.end_line..] {
        if line.contains("__LLVM_StackMaps") {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    Ok(Some((out, stats)))
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
    // watchOS is ILP32. The map's function-address field follows the target's
    // pointer width rather than assuming 8 bytes, so `arm64_32` is a supported
    // width here, not an excluded target.
    let ptr64 = !target.starts_with("arm64_32");
    if !arch_supported {
        return Err(anyhow!(
            "perry: native GC roots (PERRY_RS4GC) are not supported for target \
             `{target}` — its roots are recorded against frame bases this \
             runtime cannot resolve, and the collector would segfault rather \
             than report anything. Tracked for #7173."
        ));
    }
    let macho = target.contains("apple") || target.contains("darwin");
    let elf = !macho && !target.contains("windows") && !target.contains("msvc");
    if !macho && !elf {
        return Err(anyhow!(
            "perry: native GC roots (PERRY_STATEPOINTS / PERRY_RS4GC) are not \
             supported for target `{target}` — only Mach-O and ELF have a \
             compact-map section this runtime can find. Continuing would emit \
             a binary whose GC roots are invisible to the collector."
        ));
    }

    let compacted = compact_stack_map_asm(&asm, elf, target).map_err(|reason| {
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
    }

    assemble(clang, target, asm_path, obj_path)
}

fn assemble(clang: &Path, target: &str, asm_path: &Path, obj_path: &Path) -> Result<()> {
    let output = Command::new(clang)
        .arg("-c")
        .arg(asm_path)
        .arg("-o")
        .arg(obj_path)
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
mod tests {
    use super::*;

    /// Build a minimal but structurally real v3 block: one function, one
    /// record, two roots of which the second is the base/derived duplicate.
    fn sample_asm() -> String {
        let mut asm = String::new();
        asm.push_str("\t.no_dead_strip\t__LLVM_StackMaps\n");
        asm.push_str("\t.section\t__LLVM_STACKMAPS,__llvm_stackmaps\n");
        asm.push_str("__LLVM_StackMaps:\n");
        asm.push_str("\t.byte\t3\n\t.byte\t0\n\t.short\t0\n");
        asm.push_str("\t.long\t1\n"); // functions
        asm.push_str("\t.long\t0\n"); // constants
        asm.push_str("\t.long\t1\n"); // records
        asm.push_str("\t.quad\t_probe_fn\n");
        asm.push_str("\t.quad\t144\n"); // stack size
        asm.push_str("\t.quad\t1\n"); // record count
                                      // record: id, instruction offset, reserved, location count
        asm.push_str("\t.quad\t0\n");
        asm.push_str("\t.long\t64\n");
        asm.push_str("\t.short\t0\n");
        asm.push_str("\t.short\t4\n");
        // three statepoint preamble constants, then base/derived pair
        for _ in 0..3 {
            asm.push_str(
                "\t.byte\t4\n\t.byte\t0\n\t.short\t8\n\t.short\t0\n\t.short\t0\n\t.long\t0\n",
            );
        }
        asm.push_str(
            "\t.byte\t3\n\t.byte\t0\n\t.short\t8\n\t.short\t29\n\t.short\t0\n\t.long\t4294967272\n",
        );
        asm.push_str("\t.p2align\t3\n");
        asm.push_str("\t.short\t0\n\t.short\t0\n"); // live-out header
        asm.push_str("\t.p2align\t3\n");
        asm.push_str("\t.subsections_via_symbols\n");
        asm
    }

    /// The same map an **ELF** backend prints. Captured from
    /// `perry --target linux` on `08_map_set_sidetables.ts`: AArch64/ELF
    /// spells the stack map's fields `.hword` / `.word` / `.xword`, not
    /// `.short` / `.long` / `.quad`.
    ///
    /// One function, one record, and — critically — a `.word` **instruction
    /// offset** and a `.word` **32-bit `Offset` field per location**, which is
    /// what makes the width of `.word` load-bearing rather than cosmetic.
    #[test]
    fn ilp32_targets_emit_a_pointer_sized_address_field() {
        // watchOS `arm64_32` is ILP32. An 8-byte address slot there would need
        // a relocation ld64 has no reason to emit, and the runtime would read
        // two pointers as one — so the field follows the target's width and
        // the header records which width was used.
        let (out, stats) = compact_stack_map_asm(&sample_asm(), false, "arm64_32-apple-watchos")
            .expect("an ILP32 stack map must parse")
            .expect("an ILP32 stack map must be rewritten");
        assert!(
            out.contains("\t.long\t_probe_fn"),
            "the function address must be pointer-sized on ILP32:\n{out}"
        );
        assert!(
            !out.contains("\t.quad\t_probe_fn"),
            "an 8-byte address slot on ILP32 is the bug this guards:\n{out}"
        );
        assert!(
            out.contains("\t.short\t0\n"),
            "the header must record a 32-bit address width:\n{out}"
        );
        // 16-byte header + one 12-byte function entry (4-byte address on
        // ILP32) + one 4-byte instruction offset + a 3-byte root stream. The
        // LP64 form of the same map is 4 bytes larger, which is the whole
        // point of the field being pointer-sized.
        assert_eq!(stats.compact_bytes, 16 + 12 + 4 + 3);
    }

    #[test]
    fn lp64_targets_keep_the_eight_byte_address_field() {
        let (out, _) = compact_stack_map_asm(&sample_asm(), false, "arm64-apple-ios")
            .expect("an LP64 stack map must parse")
            .expect("an LP64 stack map must be rewritten");
        assert!(out.contains("\t.quad\t_probe_fn"), "{out}");
        assert!(
            out.contains("\t.short\t1\n"),
            "the header must record a 64-bit address width:\n{out}"
        );
    }

    fn aarch64_elf_sample_asm() -> String {
        let mut asm = String::new();
        asm.push_str("\t.section\t.llvm_stackmaps,\"a\",@progbits\n");
        asm.push_str("__LLVM_StackMaps:\n");
        asm.push_str("\t.byte\t3\n\t.byte\t0\n\t.hword\t0\n");
        asm.push_str("\t.word\t1\n"); // functions
        asm.push_str("\t.word\t0\n"); // constants
        asm.push_str("\t.word\t1\n"); // records
        asm.push_str("\t.xword\tprobe_fn\n");
        asm.push_str("\t.xword\t112\n"); // stack size
        asm.push_str("\t.xword\t1\n"); // record count
        asm.push_str("\t.xword\t0\n"); // patchpoint id
        asm.push_str("\t.word\t.Ltmp0-probe_fn\n"); // instruction offset
        asm.push_str("\t.hword\t0\n");
        asm.push_str("\t.hword\t4\n"); // location count
        for _ in 0..3 {
            asm.push_str(
                "\t.byte\t4\n\t.byte\t0\n\t.hword\t8\n\t.hword\t0\n\t.hword\t0\n\t.word\t0\n",
            );
        }
        // The live root: SP-relative (DWARF 31), frame offset 24.
        asm.push_str(
            "\t.byte\t3\n\t.byte\t0\n\t.hword\t8\n\t.hword\t31\n\t.hword\t0\n\t.word\t24\n",
        );
        asm.push_str("\t.p2align\t3\n");
        asm.push_str("\t.hword\t0\n\t.hword\t0\n"); // live-out header
        asm.push_str("\t.p2align\t3\n");
        asm.push_str("\t.section\t\".note.GNU-stack\",\"\",@progbits\n");
        asm
    }

    /// An ELF stack map must decode with the SAME roots the Mach-O spelling
    /// would give. `.word` is 4 bytes here and 2 bytes on x86 — a fixed table
    /// gets one of the two silently wrong, and "silently" is the whole problem:
    /// two bytes of drift per field relocates every root after it, so the
    /// module either refuses for an unrelated-looking reason or, worse,
    /// compacts a live set read from the wrong bytes.
    #[test]
    fn aarch64_elf_word_directives_decode_to_the_right_root() {
        let (out, stats) =
            compact_stack_map_asm(&aarch64_elf_sample_asm(), true, "aarch64-unknown-linux-gnu")
                .expect("an aarch64-ELF stack map must parse")
                .expect("an aarch64-ELF stack map must be rewritten");
        assert_eq!(stats.functions, 1);
        assert_eq!(stats.records, 1);
        // Four locations in, one root out: the three preamble constants drop.
        assert_eq!(stats.roots, 1, "the SP-relative root must survive");
        assert!(out.contains("_perry_gc_map:"));
        assert!(out.contains(".quad\tprobe_fn"));
        assert!(!out.contains("llvm_stackmaps"));
    }

    /// Reading that same ELF map with x86's `.word` (2 bytes) must not quietly
    /// produce a different answer. This is the assertion that the width is
    /// load-bearing: if `.word` were hardcoded, this test and the one above
    /// could not both hold.
    #[test]
    fn word_width_is_load_bearing_not_cosmetic() {
        assert_eq!(word_width_for("aarch64-unknown-linux-gnu"), 4);
        assert_eq!(word_width_for("arm64-apple-macosx15.0.0"), 4);
        assert_eq!(word_width_for("x86_64-unknown-linux-gnu"), 2);
        assert_eq!(word_width_for("x86_64h-apple-macosx15.0.0"), 2);
        assert_eq!(word_width_for("i686-unknown-linux-gnu"), 2);
        assert_eq!(word_width_for("i386-unknown-linux-gnu"), 2);
        // Not x86: `aarch64` must not be mistaken for one by a loose match.
        assert_eq!(word_width_for("riscv64gc-unknown-linux-gnu"), 4);

        let asm = aarch64_elf_sample_asm();
        let correct = compact_stack_map_asm(&asm, true, "aarch64-unknown-linux-gnu")
            .expect("parses under the right width")
            .expect("rewritten");
        let wrong = compact_stack_map_asm(&asm, true, "x86_64-unknown-linux-gnu");
        match wrong {
            // Either it refuses, or it decodes to something different. What it
            // must NOT do is agree — that would mean the width never mattered
            // and this guard is asserting nothing.
            Err(_) => {}
            Ok(None) => panic!("the block must not vanish"),
            Ok(Some((_, stats))) => assert_ne!(
                stats.roots, correct.1.roots,
                "decoding an ELF aarch64 map with x86's .word width agreed with the correct \
                 width — the width is not actually being used"
            ),
        }
    }

    #[test]
    fn compacts_and_keeps_only_real_roots() {
        let (out, stats) = compact_stack_map_asm(&sample_asm(), false, "arm64-apple-macosx15.0.0")
            .expect("block parses")
            .expect("block rewritten");
        assert_eq!(stats.functions, 1);
        assert_eq!(stats.records, 1);
        // Four locations in, one root out: three constants dropped.
        assert_eq!(stats.roots, 1);
        assert!(
            stats.compact_bytes < stats.original_bytes,
            "compact {} should beat original {}",
            stats.compact_bytes,
            stats.original_bytes
        );
        assert!(out.contains("_perry_gc_map:"));
        assert!(out.contains(".quad\t_probe_fn"));
        // The old section must be gone, and nothing may still name its label.
        assert!(!out.contains("__llvm_stackmaps"));
        assert!(!out.contains("__LLVM_StackMaps"));
        // The dead-strip guard must survive, retargeted.
        assert!(out.contains(".no_dead_strip\t_perry_gc_map"));
    }

    #[test]
    fn repeated_live_sets_cost_one_byte() {
        let shared = vec![(29u16, -24i32), (29, -32)];
        let functions = vec![FunctionMap {
            symbol: "_f".to_string(),
            stack_size: 64,
            records: vec![
                Record {
                    instruction_offset: "0".to_string(),
                    roots: shared.clone(),
                },
                Record {
                    instruction_offset: "8".to_string(),
                    roots: shared.clone(),
                },
                Record {
                    instruction_offset: "16".to_string(),
                    roots: shared,
                },
            ],
        }];
        let one_record = vec![FunctionMap {
            symbol: functions[0].symbol.clone(),
            stack_size: functions[0].stack_size,
            records: functions[0].records[..1].to_vec(),
        }];
        // Offsets live in their own fixed-width array now, so in the varint
        // stream the two extra records cost exactly one repeat byte each,
        // regardless of how many roots the shared live set holds.
        assert_eq!(
            encode_stream(&functions).len(),
            encode_stream(&one_record).len() + 2
        );
    }

    #[test]
    fn encodes_a_foreign_register_base() {
        // A base that is neither FP nor SP is real: LLVM uses x19 as a frame
        // base pointer in functions with dynamic stack allocation. The 2-bit
        // tag carries the DWARF number explicitly rather than refusing — the
        // format must never be the reason a root is unrepresentable.
        let asm = sample_asm().replace(
            "\t.byte\t3\n\t.byte\t0\n\t.short\t8\n\t.short\t29\n",
            "\t.byte\t3\n\t.byte\t0\n\t.short\t8\n\t.short\t19\n",
        );
        let (out, stats) = compact_stack_map_asm(&asm, true, "aarch64-unknown-linux-gnu")
            .expect("block parses")
            .expect("a foreign base must still encode");
        assert_eq!(stats.roots, 1);
        assert!(out.contains("_perry_gc_map:"));
    }

    /// The round-trip check must be able to FAIL. A verifier that only ever
    /// agrees with itself is CLAUDE.md's fourth gate-failure mode — the gate
    /// runs, its subject never did — so plant each way the stream can lie and
    /// assert the check catches it rather than merely that a clean stream
    /// passes.
    #[test]
    fn roundtrip_check_catches_a_corrupted_stream() {
        let functions = vec![FunctionMap {
            symbol: "probe".to_string(),
            stack_size: 96,
            // x86-64 shape: RSP (DWARF 7) base, ascending frame offsets.
            records: vec![
                Record {
                    instruction_offset: "0".to_string(),
                    roots: vec![(7, 8), (7, 24), (7, 40)],
                },
                Record {
                    instruction_offset: "16".to_string(),
                    roots: vec![(7, 8), (7, 24), (7, 40)],
                },
            ],
        }];
        let stream = encode_stream(&functions);
        verify_roundtrip(&functions, &stream).expect("a clean stream must verify");

        // A dropped root: the header's count is the first byte of the stream.
        let mut short = stream.clone();
        short[0] = 2 << 1;
        assert!(
            verify_roundtrip(&functions, &short).is_err(),
            "a stream claiming fewer roots than the map recorded must be rejected"
        );

        // A moved root: perturbing a delta keeps the count but relocates the
        // slot, which is the shape that makes the collector scan wrong words.
        let mut moved = stream.clone();
        moved[1] = moved[1].wrapping_add(4);
        assert!(
            verify_roundtrip(&functions, &moved).is_err(),
            "a stream that relocates a root must be rejected"
        );

        // Truncation.
        assert!(
            verify_roundtrip(&functions, &stream[..stream.len() - 1]).is_err(),
            "a truncated stream must be rejected"
        );

        // Trailing bytes: decodes cleanly and still means the two sides
        // disagree about the layout.
        let mut trailing = stream.clone();
        trailing.push(0);
        assert!(
            verify_roundtrip(&functions, &trailing).is_err(),
            "a stream with unconsumed trailing bytes must be rejected"
        );
    }

    #[test]
    fn no_stack_map_block_is_left_alone() {
        // No block at all is `Ok(None)` — nothing to compact, not a failure.
        assert!(compact_stack_map_asm(
            "\t.section\t__TEXT,__text\n\tret\n",
            false,
            "arm64-apple-macosx15.0.0"
        )
        .expect("no block is not an error")
        .is_none());
    }

    #[test]
    fn unparsable_block_is_an_error_not_a_silent_skip() {
        // Truncated header. This must be `Err`, never `Ok(None)`: the caller
        // turns `Err` into a refusal and `Ok(None)` into "assemble unchanged",
        // and assembling unchanged here ships a binary whose roots the
        // collector cannot see.
        let asm = "\t.section\t__LLVM_STACKMAPS,__llvm_stackmaps\n\t.byte\t3\n";
        let error = compact_stack_map_asm(asm, false, "arm64-apple-macosx15.0.0")
            .expect_err("truncated block must error");
        assert!(
            error.contains("no function records") || error.contains("past the end"),
            "unhelpful reason: {error}"
        );
    }
}
