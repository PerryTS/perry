//! #9486 — real, named frames in `Error.prototype.stack`.
//!
//! Split out of `error.rs` to keep that file under the 2,000-line CI cap
//! (`scripts/check_file_size.sh`); included from there with
//! `#[path = "error_stack_frames.rs"] mod stack_frames;`, so `use super::*`
//! resolves against `error.rs`.
//!
//! # What was broken
//!
//! `current_stack_frame()` produced exactly one line — `    at <anonymous>`
//! outside a `--debug-symbols` build — because nothing ever looked at the
//! native stack. Node prints the real call chain, so every `err.stack` a
//! compiled app rendered (cc's `doctor`, commander's parse-error report) lost
//! its diagnostic content entirely.
//!
//! # The two halves, and why each one is the cheap one
//!
//! **Capture** is a frame-pointer chain walk. Codegen tags every generated
//! function `"frame-pointer"="non-leaf"` (`perry-codegen/src/function.rs`),
//! which is the same property the collector's own `fp_chain` walker relies on
//! (`gc/roots/stack_maps.rs`), so `[fp] = caller fp` / `[fp+8] = return
//! address` holds for JS frames on both supported architectures. Two loads per
//! frame, no allocation, no symbolication — and it runs on EVERY `new Error`,
//! including the overwhelming majority whose `.stack` is never read.
//!
//! **Resolution** happens on first `.stack` read and reuses the registry
//! codegen already fills: `js_register_function_name` records
//! `(compiled address, JS display name)` once per function in
//! `__perry_init_strings_*` (72,713 entries for the claude-code bundle) so
//! `fn.name` and `[Function: f]` work. That table is keyed by exact function
//! address; a return address points into the MIDDLE of a function, so this
//! module snapshots it into an address-sorted vector once and answers
//! containment with a binary search.
//!
//! # Why the frames are named but not positioned
//!
//! A `file:line:col` would need a per-return-address line table — an
//! O(instructions) artifact, against this one's O(functions). The issue's bar
//! is frame COUNT and NAMES; positions are explicitly not byte-compared
//! against node. `    at name` is what a resolved frame renders as.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;

/// Native return addresses captured per construction. 16 words = 128 bytes of
/// encoded blob, enough to cover node's default `Error.stackTraceLimit` of 10
/// JS frames plus the runtime frames between `new Error` and the throwing
/// function.
pub(crate) const MAX_CAPTURED_FRAMES: usize = 16;

/// Rendered JS frames, matching V8's default `Error.stackTraceLimit`.
const RENDER_LIMIT: usize = 10;

/// Encoded characters per captured address: 48 bits at 6 bits per character.
/// Both supported platforms keep user-space text well under 2^47.
const PC_CHARS: usize = 8;
const PC_BITS: u32 = 48;

/// A frame whose containing function starts more than this far below it is not
/// plausibly inside that function: the address belongs to an unregistered
/// function (runtime Rust code, a codegen thunk) that happens to sort after a
/// registered one. Rejecting it is what keeps a native frame from being
/// reported under some unrelated JS function's name.
const MAX_FUNCTION_SPAN: usize = 1 << 20;

const ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+-";

fn decode_char(c: u8) -> Option<u64> {
    let v = match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' => 62,
        b'-' => 63,
        _ => return None,
    };
    Some(v as u64)
}

/// Encode captured addresses into an ASCII blob.
///
/// ASCII rather than the raw little-endian words for one reason: the blob is
/// carried in a `StringHeader` (the only GC cell shape an `ErrorHeader` field
/// and a closure capture slot can both already hold and trace), and a
/// `StringHeader` whose payload is arbitrary bytes is a UTF-8 hazard for every
/// generic string path that might ever touch it. Six bits per character costs
/// 8 bytes per address — the same as the raw word — so the safety is free.
pub(crate) fn encode_pcs(pcs: &[usize], out: &mut [u8; MAX_CAPTURED_FRAMES * PC_CHARS]) -> usize {
    let mut n = 0usize;
    for &pc in pcs.iter().take(MAX_CAPTURED_FRAMES) {
        let v = pc as u64;
        if v >> PC_BITS != 0 {
            continue;
        }
        for i in 0..PC_CHARS {
            let shift = PC_BITS - 6 * (i as u32 + 1);
            out[n + i] = ALPHABET[((v >> shift) & 0x3f) as usize];
        }
        n += PC_CHARS;
    }
    n
}

