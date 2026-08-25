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

    fn push_slots(stream: &mut Vec<u8>, slots: &[(u16, i32)]) {
        let mut last: Option<i32> = None;
        for (reg, offset) in slots {
            let tag = match *reg {
                DWARF_REG_FP_AARCH64 => 0u64,
                DWARF_REG_SP_AARCH64 => 1,
                _ => 2,
            };
            let delta = match last {
                None => *offset,
                Some(previous) => offset.wrapping_sub(previous),
            };
            push_varint(stream, (zigzag(delta) << 2) | tag);
            if tag == 2 {
                push_varint(stream, u64::from(*reg));
            }
            last = Some(*offset);
        }
    }

    /// Build one compact blob, mirroring `perry-codegen/src/gc_map.rs` (v4).
    /// `records` is `(instruction_offset, roots, derived, repeat)` — roots as
    /// `(dwarf_reg, offset)`, derived as `(base_index, dwarf_reg, offset)`;
    /// empty lists with `repeat` set encode the repeat flag.
    #[allow(clippy::type_complexity)]
    fn one_map_with_derived(
        function: u64,
        records: &[(u32, Vec<(u16, i32)>, Vec<(u32, u16, i32)>, bool)],
    ) -> Vec<u8> {
        let mut offsets = Vec::new();
        let mut stream = Vec::new();
        for (instruction_offset, roots, derived, repeat) in records {
            offsets.extend_from_slice(&instruction_offset.to_le_bytes());
            if *repeat {
                push_varint(&mut stream, 1);
                continue;
            }
            let has_derived = u64::from(!derived.is_empty());
            push_varint(
                &mut stream,
                ((roots.len() as u64) << 2) | (has_derived << 1),
            );
            push_slots(&mut stream, roots);
            if !derived.is_empty() {
                push_varint(&mut stream, derived.len() as u64);
                for &(base_index, _, _) in derived {
                    push_varint(&mut stream, u64::from(base_index));
                }
                let slots: Vec<(u16, i32)> =
                    derived.iter().map(|&(_, reg, off)| (reg, off)).collect();
                push_slots(&mut stream, &slots);
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

    /// The v3-shaped builder every existing test uses: roots only.
    fn one_map(function: u64, records: &[(u32, Vec<(u16, i32)>, bool)]) -> Vec<u8> {
        let with_derived: Vec<(u32, Vec<(u16, i32)>, Vec<(u32, u16, i32)>, bool)> = records
            .iter()
            .map(|(off, roots, repeat)| (*off, roots.clone(), Vec::new(), *repeat))
            .collect();
        one_map_with_derived(function, &with_derived)
    }

    fn simple(function: u64, offset: u32, frame_offset: i32) -> Vec<u8> {
        one_map(function, &[(offset, vec![(29, frame_offset)], false)])
    }

    #[test]
    fn decodes_frame_location() {
        let bytes = simple(0x1000, 0x10, -8);
        let (records, roots, _) = parse_gc_map(&bytes).expect("valid map");
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
        let (records, _, _) = parse_gc_map(&bytes).expect("concatenated maps");
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].pc, 0x1010);
        assert_eq!(records[1].pc, 0x2020);
    }

    #[test]
    fn merges_gc_maps_from_separate_loaded_images() {
        let first = simple(0x1000, 0x10, -8);
        let second = simple(0x2000, 0x20, -16);
        let mut records = Vec::new();
        let mut roots = Vec::new();
        let mut derived = Vec::new();
        append_gc_map_section(&mut records, &mut roots, &mut derived, &first)
            .expect("first image map");
        append_gc_map_section(&mut records, &mut roots, &mut derived, &second)
            .expect("second image map");

        assert_eq!(records.len(), 2);
        assert_eq!(roots.len(), 2);
        assert_eq!(records[0].roots_start, 0);
        assert_eq!(records[1].roots_start, 1);
        assert_eq!(roots[records[0].roots_start as usize].offset, -8);
        assert_eq!(roots[records[1].roots_start as usize].offset, -16);
    }

    #[test]
    fn an_older_initializer_finishing_last_cannot_replace_a_newer_snapshot() {
        use std::sync::{mpsc, Arc};

        fn index_for(function: u64) -> StackMapIndex {
            let mut records = Vec::new();
            let mut roots = Vec::new();
            let mut derived = Vec::new();
            append_gc_map_section(
                &mut records,
                &mut roots,
                &mut derived,
                &simple(function, 0x20, -8),
            )
            .expect("valid test map");
            records.sort_unstable_by_key(|record| record.pc);
            index_records(records, roots, derived)
        }

        fn force_reversed_publication(store: Arc<StackMapIndexStore>, expected_generation: u64) {
            let (older_snapshotted, wait_for_older) = mpsc::channel();
            let (release_older, older_may_finish) = mpsc::channel();
            let older_store = Arc::clone(&store);
            let older = std::thread::spawn(move || {
                older_store.rebuild_with(|| {
                    let stale = index_for(0x1000);
                    older_snapshotted.send(()).expect("announce older snapshot");
                    older_may_finish.recv().expect("release older snapshot");
                    stale
                });
            });

            wait_for_older
                .recv()
                .expect("older initializer took its snapshot");
            let newer_store = Arc::clone(&store);
            let newer = std::thread::spawn(move || {
                newer_store.rebuild_with(|| index_for(0x2000));
            });
            newer.join().expect("newer initializer completed");
            release_older.send(()).expect("resume older initializer");
            older.join().expect("older initializer completed last");

            let published = store.read();
            assert_eq!(published.generation, expected_generation);
            assert_eq!(published.index.records.len(), 1);
            assert_eq!(published.index.records[0].pc, 0x2020);
        }

        // Cover both races from the review: the newer initializer wins the
        // OnceLock installation while the older one is stalled, and two
        // replacements finish in reverse order after an index already exists.
        force_reversed_publication(Arc::new(StackMapIndexStore::new()), 2);
        let seeded = Arc::new(StackMapIndexStore::new());
        seeded.rebuild_with(|| index_for(0x0800));
        force_reversed_publication(seeded, 3);
    }

    #[test]
    fn reading_an_uninitialized_store_does_not_inspect_loaded_images() {
        let store = StackMapIndexStore::new();
        let published = store.read();

        assert_eq!(published.generation, 0);
        assert!(published.index.records.is_empty());
        assert_eq!(
            store
                .next_generation
                .load(std::sync::atomic::Ordering::Relaxed),
            0,
            "root scanning must not start a loader snapshot"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn discovers_a_map_from_a_later_loaded_shared_object() {
        use std::ffi::CString;
        use std::fmt::Write as _;
        use std::os::unix::ffi::OsStrExt;
        use std::process::Command;

        struct TempDir(std::path::PathBuf);
        impl Drop for TempDir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        let map = simple(0x8075_0000, 0x20, -8);
        let unique = format!(
            "perry-stack-map-dylib-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        );
        let temp = TempDir(std::env::temp_dir().join(unique));
        std::fs::create_dir(&temp.0).expect("create temporary dylib directory");
        let source = temp.0.join("map.c");
        let library = temp.0.join("libmap.so");
        let mut bytes = String::new();
        for (index, byte) in map.iter().enumerate() {
            if index != 0 {
                bytes.push(',');
            }
            write!(bytes, "0x{byte:02x}").expect("format map byte");
        }
        std::fs::write(
            &source,
            format!(
                "__attribute__((used, section(\".perry_gcmap\")))\n\
                 const unsigned char perry_test_map[] = {{{bytes}}};\n\
                 int perry_test_anchor(void) {{ return 8075; }}\n"
            ),
        )
        .expect("write dylib source");
        let compiler = std::env::var_os("CC").unwrap_or_else(|| "cc".into());
        let output = Command::new(compiler)
            .args(["-shared", "-fPIC", "-o"])
            .arg(&library)
            .arg(&source)
            .output()
            .expect("run C compiler");
        assert!(
            output.status.success(),
            "C compiler failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );

        let path = CString::new(library.as_os_str().as_bytes()).expect("NUL-free dylib path");
        let handle = unsafe { libc::dlopen(path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        assert!(!handle.is_null(), "dlopen failed");

        // The old Linux loader inspected /proc/self/exe alone, so this exact
        // map was invisible even though the generated frame was live in the
        // process. Discovering and decoding it pins both the dl_iterate_phdr
        // image walk and the per-image load-bias calculation.
        let sections = loaded_stack_map_sections().expect("inspect every loaded image");
        assert!(
            sections.iter().any(|section| section.starts_with(&map)),
            "the later-loaded shared object's GC map was not discovered"
        );
        let index = build_stack_map_index();
        assert!(
            index.records.iter().any(|record| record.pc == 0x8075_0020),
            "the later-loaded shared object's GC map was not indexed"
        );
        drop(index);
        drop(sections);
        unsafe { libc::dlclose(handle) };
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn rejects_an_unreadable_loaded_shared_object() {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        use std::process::Command;

        struct TempDir(std::path::PathBuf);
        impl Drop for TempDir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        let unique = format!(
            "perry-unreadable-dylib-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        );
        let temp = TempDir(std::env::temp_dir().join(unique));
        std::fs::create_dir(&temp.0).expect("create temporary dylib directory");
        let source = temp.0.join("unreadable.c");
        let library = temp.0.join("libunreadable.so");
        std::fs::write(
            &source,
            "int perry_unreadable_anchor(void) { return 8075; }\n",
        )
        .expect("write dylib source");
        let compiler = std::env::var_os("CC").unwrap_or_else(|| "cc".into());
        let output = Command::new(compiler)
            .args(["-shared", "-fPIC", "-o"])
            .arg(&library)
            .arg(&source)
            .output()
            .expect("run C compiler");
        assert!(
            output.status.success(),
            "C compiler failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );

        let path = CString::new(library.as_os_str().as_bytes()).expect("NUL-free dylib path");
        let handle = unsafe { libc::dlopen(path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        assert!(!handle.is_null(), "dlopen failed");
        std::fs::remove_file(&library).expect("unlink loaded dylib");

        let error = loaded_stack_map_sections().expect_err("unreadable image must fail closed");
        assert!(
            error.contains("libunreadable.so"),
            "diagnostic did not identify the unreadable image: {error}"
        );

        unsafe { libc::dlclose(handle) };
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
        let (records, roots, _) = parse_gc_map(&bytes).expect("valid map");
        assert_eq!(records.len(), 3);
        assert_eq!(roots.len(), 2, "the repeats must not append new roots");
        for record in &records {
            assert_eq!(record.roots_start, 0);
            assert_eq!(record.roots_len, 2);
        }
    }

    #[test]
    fn decodes_derived_interior_slots_with_their_bases() {
        // #7803: a for-of element cursor is a DERIVED pointer — the v3 format
        // collapsed the (base, derived) pair, so the walker chased the
        // interior address as an object start and never rewrote the cursor
        // as base'+delta after a move. v4 keeps the pairing; the repeat flag
        // must carry it too.
        let bytes = one_map_with_derived(
            0x1000,
            &[
                (0x10, vec![(29, -16), (29, -8)], vec![(1, 31, 24)], false),
                (0x20, vec![], vec![], true),
            ],
        );
        let (records, roots, derived) = parse_gc_map(&bytes).expect("valid map");
        assert_eq!(records.len(), 2);
        assert_eq!(roots.len(), 2);
        assert_eq!(
            derived,
            vec![StackMapDerived {
                base_index: 1,
                slot: StackMapLocation {
                    dwarf_reg: 31,
                    offset: 24,
                },
            }]
        );
        for record in &records {
            assert_eq!(record.derived_start, 0);
            assert_eq!(
                record.derived_len, 1,
                "the repeat must carry the derived set"
            );
        }
    }

    #[test]
    fn rejects_a_derived_base_index_out_of_range() {
        // Fail closed, like every other malformed map: a base index past the
        // record's roots would make the walker read a base word from another
        // record's slot.
        let bytes =
            one_map_with_derived(0x1000, &[(0x10, vec![(29, -8)], vec![(1, 31, 24)], false)]);
        assert!(
            parse_gc_map(&bytes).is_none(),
            "base index 1 of 1 roots must not decode"
        );
    }

    #[test]
    fn decodes_negative_and_ascending_root_offsets() {
        let bytes = one_map(0x1000, &[(0, vec![(29, -64), (29, -8), (31, 24)], false)]);
        let (_, roots, _) = parse_gc_map(&bytes).expect("valid map");
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
        let (_, roots, _) = parse_gc_map(&bytes).expect("valid map");
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
    fn an_x19_base_keeps_the_fast_walk_available() {
        // #8770: x19 is the base pointer LLVM takes for a dynamic-allocation
        // frame, captured as `mov x19, sp` after the fixed prologue — so it
        // equals the body SP the x29-chain walker already reconstructs. It is
        // chain-walkable (confirmed per frame at walk time by `x19_is_body_sp`),
        // not a reason to force the whole image onto the platform unwinder.
        let index = index_records(
            vec![StackMapRecord {
                pc: 0x1000,
                function_address: 0x1000,
                stack_size: 64,
                roots_start: 0,
                roots_len: 1,
                derived_start: 0,
                derived_len: 0,
            }],
            vec![StackMapLocation {
                dwarf_reg: 19,
                offset: -40,
            }],
            Vec::new(),
        );
        assert!(index.chain_walkable);
    }

    #[test]
    fn an_unsupported_base_register_disables_the_fast_walk() {
        // The x29-chain walker recovers FP, SP, and x19 (== body SP); any OTHER
        // base register it cannot derive from the frame, so such a record must
        // fall back to the platform unwinder, which reads the frame's CFI.
        let index = index_records(
            vec![StackMapRecord {
                pc: 0x1000,
                function_address: 0x1000,
                stack_size: 64,
                roots_start: 0,
                roots_len: 1,
                derived_start: 0,
                derived_len: 0,
            }],
            vec![StackMapLocation {
                dwarf_reg: 5,
                offset: -40,
            }],
            Vec::new(),
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
            derived_start: 0,
            derived_len: 0,
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
            Vec::new(),
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
                Vec::new()
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
                    derived_start: 0,
                    derived_len: 0,
                },
                StackMapRecord {
                    pc: 0x2040,
                    function_address: 0x2000,
                    stack_size: 32,
                    roots_start: 0,
                    roots_len: 1,
                    derived_start: 0,
                    derived_len: 0,
                },
            ],
            vec![StackMapLocation {
                dwarf_reg: 29,
                offset: -8,
            }],
            Vec::new(),
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
            derived_start: 0,
            derived_len: 0,
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

/// #7984: the prologue shape LLVM emits when SVE is on.
///
/// Every word here was read out of a real aarch64-Linux binary with
/// `objdump -d` — `benchmarks/gc_ratchet/probes/01_nursery_churn.ts` built by
/// this compiler with `PERRY_TARGET_CPU=neoverse-n2`, function `main`, which is
/// the module body and carries 100 stack-map records. The same probe built
/// `-mcpu=neoverse-n1` produces neither the interleaved stores nor the `addvl`,
/// which is why this was an ARM-Linux-runner-only failure.
#[cfg(all(test, target_arch = "aarch64"))]
mod sve_prologue_tests {
    use super::super::fp_to_sp_offset;

    fn decode(words: &[u32]) -> Option<usize> {
        let buf = words.to_vec().into_boxed_slice();
        let out = fp_to_sp_offset(buf.as_ptr() as usize);
        drop(buf);
        out
    }

    /// What `addvl sp, sp, #-2` costs on THIS host, and `None` where the
    /// vector length cannot be read — which is every core without SVE,
    /// including all Apple ones. Deriving the expectation instead of writing a
    /// constant is what makes these tests pin the SCALING rather than one
    /// machine's answer.
    fn two_vector_lengths() -> Option<usize> {
        super::super::sve_vector_length_bytes().map(|vl| 2 * vl)
    }

    //     124790: str   d10, [sp, #-128]!
    //     124794: stp   d9, d8, [sp, #16]
    //     124798: stp   x29, x30, [sp, #32]
    //     12479c: add   x29, sp, #0x20
    //     1247a0: stp   x28, x27, [sp, #48]
    //     1247a4: stp   x26, x25, [sp, #64]
    //     1247a8: stp   x24, x23, [sp, #80]
    //     1247ac: stp   x22, x21, [sp, #96]
    //     1247b0: stp   x20, x19, [sp, #112]
    //     1247b4: sub   sp, sp, #0x50
    //     1247b8: addvl sp, sp, #-2
    //     1247bc: bl    js_inline_arena_state
    const STR_D10_SP_M128_PRE: u32 = 0xFC18_0FEA;
    const STP_D9_D8_SP_16: u32 = 0x6D01_23E9;
    const STP_X29_X30_SP_32: u32 = 0xA902_7BFD;
    const ADD_X29_SP_0X20: u32 = 0x9100_83FD;
    const STP_X28_X27_SP_48: u32 = 0xA903_6FFC;
    const STP_X26_X25_SP_64: u32 = 0xA904_67FA;
    const STP_X24_X23_SP_80: u32 = 0xA905_5FF8;
    const STP_X22_X21_SP_96: u32 = 0xA906_57F6;
    const STP_X20_X19_SP_112: u32 = 0xA907_4FF4;
    const SUB_SP_SP_0X50: u32 = 0xD101_43FF;
    const ADDVL_SP_SP_M2: u32 = 0x043F_57DF;
    const BL: u32 = 0x9418_FFE3;

    /// A callee-save store does not move sp, so it cannot end the prologue's
    /// run of stack adjustments.
    ///
    /// Before #7984 the first `stp` after the frame-pointer setup ended the
    /// run, so this frame decoded as 0x20 when its body SP is 0x50 further
    /// down — placing every SP-relative root in it 80 bytes too high, silently,
    /// on the walker that runs when `verify` is off.
    #[test]
    fn callee_save_stores_do_not_end_the_stack_adjustment_run() {
        assert_eq!(
            decode(&[
                STR_D10_SP_M128_PRE,
                STP_D9_D8_SP_16,
                STP_X29_X30_SP_32,
                ADD_X29_SP_0X20,
                STP_X28_X27_SP_48,
                STP_X26_X25_SP_64,
                STP_X24_X23_SP_80,
                STP_X22_X21_SP_96,
                STP_X20_X19_SP_112,
                SUB_SP_SP_0X50,
                BL,
            ]),
            Some(0x20 + 0x50),
            "the `sub sp, sp, #0x50` behind five callee-save pairs must still \
             be folded into the frame base"
        );
    }

    /// An SVE stack adjustment is in units of the runtime vector length, so
    /// there is no correct byte count to return. Fail closed and let the
    /// platform unwinder — which reads DWARF CFI, and needs no VG for an
    /// fp-based frame — answer for this frame.
    ///
    /// Returning the un-scaled value instead is #7984: `main` decoded as 0x20
    /// against a real `x29 - body_sp` of 0x90, and `PERRY_STACKMAP_WALKER=verify`
    /// caught the fp-chain walker and the unwinder 96 bytes apart on
    /// `ubuntu-24.04-arm`.
    #[test]
    fn an_sve_stack_adjustment_is_scaled_by_the_vector_length_or_fails_closed() {
        assert_eq!(
            decode(&[
                ADD_X29_SP_0X20,
                STP_X28_X27_SP_48,
                SUB_SP_SP_0X50,
                ADDVL_SP_SP_M2,
                BL,
            ]),
            two_vector_lengths().map(|bytes| 0x20 + 0x50 + bytes),
            "`addvl sp, sp, #-2` allocates two vector lengths; where the \
             length cannot be read the whole decode must fail so the frame \
             goes to the unwinder, never report the 0x70 it could read"
        );
    }

    /// The multiplier is read from the instruction and the unit from the
    /// kernel, so the two must be pinned separately — a decoder that ignored
    /// `imm6` would still pass the test above on a host with VL = 16 and one
    /// `addvl`.
    #[test]
    fn the_sve_multiplier_comes_from_the_instruction() {
        let Some(vl) = super::super::sve_vector_length_bytes() else {
            return; // no SVE on this host; the fail-closed arm covers it
        };
        assert_eq!(
            super::super::sve_sp_allocation_bytes(ADDVL_SP_SP_M2),
            Some(2 * vl)
        );
        // `addvl sp, sp, #-1`: imm6 = -1 in bits [10:5].
        let addvl_m1 = (ADDVL_SP_SP_M2 & !(0x3F << 5)) | (0x3F << 5);
        assert_eq!(super::super::sve_sp_allocation_bytes(addvl_m1), Some(vl));
        // A POSITIVE multiplier is a deallocation, not a prologue allocation.
        let addvl_p2 = (ADDVL_SP_SP_M2 & !(0x3F << 5)) | (2 << 5);
        assert_eq!(super::super::sve_sp_allocation_bytes(addvl_p2), None);
    }

    /// The whole measured prologue, verbatim: the two defects compose, and the
    /// answer is still `None` rather than a partially-correct number.
    #[test]
    fn the_measured_neoverse_n2_prologue_decodes_or_fails_closed() {
        assert_eq!(
            decode(&[
                STR_D10_SP_M128_PRE,
                STP_D9_D8_SP_16,
                STP_X29_X30_SP_32,
                ADD_X29_SP_0X20,
                STP_X28_X27_SP_48,
                STP_X26_X25_SP_64,
                STP_X24_X23_SP_80,
                STP_X22_X21_SP_96,
                STP_X20_X19_SP_112,
                SUB_SP_SP_0X50,
                ADDVL_SP_SP_M2,
                BL,
            ]),
            two_vector_lengths().map(|bytes| 0x20 + 0x50 + bytes),
            "the whole measured prologue: 0x20 from the `add`, 0x50 from the \
             `sub` behind five callee-save pairs, and two vector lengths"
        );
    }

    /// `addvl` into a scratch register is not a stack adjustment and must not
    /// disable the frame. LLVM emits `addvl x8, sp, #2` all over an SVE
    /// function body to address spill slots; only a write to SP moves the
    /// frame. (`043f57df` is `addvl sp,…`; clearing the destination field to
    /// x8 gives the body form.)
    #[test]
    fn addvl_into_a_scratch_register_is_not_a_stack_adjustment() {
        let addvl_x8 = (ADDVL_SP_SP_M2 & !0x1F) | 8;
        assert_eq!(
            decode(&[ADD_X29_SP_0X20, SUB_SP_SP_0X50, addvl_x8, BL]),
            Some(0x20 + 0x50),
            "only `addvl` writing SP is undecodable; one writing x8 is a body \
             address computation"
        );
    }
}
