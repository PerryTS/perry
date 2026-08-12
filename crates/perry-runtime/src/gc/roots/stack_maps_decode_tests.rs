//! Decoder and record-matching tests for `stack_maps.rs`.
//!
//! Its own file for the same reason `stack_maps_unwind_contract.rs` is: the
//! parent is at the 2000-line cap, and a test that cannot be added without
//! deleting production commentary is a test that does not get added.

#[cfg(test)]
mod tests {
    use super::super::*;

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

    /// Build one compact blob, mirroring `perry-codegen/src/gc_map.rs`.
    /// `records` is `(instruction_offset, roots)`, roots as `(dwarf_reg, offset)`;
    /// an empty root slice with `repeat` set encodes the repeat flag.
    fn one_map(function: u64, records: &[(u32, Vec<(u16, i32)>, bool)]) -> Vec<u8> {
        let mut offsets = Vec::new();
        let mut stream = Vec::new();
        for (instruction_offset, roots, repeat) in records {
            offsets.extend_from_slice(&instruction_offset.to_le_bytes());
            if *repeat {
                push_varint(&mut stream, 1);
                continue;
            }
            push_varint(&mut stream, (roots.len() as u64) << 1);
            let mut last: Option<i32> = None;
            for (reg, offset) in roots {
                let tag = match *reg {
                    DWARF_REG_FP_AARCH64 => 0u64,
                    DWARF_REG_SP_AARCH64 => 1,
                    _ => 2,
                };
                let delta = match last {
                    None => *offset,
                    Some(previous) => offset.wrapping_sub(previous),
                };
                push_varint(&mut stream, (zigzag(delta) << 2) | tag);
                if tag == 2 {
                    push_varint(&mut stream, u64::from(*reg));
                }
                last = Some(*offset);
            }
        }

        // Build for THIS host's pointer width, mirroring the emitter: the
        // decoder rejects a blob whose recorded width disagrees with its own.
        let ptr64 = std::mem::size_of::<usize>() == 8;
        let entry = if ptr64 { 16 } else { 12 };
        let total_len = 16 + entry + offsets.len() + stream.len();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(GC_MAP_MAGIC);
        bytes.push(GC_MAP_VERSION);
        bytes.push(0);
        bytes.extend_from_slice(&u16::from(ptr64).to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&(total_len as u32).to_le_bytes());
        if ptr64 {
            bytes.extend_from_slice(&function.to_le_bytes());
        } else {
            bytes.extend_from_slice(&(function as u32).to_le_bytes());
        }
        bytes.extend_from_slice(&32u32.to_le_bytes());
        bytes.extend_from_slice(&(records.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&offsets);
        bytes.extend_from_slice(&stream);
        while bytes.len() % 8 != 0 {
            bytes.push(0);
        }
        bytes
    }

    fn simple(function: u64, offset: u32, frame_offset: i32) -> Vec<u8> {
        one_map(function, &[(offset, vec![(29, frame_offset)], false)])
    }

    #[test]
    fn decodes_frame_location() {
        let bytes = simple(0x1000, 0x10, -8);
        let (records, roots) = parse_gc_map(&bytes).expect("valid map");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].pc, 0x1010);
        assert_eq!(records[0].function_address, 0x1000);
        assert_eq!(records[0].stack_size, 32);
        assert_eq!(
            roots,
            vec![StackMapLocation {
                dwarf_reg: 29,
                offset: -8,
            }]
        );
    }

    #[test]
    fn decodes_linker_concatenated_input_sections() {
        let mut bytes = simple(0x1000, 0x10, -8);
        bytes.extend_from_slice(&simple(0x2000, 0x20, -16));
        let (records, _) = parse_gc_map(&bytes).expect("concatenated maps");
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].pc, 0x1010);
        assert_eq!(records[1].pc, 0x2020);
    }

    #[test]
    fn repeated_live_sets_share_one_copy() {
        // Three safepoints, the last two repeating the first's live set: the
        // whole point of the format, and the reason the in-memory index does
        // not hold 154k duplicated entries on a real application.
        let bytes = one_map(
            0x1000,
            &[
                (0x10, vec![(29, -8), (29, -16)], false),
                (0x20, vec![], true),
                (0x30, vec![], true),
            ],
        );
        let (records, roots) = parse_gc_map(&bytes).expect("valid map");
        assert_eq!(records.len(), 3);
        assert_eq!(roots.len(), 2, "the repeats must not append new roots");
        for record in &records {
            assert_eq!(record.roots_start, 0);
            assert_eq!(record.roots_len, 2);
        }
    }

    #[test]
    fn decodes_negative_and_ascending_root_offsets() {
        let bytes = one_map(0x1000, &[(0, vec![(29, -64), (29, -8), (31, 24)], false)]);
        let (_, roots) = parse_gc_map(&bytes).expect("valid map");
        assert_eq!(
            roots,
            vec![
                StackMapLocation {
                    dwarf_reg: 29,
                    offset: -64
                },
                StackMapLocation {
                    dwarf_reg: 29,
                    offset: -8
                },
                StackMapLocation {
                    dwarf_reg: 31,
                    offset: 24
                },
            ]
        );
    }

    #[test]
    fn decodes_an_explicit_base_register() {
        // LLVM uses x19 as a frame base pointer in functions with dynamic
        // stack allocation — 66 root slots in one real module. A single FP/SP
        // bit cannot express that, which is what forced the 2-bit base tag.
        let bytes = one_map(0x1000, &[(0x10, vec![(19, -40), (29, -8)], false)]);
        let (_, roots) = parse_gc_map(&bytes).expect("valid map");
        assert_eq!(
            roots,
            vec![
                StackMapLocation {
                    dwarf_reg: 19,
                    offset: -40
                },
                StackMapLocation {
                    dwarf_reg: 29,
                    offset: -8
                },
            ]
        );
    }

    #[test]
    fn an_explicit_base_register_disables_the_fast_walk() {
        // The x29-chain walker can only recover FP and SP; anything else must
        // fall back to the platform unwinder, which can.
        let index = index_records(
            vec![StackMapRecord {
                pc: 0x1000,
                function_address: 0x1000,
                stack_size: 64,
                roots_start: 0,
                roots_len: 1,
            }],
            vec![StackMapLocation {
                dwarf_reg: 19,
                offset: -40,
            }],
        );
        assert!(!index.chain_walkable);
    }

    #[test]
    fn rejects_a_blob_built_for_the_other_pointer_width() {
        // The header records the width the emitter used. A blob claiming the
        // other width would have every function address misread, so it must be
        // refused rather than decoded — watchOS `arm64_32` is ILP32 while every
        // other supported target is LP64.
        let mut bytes = simple(0x1000, 0x10, -8);
        let flags = u16::from_le_bytes([bytes[6], bytes[7]]);
        bytes[6..8].copy_from_slice(&(flags ^ 1).to_le_bytes());
        assert!(
            parse_gc_map(&bytes).is_none(),
            "a map built for the other pointer width must be refused"
        );
    }

    #[test]
    fn rejects_a_blob_whose_length_cannot_advance_the_cursor() {
        // `total_len` comes straight from the header. A zero (or too-small)
        // value leaves `base` where it was, and because the magic still
        // matches there the resync path never runs — the loop spins forever
        // inside `OnceLock::get_or_init`, hanging the process at the first
        // collection instead of failing closed.
        let mut bytes = simple(0x1000, 0x10, -8);
        bytes[12..16].copy_from_slice(&0u32.to_le_bytes());
        assert!(
            parse_gc_map(&bytes).is_none(),
            "a blob that cannot advance the cursor must be rejected, not looped on"
        );

        // Long enough to look plausible, still short of header + function table.
        let mut bytes = simple(0x1000, 0x10, -8);
        bytes[12..16].copy_from_slice(&20u32.to_le_bytes());
        assert!(parse_gc_map(&bytes).is_none());
    }

    #[test]
    fn rejects_a_truncated_function_table() {
        // The record counts size the fixed-width offset array; a short read
        // there must not be rounded down to zero, or every later varint is
        // decoded from the wrong offset.
        let bytes = simple(0x1000, 0x10, -8);
        let truncated = &bytes[..20];
        assert!(parse_gc_map(truncated).is_none());
    }

    #[test]
    fn rejects_truncated_or_wrong_version_sections() {
        assert!(parse_gc_map(&[]).is_none() || parse_gc_map(&[]).unwrap().0.is_empty());
        let mut bytes = simple(0x1000, 0x10, -8);
        bytes[4] = GC_MAP_VERSION + 1;
        assert!(
            parse_gc_map(&bytes).is_none(),
            "an unknown version must not be guessed at"
        );
        // A total_len that runs past the section must fail rather than read on.
        let mut bytes = simple(0x1000, 0x10, -8);
        let len = bytes.len();
        bytes[12..16].copy_from_slice(&((len as u32) + 64).to_le_bytes());
        assert!(parse_gc_map(&bytes).is_none());
    }

    #[test]
    fn chain_walkable_index_accepts_fp_and_sp_locations_only() {
        let rec = |pc: usize| StackMapRecord {
            pc,
            function_address: pc,
            stack_size: 160,
            roots_start: 0,
            roots_len: 1,
        };
        // FP and SP are both walkable: SP resolves per frame by decoding the
        // owning function's prologue (#7173).
        let walkable = index_records(
            vec![rec(0x1000), rec(0x2000)],
            vec![
                StackMapLocation {
                    dwarf_reg: DWARF_REG_FP_AARCH64,
                    offset: -8,
                },
                StackMapLocation {
                    dwarf_reg: DWARF_REG_SP_AARCH64,
                    offset: -8,
                },
            ],
        );
        assert!(walkable.chain_walkable);
        assert_eq!(walkable.min_pc, 0x1000);
        assert_eq!(walkable.max_pc, 0x2000);
        // Any other register disqualifies the whole image.
        assert!(
            !index_records(
                vec![rec(0x1000)],
                vec![StackMapLocation {
                    dwarf_reg: 1,
                    offset: -8
                }],
            )
            .chain_walkable,
            "a non-FP/SP register must disable the fast walk"
        );
    }

    #[test]
    fn rejects_a_record_from_an_adjacent_function() {
        // A safepoint at the end of A must not be matched for an `ip` early in
        // B just because it falls inside the +-16 window: the walker would use
        // A's frame offsets against B's frame.
        let index = index_records(
            vec![
                StackMapRecord {
                    pc: 0x1ffc,
                    function_address: 0x1000,
                    stack_size: 32,
                    roots_start: 0,
                    roots_len: 1,
                },
                StackMapRecord {
                    pc: 0x2040,
                    function_address: 0x2000,
                    stack_size: 32,
                    roots_start: 0,
                    roots_len: 1,
                },
            ],
            vec![StackMapLocation {
                dwarf_reg: 29,
                offset: -8,
            }],
        );
        // 0x2004 is 8 bytes past A's last safepoint but lives in B.
        assert!(
            index.match_records(0x2004).is_empty(),
            "a record from the previous function must not match"
        );
        // A same-function near-match is still accepted — requiring an exact pc
        // would drop it, and the measured suite has one.
        assert_eq!(index.match_records(0x2038).len(), 1);
    }

    #[test]
    fn matches_plain_maps_before_and_statepoints_after_unwinder_ips() {
        let rec = |pc: usize| StackMapRecord {
            pc,
            function_address: pc,
            stack_size: 32,
            roots_start: 0,
            roots_len: 0,
        };
        let maps = vec![rec(0x1000), rec(0x1020)];
        assert_eq!(closest_record_pc(&maps, 0x1004), Some(0x1000));
        assert_eq!(closest_record_pc(&maps, 0x101c), Some(0x1020));
        assert_eq!(closest_record_pc(&maps, 0x1020), Some(0x1020));
    }
}

