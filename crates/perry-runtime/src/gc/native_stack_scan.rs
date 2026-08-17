//! Diagnostic native-stack scan for stale from-space pointers (#8220 class).
//!
//! After a copying minor rewrites all known roots and moves survivors, but
//! BEFORE `copying_reset_from_spaces_and_flip` recycles from-space, this pass
//! walks the native (Rust) stack looking for words that decode to a from-space
//! address whose target carries `GC_FLAG_FORWARDED`. Such a word is an
//! unambiguous stale pointer: the object moved this cycle and this stack slot
//! was not updated — exactly the #8220 class (a raw pointer held in a native
//! Rust frame across a copying minor, invisible to the precise root map).
//!
//! Gated behind `PERRY_GC_SCAN_NATIVE_STACK=1`. Non-fatal by default;
//! `PERRY_GC_SCAN_NATIVE_STACK_ABORT=1` aborts after the first offender so a
//! debugger catches the cycle.

use crate::arena::classify_heap_space_in_range;
use crate::gc::types::{forwarding_address, GC_FLAG_FORWARDED, GC_HEADER_SIZE};

/// Is the native-stack diagnostic enabled?
///
/// **ABORT IMPLIES SCAN** — the same rule `fromspace_scan::resolve_scan_knobs`
/// carries for #7154's pair. Without it `PERRY_GC_SCAN_NATIVE_STACK_ABORT=1`
/// alone is completely inert: `run_native_stack_scan` returns at this gate, the
/// scan never runs, there is nothing to abort, and the run reports success. A
/// knob that reads as "abort on the first offender" and silently does nothing
/// is exactly what the GC knob kill-policy exists to prevent, and exactly what
/// an investigator reaching for the abort switch mid-hunt would be misled by.
fn native_stack_scan_enabled() -> bool {
    crate::gc::env_flag_enabled("PERRY_GC_SCAN_NATIVE_STACK") || native_stack_scan_abort()
}

/// Should we abort on the first stale pointer found?
fn native_stack_scan_abort() -> bool {
    crate::gc::env_flag_enabled("PERRY_GC_SCAN_NATIVE_STACK_ABORT")
}