/// Inverse of [`encode_pcs`]. A blob whose length is not a multiple of
/// [`PC_CHARS`], or that contains a character outside the alphabet, decodes to
/// nothing rather than to garbage addresses.
fn decode_pcs(blob: &[u8]) -> Vec<usize> {
    if blob.is_empty() || blob.len() % PC_CHARS != 0 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(blob.len() / PC_CHARS);
    for chunk in blob.chunks_exact(PC_CHARS) {
        let mut v: u64 = 0;
        for &c in chunk {
            match decode_char(c) {
                Some(bits) => v = (v << 6) | bits,
                None => return Vec::new(),
            }
        }
        out.push(v as usize);
    }
    out
}

// ---------------------------------------------------------------------------
// Capture: the frame-pointer chain walk.
// ---------------------------------------------------------------------------

#[cfg(all(
    any(target_vendor = "apple", target_os = "linux"),
    any(target_arch = "aarch64", target_arch = "x86_64")
))]
mod walk {
    use super::MAX_CAPTURED_FRAMES;

    /// A frame record is two words and must be word-aligned; anything else is
    /// a corrupt chain and abandons the walk, exactly as the collector's
    /// `fp_chain::visit` does.
    const FRAME_RECORD_ALIGN_MASK: usize = 0x7;

    #[cfg(target_arch = "aarch64")]
    #[inline(always)]
    fn current_frame_pointer() -> usize {
        let fp: usize;
        unsafe {
            core::arch::asm!("mov {fp}, x29", fp = out(reg) fp, options(nomem, nostack));
        }
        fp
    }

    #[cfg(target_arch = "x86_64")]
    #[inline(always)]
    fn current_frame_pointer() -> usize {
        let fp: usize;
        unsafe {
            core::arch::asm!("mov {fp}, rbp", fp = out(reg) fp, options(nomem, nostack));
        }
        fp
    }

    #[cfg(target_vendor = "apple")]
    fn stack_top_uncached() -> usize {
        unsafe extern "C" {
            fn pthread_self() -> *mut core::ffi::c_void;
            fn pthread_get_stackaddr_np(thread: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
        }
        unsafe { pthread_get_stackaddr_np(pthread_self()) as usize }
    }

    #[cfg(all(target_os = "linux", not(target_vendor = "apple")))]
    fn stack_top_uncached() -> usize {
        unsafe extern "C" {
            fn pthread_self() -> usize;
            fn pthread_getattr_np(thread: usize, attr: *mut u8) -> i32;
            fn pthread_attr_getstack(
                attr: *const u8,
                stackaddr: *mut *mut core::ffi::c_void,
                stacksize: *mut usize,
            ) -> i32;
            fn pthread_attr_destroy(attr: *mut u8) -> i32;
        }
        let mut attr = [0u8; 128];
        let mut addr: *mut core::ffi::c_void = core::ptr::null_mut();
        let mut size: usize = 0;
        unsafe {
            if pthread_getattr_np(pthread_self(), attr.as_mut_ptr()) != 0 {
                return 0;
            }
            let ok = pthread_attr_getstack(attr.as_ptr(), &mut addr, &mut size) == 0;
            pthread_attr_destroy(attr.as_mut_ptr());
            if !ok {
                return 0;
            }
        }
        (addr as usize).saturating_add(size)
    }

    // The bound is a property of the thread, and `new Error` is frequent
    // enough that two libc calls per construction would be the dominant cost
    // of the capture.
    crate::perry_thread_local! {
        static STACK_TOP: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    fn stack_top() -> usize {
        STACK_TOP.with(|c| {
            let cached = c.get();
            if cached != 0 {
                return cached;
            }
            let top = stack_top_uncached();
            c.set(top);
            top
        })
    }

    /// Collect return addresses from this frame outward, innermost first.
    ///
    /// Fails closed: any misaligned, non-increasing or out-of-bounds frame
    /// pointer ends the walk and keeps whatever was collected before it,
    /// rather than reading a word at a fabricated address.
    pub(super) fn capture(out: &mut [usize; MAX_CAPTURED_FRAMES]) -> usize {
        let top = stack_top();
        if top == 0 {
            return 0;
        }
        let mut n = 0usize;
        let mut fp = current_frame_pointer();
        while n < MAX_CAPTURED_FRAMES && fp != 0 {
            if fp & FRAME_RECORD_ALIGN_MASK != 0 {
                break;
            }
            match fp.checked_add(16) {
                Some(end) if end <= top => {}
                _ => break,
            }
            // SAFETY: `fp` is word-aligned and `fp..fp+16` lies inside this
            // thread's stack, so both words of the frame record are readable.
            let return_address = unsafe { *((fp + 8) as *const usize) };
            let caller_fp = unsafe { *(fp as *const usize) };
            if return_address == 0 {
                break;
            }
            out[n] = return_address;
            n += 1;
            if caller_fp <= fp {
                break;
            }
            fp = caller_fp;
        }
        n
    }
}

#[cfg(not(all(
    any(target_vendor = "apple", target_os = "linux"),
    any(target_arch = "aarch64", target_arch = "x86_64")
)))]
mod walk {
    use super::MAX_CAPTURED_FRAMES;