#[cfg(all(test, target_arch = "aarch64"))]
mod fp_offset_trailing_sub_tests {
    use super::super::fp_to_sp_offset;

    /// Assemble a prologue into executable-ish memory and decode it. The
    /// decoder only reads words, so a plain aligned buffer is enough.
    fn decode(words: &[u32]) -> Option<usize> {
        let buf = words.to_vec().into_boxed_slice();
        let addr = buf.as_ptr() as usize;
        let out = fp_to_sp_offset(addr);
        drop(buf);
        out
    }

    const ADD_X29_SP_0X90: u32 = 0x9102_43FD; // add x29, sp, #0x90
    const SUB_SP_SP_0X170: u32 = 0xD105_C3FF; // sub sp, sp, #0x170
    const RET: u32 = 0xD65F_03C0;
    const NOP: u32 = 0xD503_201F;

    // The three prologue words #7394 was measured on, read out of
    // `perry_fn_test_gap_gc_call_argument_rooting_ts__run` at +0x20:
    //
    //     9101c3fd   add x29, sp, #0x70
    //     d14007ff   sub sp, sp, #0x1, lsl #12
    //     d12103ff   sub sp, sp, #0x840
    const ADD_X29_SP_0X70: u32 = 0x9101_C3FD;
    const SUB_SP_SP_1_LSL12: u32 = 0xD140_07FF;
    const SUB_SP_SP_0X840: u32 = 0xD121_03FF;
    const ADD_X29_SP_2_LSL12: u32 = 0x9140_0BFD; // add x29, sp, #0x2, lsl #12