/// Run the diagnostic native-stack scan. Call this after the rewrite pass and
/// before from-space reset, inside the copying minor.
pub(super) fn run_native_stack_scan() {
    if !native_stack_scan_enabled() {
        return;
    }

    // Capture the backtrace so we can attribute stack slots to frames.
    let bt = std::backtrace::Backtrace::force_capture();

    // Walk the frame pointer chain to get frame boundaries. On arm64/x86_64
    // macOS, FP (x29 / rbp) points to [prev_fp, return_addr]. We walk
    // upward and record (sp, return_addr_symbol) for each frame so we can
    // match stale pointer stack addresses to the frame that holds them.
    let frames = walk_frame_pointers();

    // Get the stack bounds so we don't read past the valid stack.
    // On macOS, pthread_get_stackaddr_np gives the TOP of the stack
    // (highest address), and pthread_get_stacksize_np gives the total size.
    // The stack grows downward, so valid range is [top - size, top].
    #[cfg(target_os = "macos")]
    let (_stack_lo, stack_hi) = {
        let top = unsafe { libc::pthread_get_stackaddr_np(libc::pthread_self()) } as usize;
        let size = unsafe { libc::pthread_get_stacksize_np(libc::pthread_self()) } as usize;
        (top - size, top)
    };
    #[cfg(not(target_os = "macos"))]
    let (stack_lo, stack_hi) = {
        // Fallback: use a local variable address and scan 256KB upward.
        let marker: usize = 0;
        let sp = std::ptr::addr_of!(marker) as usize;
        (sp, sp + 256 * 1024)
    };

    // Get a reference point on the stack. On x86_64/aarch64 the stack grows
    // downward, so we scan from this address upward (toward higher addresses).
    let stack_marker: usize = 0;
    let scan_start = std::ptr::addr_of!(stack_marker) as usize;
    // Align upward to 8 bytes.
    let scan_start = (scan_start + 7) & !7;
    // Don't scan past the stack top.
    let scan_end = stack_hi;

    let mut offenders: Vec<StaleStackSlot> = Vec::new();
    let mut words_scanned: usize = 0;

    let mut addr = scan_start;
    while addr + 8 <= scan_end {
        words_scanned += 1;
        // SAFETY: we are scanning the stack above our own frame. These pages
        // are all mapped (they are the active stack). We only READ.
        let word = unsafe { *(addr as *const u64) };

        // Try decoding as a raw address (plain *const T).
        if let Some(slot) = check_word_as_heap_pointer(word, addr) {
            offenders.push(slot);
        }

        // Try decoding as a NaN-boxed pointer. Perry uses several tags:
        // POINTER_TAG (0x7FFD) for objects/arrays, STRING_TAG (0x7FFF) for
        // heap strings, BIGINT_TAG for bigints. All encode the pointer in the
        // lower 48 bits.
        let nanboxed_addr = word & 0x0000_FFFF_FFFF_FFFF;
        let tag = word & 0xFFFF_0000_0000_0000;
        if tag == 0x7FFD_0000_0000_0000      // POINTER_TAG
            || tag == 0x7FFF_0000_0000_0000
        // STRING_TAG
        {
            if let Some(slot) = check_word_as_heap_pointer(nanboxed_addr, addr) {
                offenders.push(slot);
            }
        }

        addr += 8;
    }

    eprintln!(
        "[native-stack-scan] scanned {words_scanned} words on stack [{scan_start:#x}..{scan_end:#x}], found {n} stale pointer(s)",
        words_scanned = words_scanned,
        scan_start = scan_start,
        scan_end = scan_end,
        n = offenders.len(),
    );

    // Dump ALL words in the generated code's frame (the last few frames
    // in the walk) to see what's actually on the stack.
    if !frames.is_empty() {
        // The generated code's frame is above js_gc_loop_safepoint_armed's
        // frame. Find it by looking for the frame that returns to
        // src_lib_util_graceful_exit_ts__init_body.
        for (i, frame) in frames.iter().enumerate() {
            let sym = resolve_symbol(frame.return_addr);
            if sym.contains("graceful_exit") || sym.contains("init_body") {
                let dump_start = frame.sp;
                let dump_end = frame.sp + frame.size;
                eprintln!(
                    "[native-stack-scan] generated code frame {i}: sp=0x{sp:x}..0x{end:x} ({size} bytes)",
                    i = i,
                    sp = frame.sp,
                    end = dump_end,
                    size = frame.size,
                );
                let mut off = 0usize;
                let mut a = dump_start;
                while a + 8 <= dump_end && off < 64 {
                    let w = unsafe { *(a as *const u64) };
                    if w != 0 {
                        eprintln!("  sp+{off:#04x}: 0x{w:016x}", off = (a - dump_start), w = w,);
                    }
                    a += 8;
                    off += 1;
                }
            }
        }
    }

    if offenders.is_empty() {
        return;
    }

    eprintln!("[native-stack-scan] backtrace at scan point:\n{}", bt);

    // Print frame boundaries so we can match stale pointers to frames.
    for (i, frame) in frames.iter().enumerate() {
        // Resolve the symbol for the return address using dladdr.
        let sym = resolve_symbol(frame.return_addr);
        eprintln!(
            "[native-stack-scan] frame {i}: sp=0x{sp:x}..0x{sp_end:x} ret={ret:#x} sym={sym}",
            i = i,
            sp = frame.sp,
            sp_end = frame.sp + frame.size,
            ret = frame.return_addr,
            sym = sym,
        );
    }

    for (i, slot) in offenders.iter().enumerate() {
        // Find which frame this stale pointer belongs to.
        let frame_idx = frames
            .iter()
            .position(|f| slot.stack_addr >= f.sp && slot.stack_addr < f.sp + f.size);
        // Read the raw word for debugging.
        let raw_word = unsafe { *(slot.stack_addr as *const u64) };
        eprintln!(
            "[native-stack-scan] #{i}: stack_addr=0x{stack_addr:x} (offset +{offset:#x} from scan_start) \
             raw_word=0x{raw_word:016x} from_space_addr=0x{from_space:x} -> forwarded_to=0x{new_addr:x} \
             target_obj_type={obj_type} target_space={space:?} nanboxed={nanboxed} \
             frame_idx={frame_idx:?}",
            i = i,
            stack_addr = slot.stack_addr,
            offset = slot.stack_addr - scan_start,
            raw_word = raw_word,
            from_space = slot.from_space_addr,
            new_addr = slot.forwarded_to,
            obj_type = slot.target_obj_type,
            space = slot.target_space,
            nanboxed = slot.nanboxed,
            frame_idx = frame_idx,
        );
    }

    if native_stack_scan_abort() {
        std::process::abort();
    }
}

/// One frame in the native call stack, from the frame pointer chain.
#[derive(Clone, Copy)]
struct FrameInfo {
    /// Stack pointer (lowest address) of this frame.
    sp: usize,
    /// Size of this frame in bytes (distance to the next frame's SP).
    size: usize,
    /// Return address (the instruction after the call in the caller).
    return_addr: usize,
}