    /// Windows and the non-frame-pointer targets keep the pre-#9486 behavior
    /// (a single `<anonymous>` frame) rather than guess at a chain shape the
    /// ABI does not guarantee.
    pub(super) fn capture(_out: &mut [usize; MAX_CAPTURED_FRAMES]) -> usize {
        0
    }
}

/// Capture the current native return addresses and encode them.
/// Returns `(buffer, len)`; `len == 0` means nothing was captured.
pub(crate) fn capture_encoded() -> ([u8; MAX_CAPTURED_FRAMES * PC_CHARS], usize) {
    let mut pcs = [0usize; MAX_CAPTURED_FRAMES];
    let n = walk::capture(&mut pcs);
    let mut blob = [0u8; MAX_CAPTURED_FRAMES * PC_CHARS];
    if n == 0 {
        return (blob, 0);
    }
    let len = encode_pcs(&pcs[..n], &mut blob);
    (blob, len)
}

// ---------------------------------------------------------------------------
// Resolution: address -> JS display name.
// ---------------------------------------------------------------------------

struct CodeSymbolIndex {
    /// Registry size the snapshot was taken at. `register_function_name_if_absent`
    /// can add entries after module init (symbol-keyed object literals,
    /// `util.promisify`), so a changed length rebuilds rather than serving a
    /// stale table.
    source_len: usize,
    /// `(function start address, display name)`, sorted by address.
    entries: Vec<(usize, Arc<[u8]>)>,
}

fn index_slot() -> &'static Mutex<Option<CodeSymbolIndex>> {
    static INDEX: OnceLock<Mutex<Option<CodeSymbolIndex>>> = OnceLock::new();
    INDEX.get_or_init(|| Mutex::new(None))
}

/// Resolve `ip` to the display name of the function containing it.
///
/// The registry names function STARTS, and there is no end address to pair
/// with them, so containment is "the greatest start at or below `ip`, provided
/// `ip` is below the next start and within [`MAX_FUNCTION_SPAN`] of this one".
/// The span cap is what stops an address past the last registered function —
/// every runtime Rust frame, on a link layout that places the archives after
/// the generated objects — from being reported under that function's name.
fn name_for_ip(index: &CodeSymbolIndex, ip: usize) -> Option<&Arc<[u8]>> {
    let at = index.entries.partition_point(|(addr, _)| *addr <= ip);
    let at = at.checked_sub(1)?;
    let (start, name) = &index.entries[at];
    if ip < *start || ip - *start > MAX_FUNCTION_SPAN {
        return None;
    }
    if let Some((next, _)) = index.entries.get(at + 1) {
        if ip >= *next {
            return None;
        }
    }
    Some(name)
}

fn with_index<R>(f: impl FnOnce(&CodeSymbolIndex) -> R) -> Option<R> {
    let mut slot = index_slot().lock().ok()?;
    let current_len = crate::builtins::function_name_registry_len()?;
    let stale = match slot.as_ref() {
        Some(index) => index.source_len != current_len,
        None => true,
    };
    if stale {
        let mut entries = crate::builtins::function_name_registry_entries()?;
        entries.sort_unstable_by_key(|(addr, _)| *addr);
        *slot = Some(CodeSymbolIndex {
            source_len: current_len,
            entries,
        });
    }
    slot.as_ref().map(f)
}