    /// #7328: `add x29, sp, #imm` is not always the last stack adjustment.
    /// LLVM emits a further `sub sp, sp, #N` after establishing the frame
    /// pointer, and reading only the `add` left the fast walker N bytes high
    /// on every slot in that frame — a silent wrong answer, since the walker
    /// then enumerated addresses the collector treated as roots.
    #[test]
    fn a_sub_after_the_fp_setup_is_included() {
        assert_eq!(
            decode(&[ADD_X29_SP_0X90, SUB_SP_SP_0X170, NOP, RET]),
            Some(0x90 + 0x170),
            "the trailing `sub sp, sp, #0x170` must be added to the fp offset"
        );
    }

    /// The common shape — fp established last — must be unchanged.
    #[test]
    fn a_prologue_with_no_trailing_sub_is_unchanged() {
        assert_eq!(decode(&[ADD_X29_SP_0X90, NOP, RET]), Some(0x90));
    }

    /// Only a contiguous run of `sub sp` immediately after the `add` counts.
    /// A later `sub sp` is a body operation (dynamic alloca, call-argument
    /// area) already accounted for by the stack map's own slot offsets.
    #[test]
    fn a_sub_after_the_prologue_run_is_not_counted() {
        assert_eq!(
            decode(&[ADD_X29_SP_0X90, NOP, SUB_SP_SP_0X170, RET]),
            Some(0x90),
            "a `sub sp` separated from the prologue run must not be folded in"
        );
    }