/// Walk the arm64 frame pointer chain to collect frame boundaries.
///
/// On arm64, the frame pointer (FP / x29) points to a saved [prev_fp, lr]
/// pair on the stack. Starting from the current FP, we walk upward:
///
/// ```text
///   [current FP] -> [prev_fp] [return_addr]
///                    |
///                    v
///                    [prev_prev_fp] [return_addr_2]
///                    ...
/// ```
///
/// Each frame's SP is the address of its saved FP slot, and its size is
/// the distance to the previous frame's FP slot.
#[cfg(target_arch = "aarch64")]
fn walk_frame_pointers() -> Vec<FrameInfo> {
    let mut frames = Vec::new();
    let mut fp: usize;

    // Get the current frame pointer.
    #[cfg(target_os = "macos")]
    unsafe {
        std::arch::asm!(
            "mov {fp}, x29",
            fp = out(reg) fp,
        );
    }
    #[cfg(not(target_os = "macos"))]
    {
        let marker: usize = 0;
        fp = std::ptr::addr_of!(marker) as usize;
    }

    let return_addr: usize;
    let mut depth = 0;

    while fp != 0 && depth < 64 {
        // On arm64, [fp] = prev_fp, [fp+8] = return_addr (LR).
        let saved_fp = unsafe { *(fp as *const usize) };
        let saved_lr = unsafe { *((fp + 8) as *const usize) };

        if saved_fp == 0 || saved_fp <= fp {
            // End of chain or invalid (fp should increase as we go up).
            return_addr = saved_lr;
            frames.push(FrameInfo {
                sp: fp,
                size: 4096, // unknown, give a generous bound
                return_addr,
            });
            break;
        }

        let frame_size = saved_fp - fp;
        frames.push(FrameInfo {
            sp: fp,
            size: frame_size,
            return_addr: saved_lr,
        });
        fp = saved_fp;
        depth += 1;
    }

    frames
}

#[cfg(not(target_arch = "aarch64"))]
fn walk_frame_pointers() -> Vec<FrameInfo> {
    Vec::new()
}

/// Resolve a code address to a symbol name using `dladdr`.
fn resolve_symbol(addr: usize) -> String {
    unsafe {
        let mut info: libc::Dl_info = std::mem::zeroed();
        if libc::dladdr(addr as *const libc::c_void, &mut info) != 0 {
            let sym = if info.dli_sname.is_null() {
                "?".to_string()
            } else {
                std::ffi::CStr::from_ptr(info.dli_sname)
                    .to_string_lossy()
                    .into_owned()
            };
            let fname = if info.dli_fname.is_null() {
                "?".to_string()
            } else {
                std::ffi::CStr::from_ptr(info.dli_fname)
                    .to_string_lossy()
                    .into_owned()
            };
            let offset = addr - (info.dli_saddr as usize);
            format!("{sym}+{offset:#x} in {fname}")
        } else {
            format!("0x{addr:x} (dladdr failed)")
        }
    }
}

#[derive(Clone, Copy)]
struct StaleStackSlot {
    stack_addr: usize,
    from_space_addr: usize,
    forwarded_to: usize,
    target_obj_type: u8,
    target_space: crate::arena::HeapSpace,
    nanboxed: bool,
}

/// Check if `word` is a heap pointer to a forwarded from-space object.
/// Returns `Some(StaleStackSlot)` if it is, `None` otherwise.
fn check_word_as_heap_pointer(word: u64, stack_addr: usize) -> Option<StaleStackSlot> {
    if word == 0 {
        return None;
    }
    let addr = word as usize;

    // Check if this address is in a nursery space (from-space).
    // #8277 widened this to (space, base, object_start_bitmap); the diagnostic
    // only needs the space and the block base.
    let (space, base, _starts) = classify_heap_space_in_range(addr)?;
    if !space.is_nursery() {
        return None;
    }

    // The word could be a user pointer (payload start) or a header pointer.
    // Try both: first as a user pointer (header is at addr - GC_HEADER_SIZE),
    // then as a header pointer itself.
    let header_addr = if addr >= base + GC_HEADER_SIZE {
        addr - GC_HEADER_SIZE
    } else {
        addr
    };

    // SAFETY: the address is classified as nursery/from-space, which means
    // it's in a registered arena block. The from-space is still intact (we
    // run before the reset), so the header is valid.
    let header = header_addr as *const crate::gc::types::GcHeader;
    let flags = unsafe { (*header).gc_flags };
    if flags & GC_FLAG_FORWARDED == 0 {
        return None;
    }

    // This is a stale pointer! The object was forwarded (moved) but this
    // stack slot still points to the old location.
    let forwarded_to = unsafe { forwarding_address(header) } as usize;
    let obj_type = unsafe { (*header).obj_type };

    Some(StaleStackSlot {
        stack_addr,
        from_space_addr: addr,
        forwarded_to,
        target_obj_type: obj_type,
        target_space: space,
        nanboxed: (word & 0xFFFF_0000_0000_0000) == 0x7FFD_0000_0000_0000,
    })
}