/// Render captured frames as `.stack` frame lines, or `None` when nothing in
/// the capture resolved to a JS function.
///
/// Unresolved frames are DROPPED rather than printed as bare addresses. Node
/// elides its own internal frames the same way, and the frames this drops are
/// exactly the runtime's: `js_error_new_with_message`, `alloc_error`, the
/// builtin that invoked a callback. A capture in which nothing resolves
/// returns `None` so the caller can fall back to the pre-#9486 single line
/// instead of producing a headed stack with no frames at all.
pub(crate) fn render_frames(blob: &[u8]) -> Option<String> {
    let pcs = decode_pcs(blob);
    if pcs.is_empty() {
        return None;
    }
    with_index(|index| {
        let mut out = String::new();
        let mut rendered = 0usize;
        for pc in &pcs {
            if rendered >= RENDER_LIMIT {
                break;
            }
            // A return address points AFTER the call instruction; on a tail
            // position that byte can belong to the next function, so resolve
            // the call site itself.
            let Some(name) = name_for_ip(index, pc.saturating_sub(1)) else {
                continue;
            };
            let Ok(name) = std::str::from_utf8(name) else {
                continue;
            };
            if name.is_empty() {
                continue;
            }
            if rendered > 0 {
                out.push('\n');
            }
            out.push_str("    at ");
            out.push_str(name);
            rendered += 1;
        }
        if rendered == 0 {
            None
        } else {
            Some(out)
        }
    })
    .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcs_round_trip_through_the_ascii_blob() {
        let pcs = [0x1_0000_1234usize, 0x7fff_ffff_0000, 1, 0];
        let mut buf = [0u8; MAX_CAPTURED_FRAMES * PC_CHARS];
        let len = encode_pcs(&pcs, &mut buf);
        assert_eq!(len, pcs.len() * PC_CHARS);
        assert!(
            buf[..len].iter().all(|b| b.is_ascii_graphic()),
            "the blob rides in a StringHeader — every byte must be plain ASCII"
        );
        assert_eq!(decode_pcs(&buf[..len]), pcs.to_vec());
    }

    #[test]
    fn a_malformed_blob_decodes_to_nothing_rather_than_to_addresses() {
        assert!(decode_pcs(b"AAA").is_empty(), "short blob");
        assert!(decode_pcs(b"AAAAAAA!").is_empty(), "character outside the alphabet");
        assert!(decode_pcs(b"").is_empty(), "empty blob");
    }

    /// The containment rule is the whole correctness story of the resolver: a
    /// registry entry names a function START, and mis-reading the gap after
    /// the last one is how a runtime frame would acquire a JS function's name.
    #[test]
    fn containment_rejects_addresses_past_a_function() {
        let index = CodeSymbolIndex {
            source_len: 2,
            entries: vec![
                (0x1000, Arc::from(&b"first"[..])),
                (0x2000, Arc::from(&b"second"[..])),
            ],
        };
        assert_eq!(
            name_for_ip(&index, 0x1004).map(|n| n.to_vec()),
            Some(b"first".to_vec())
        );
        assert_eq!(
            name_for_ip(&index, 0x2000).map(|n| n.to_vec()),
            Some(b"second".to_vec())
        );
        assert!(
            name_for_ip(&index, 0x0fff).is_none(),
            "below the first entry belongs to nobody"
        );
        assert!(
            name_for_ip(&index, 0x2000 + MAX_FUNCTION_SPAN + 1).is_none(),
            "past the last entry by more than a function's plausible span is \
             a runtime frame, not `second`"
        );
    }

    /// A capture in which no address resolves must not produce an empty frame
    /// list — the caller has to be able to fall back.
    #[test]
    fn a_capture_with_no_resolvable_frame_renders_nothing() {
        let mut buf = [0u8; MAX_CAPTURED_FRAMES * PC_CHARS];
        // Address 8, which no registry can plausibly contain.
        let len = encode_pcs(&[8usize], &mut buf);
        assert_eq!(render_frames(&buf[..len]), None);
    }

    #[test]
    fn the_walk_sees_more_than_one_frame() {
        // The unit binary is Rust, not generated code, so nothing here
        // RESOLVES — but the chain itself must be walkable, which is the
        // half of the capture this crate can test on its own.
        #[inline(never)]
        fn innermost() -> usize {
            let (_, len) = capture_encoded();
            len
        }
        #[inline(never)]
        fn middle() -> usize {
            std::hint::black_box(innermost())
        }
        let len = std::hint::black_box(middle());
        if cfg!(all(
            any(target_vendor = "apple", target_os = "linux"),
            any(target_arch = "aarch64", target_arch = "x86_64")
        )) {
            assert!(
                len >= 2 * PC_CHARS,
                "the frame-pointer chain must yield at least two return \
                 addresses from a two-deep call; got {} chars",
                len
            );
        }
    }
}