    /// A leaf that never sets up fp still fails closed, so the caller falls
    /// back to the platform unwinder rather than inventing an offset.
    #[test]
    fn a_leaf_without_fp_setup_still_fails_closed() {
        assert_eq!(decode(&[NOP, RET]), None);
    }

    /// #7394: a trailing `sub sp, sp, #imm, lsl #12` must contribute
    /// `imm << 12`. #7328's decoder masked the `sh` bit into the opcode
    /// comparison, so a shifted `sub` did not match at all.
    #[test]
    fn a_shifted_trailing_sub_is_included() {
        assert_eq!(
            decode(&[ADD_X29_SP_0X70, SUB_SP_SP_1_LSL12, NOP, RET]),
            Some(0x70 + 0x1000),
            "`sub sp, sp, #0x1, lsl #12` must contribute 4096, not 1"
        );
    }

    /// The measured shape. The shifted `sub` is not the last one, so failing
    /// to match it also **ended the accumulation run** and dropped the
    /// `sub sp, sp, #0x840` behind it: the decoder reported 0x70 for a frame
    /// whose body SP is 0x18B0 below the frame pointer, and the walker
    /// enumerated — and the collector wrote through — addresses 6208 bytes
    /// off. That is CLAUDE.md's fourth gate-failure mode: a live walker
    /// visiting the wrong stack.
    #[test]
    fn a_shifted_sub_does_not_end_the_accumulation_run() {
        assert_eq!(
            decode(&[
                ADD_X29_SP_0X70,
                SUB_SP_SP_1_LSL12,
                SUB_SP_SP_0X840,
                NOP,
                RET
            ]),
            Some(0x70 + 0x1000 + 0x840),
            "every `sub sp` in the contiguous prologue run must be folded in"
        );
    }

    /// The `sh` bit is decoded on the `add` that establishes the frame
    /// pointer too — the same masking bug applied there, where it would have
    /// made the decoder skip the fp setup entirely and report a later
    /// instruction's offset (or `None`).
    #[test]
    fn a_shifted_fp_setup_is_decoded() {
        assert_eq!(
            decode(&[ADD_X29_SP_2_LSL12, NOP, RET]),
            Some(0x2000),
            "`add x29, sp, #0x2, lsl #12` establishes fp 8192 above sp"
        );
    }
}
