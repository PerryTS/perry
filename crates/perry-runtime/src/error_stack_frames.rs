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
//! codegen already fills: `js_register_function_name_static` records
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
//! against node. A resolved frame renders as `    at name (<anonymous>)` —
//! V8's own spelling for a frame whose script position is unknown, and the
//! shape the stack-parsing libraries in real bundles expect.

use super::*;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;

#[cfg(all(test, unix))]
static STATIC_SYMBOL_SPAWN_ATTEMPTS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

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

/// A frame whose nearest registered function starts more than this far below
/// it is not plausibly inside that function: the address belongs to an
/// unregistered one — runtime Rust code, a codegen thunk — that happens to
/// sort after it. Rejecting it is what keeps a native frame out of the trace
/// under some unrelated JS function's name, and dropping a frame is the right
/// way to be wrong here: a MISSING frame is visibly missing, while a
/// MIS-NAMED one sends the reader after the wrong function.
///
/// 64 KiB of machine code is a very large single JS function and a very small
/// slice of the runtime, which is the asymmetry this number trades on.
const MAX_FUNCTION_SPAN: usize = 64 * 1024;

/// `PERRY_ERROR_STACK_DIAG=1` prints, per `.stack` materialisation, what the
/// capture collected and what the resolver made of it.
///
/// Parsed by VALUE, not by presence: `PERRY_GC_DIAG` was `var_os(..).is_some()`
/// for long enough that `PERRY_GC_DIAG=0` ENABLED diagnostics and silently
/// collapsed one arm of an A/B (fixed in #7993). A new knob does not get to
/// repeat that.
pub(crate) fn diag_enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        matches!(
            std::env::var("PERRY_ERROR_STACK_DIAG").ok().as_deref(),
            Some("1") | Some("on") | Some("true")
        )
    })
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+-";

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
        crate::native_stack::stack_top()
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
    pub(super) fn diag_stack_top() -> usize {
        stack_top()
    }

    pub(super) fn diag_frame_pointer() -> usize {
        current_frame_pointer()
    }

    pub(super) fn capture(out: &mut [usize; MAX_CAPTURED_FRAMES]) -> usize {
        let top = stack_top();
        if top == 0 {
            return 0;
        }
        // The low bound is this frame's own stack address. On aarch64 the
        // platform ABI makes x29 a real frame pointer everywhere, so this is
        // belt-and-braces; on x86_64 the runtime's own Rust frames may omit
        // one, in which case `rbp` still holds the frame pointer of the
        // innermost function that DID establish one — generated code always
        // does — and that is exactly the frame the capture wants. What the
        // bound rules out is the remaining case: an `rbp` holding something
        // that is not a stack address at all, which would otherwise start the
        // walk on fabricated frame records.
        let probe = 0usize;
        let low = &probe as *const usize as usize;
        let mut n = 0usize;
        let mut fp = current_frame_pointer();
        if fp < low {
            return 0;
        }
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

    pub(super) fn diag_stack_top() -> usize {
        0
    }

    pub(super) fn diag_frame_pointer() -> usize {
        0
    }
}

/// Capture the current native return addresses and encode them.
/// Returns `(buffer, len)`; `len == 0` means nothing was captured.
pub(crate) fn capture_encoded() -> ([u8; MAX_CAPTURED_FRAMES * PC_CHARS], usize) {
    let mut pcs = [0usize; MAX_CAPTURED_FRAMES];
    let n = walk::capture(&mut pcs);
    if diag_enabled() {
        eprintln!(
            "[stackdiag] capture: frames={n} top={:#x} fp0={:#x} pcs={:x?}",
            walk::diag_stack_top(),
            walk::diag_frame_pointer(),
            &pcs[..n]
        );
    }
    let mut blob = [0u8; MAX_CAPTURED_FRAMES * PC_CHARS];
    if n == 0 {
        return (blob, 0);
    }
    let len = encode_pcs(&pcs[..n], &mut blob);
    (blob, len)
}

/// Raw return addresses of the current native stack, innermost first, for
/// the GC's site-attribution diagnostics (`gc/diag_sites.rs`, the
/// `PERRY_ALLOC_SITE_SAMPLE` sampler). Same walk as `capture_encoded`, no
/// encoding.
pub(crate) fn capture_ips(out: &mut [usize; MAX_CAPTURED_FRAMES]) -> usize {
    walk::capture(out)
}

/// Best-effort one-line description of a code address for diagnostics: the
/// registered JS display name when `ip` is inside a compiled user function,
/// else the nearest dynamic linker symbol (`dladdr`), else an executable-image
/// offset. `PERRY_STACK_SYMBOLS=1` opts into replacing that offset with a
/// retained static symbol. Never called on a hot path — the JS-name index takes
/// a lock, while the expensive static table is never loaded by default.
pub(crate) fn describe_ip(ip: usize) -> String {
    let js = with_index(|index| {
        let lookup_ip = ip.saturating_sub(1);
        symbol_for_ip(index, lookup_ip).and_then(|(start, name)| {
            std::str::from_utf8(name)
                .ok()
                .map(|name| (start, name.to_string()))
        })
    })
    .flatten();
    if let Some((start, name)) = js.filter(|(_, name)| !name.is_empty()) {
        return format_js_description(ip, start, &name);
    }
    #[cfg(unix)]
    {
        if let Some(info) = dladdr_info(ip) {
            if info.dli_sname.is_null() {
                if stack_symbols_enabled() {
                    if let Some((name, off)) = static_symbol_for_ip(ip, info.dli_fbase as usize) {
                        return format!("rt:{name}+{off:#x}");
                    }
                } else if executable_image_base() == Some(info.dli_fbase as usize) {
                    if let Some(off) = ip.checked_sub(info.dli_fbase as usize) {
                        return format!("rt+{off:#x}");
                    }
                }
            } else {
                let name = unsafe { std::ffi::CStr::from_ptr(info.dli_sname) }.to_string_lossy();
                let off = ip.saturating_sub(info.dli_saddr as usize);
                let mut n = name.into_owned();
                if n.len() > 72 {
                    n.truncate(72);
                }
                return format!("{n}+{off:#x}");
            }
        }
    }
    format!("{ip:#x}")
}

fn format_js_description(ip: usize, start: usize, name: &str) -> String {
    if ip.saturating_sub(1).saturating_sub(start) > MAX_FUNCTION_SPAN / 2 {
        format!("js:{name}@{ip:#x}")
    } else {
        format!("js:{name}")
    }
}

#[cfg(unix)]
fn dladdr_info(ip: usize) -> Option<libc::Dl_info> {
    let mut info: libc::Dl_info = unsafe { std::mem::zeroed() };
    // SAFETY: `dladdr` only reads the address and fills `info`.
    (unsafe { libc::dladdr(ip as *const libc::c_void, &mut info) } != 0).then_some(info)
}

/// Static executable symbolization is deliberately opt-in: `nm` is a process
/// spawn and can consume seconds on a large retained-symbol binary. Read the
/// switch once so every default-path miss pays only a cached boolean load.
#[cfg(unix)]
fn stack_symbols_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| crate::gc::env_flag_enabled("PERRY_STACK_SYMBOLS"))
}

/// Loaded base of this executable, derived without opening the executable or
/// spawning a tool. Zero is the cached failure sentinel; supported Unix image
/// bases are non-null.
#[cfg(unix)]
fn executable_image_base() -> Option<usize> {
    static IMAGE_BASE: OnceLock<usize> = OnceLock::new();
    let base = *IMAGE_BASE.get_or_init(|| {
        dladdr_info(load_static_symbol_index as *const () as usize)
            .map(|info| info.dli_fbase as usize)
            .unwrap_or(0)
    });
    (base != 0).then_some(base)
}

#[cfg(unix)]
struct StaticSymbolIndex {
    image_base: usize,
    /// Runtime-relocated `(text symbol start, demangled name)` entries.
    entries: Vec<(usize, String)>,
}

#[cfg(unix)]
fn static_symbol_index() -> &'static Option<StaticSymbolIndex> {
    static INDEX: OnceLock<Option<StaticSymbolIndex>> = OnceLock::new();
    INDEX.get_or_init(load_static_symbol_index)
}

#[cfg(unix)]
fn relocated_address(address: usize, preferred_base: usize, image_base: usize) -> Option<usize> {
    if image_base >= preferred_base {
        address.checked_add(image_base - preferred_base)
    } else {
        address.checked_sub(preferred_base - image_base)
    }
}

#[cfg(unix)]
fn demangle_nm_symbol(name: &str) -> String {
    // Mach-O's assembler-level symbol spelling adds one leading underscore
    // before Rust's `_R...` / `_ZN...` mangling. `nm -C` implementations that
    // know Rust already return a readable name; `try_demangle` rejects that
    // and we preserve it unchanged.
    let candidate = if cfg!(target_vendor = "apple") {
        name.strip_prefix('_').unwrap_or(name)
    } else {
        name
    };
    rustc_demangle::try_demangle(candidate)
        .map(|symbol| symbol.to_string())
        .unwrap_or_else(|_| name.to_string())
}

/// Read the executable's retained `t`/`T` symbols exactly once. `nm -nC`
/// gives us the same address order and demangling a developer would use by
/// hand, without making an object-file parser part of every shipped runtime.
/// A stripped image, a host without `nm`, or an unfamiliar `nm` format simply
/// leaves the old bare-address fallback in place.
#[cfg(unix)]
fn load_static_symbol_index() -> Option<StaticSymbolIndex> {
    let executable = std::env::current_exe().ok()?;
    #[cfg(test)]
    STATIC_SYMBOL_SPAWN_ATTEMPTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let output = std::process::Command::new("nm")
        .arg("-nC")
        .arg(executable)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = std::str::from_utf8(&output.stdout).ok()?;
    let mut preferred_base = None;
    let mut raw_entries = Vec::new();
    for line in text.lines() {
        let mut fields = line.split_whitespace();
        let Some(address) = fields
            .next()
            .and_then(|word| usize::from_str_radix(word, 16).ok())
        else {
            continue;
        };
        let Some(kind) = fields.next() else {
            continue;
        };
        let name = fields.collect::<Vec<_>>().join(" ");
        if name.ends_with("__mh_execute_header") || name.ends_with("__executable_start") {
            preferred_base = Some(address);
        }
        if !matches!(kind, "t" | "T") || name.is_empty() {
            continue;
        }
        raw_entries.push((address, demangle_nm_symbol(&name)));
    }
    if raw_entries.is_empty() {
        return None;
    }
    raw_entries.sort_unstable_by_key(|(address, _)| *address);

    // `dladdr` reports the loaded base. Mach-O and GNU-linked ELF images also
    // publish their preferred base as one of the marker symbols above. For an
    // ELF variant without that marker, select PIE-relative versus absolute
    // `nm` addresses by whichever places a text symbol nearest this function.
    let image_base = executable_image_base()?;
    let preferred_base = preferred_base.unwrap_or_else(|| {
        let anchor = load_static_symbol_index as *const () as usize;
        let absolute_distance = raw_entries
            .iter()
            .map(|(address, _)| address.abs_diff(anchor))
            .min()
            .unwrap_or(usize::MAX);
        let relative_distance = raw_entries
            .iter()
            .filter_map(|(address, _)| address.checked_add(image_base))
            .map(|address| address.abs_diff(anchor))
            .min()
            .unwrap_or(usize::MAX);
        if relative_distance < absolute_distance {
            0
        } else {
            image_base
        }
    });

    let mut entries: Vec<_> = raw_entries
        .into_iter()
        .filter_map(|(address, name)| {
            relocated_address(address, preferred_base, image_base).map(|address| (address, name))
        })
        .collect();
    entries.sort_unstable_by_key(|(address, _)| *address);
    entries.dedup_by(|right, left| right.0 == left.0);
    (!entries.is_empty()).then_some(StaticSymbolIndex {
        image_base,
        entries,
    })
}

#[cfg(unix)]
fn static_symbol_for_ip(ip: usize, image_base: usize) -> Option<(&'static str, usize)> {
    let index = static_symbol_index().as_ref()?;
    // Never apply the executable's lower-bound table to a shared library.
    if image_base != index.image_base {
        return None;
    }
    let at = index.entries.partition_point(|(address, _)| *address <= ip);
    let (start, name) = &index.entries[at.checked_sub(1)?];
    Some((name.as_str(), ip - *start))
}

/// `describe_ip` for a chain, innermost first, skipping frames inside `skip`
/// (a set of symbol-name substrings the caller considers plumbing). Returns
/// up to `max` descriptions joined by ` < `.
pub(crate) fn describe_chain(pcs: &[usize], max: usize) -> String {
    let mut out = Vec::with_capacity(max);
    for &pc in pcs {
        if out.len() >= max {
            break;
        }
        out.push(describe_ip(pc));
    }
    out.join(" < ")
}

// ---------------------------------------------------------------------------
// Resolution: address -> JS display name.
// ---------------------------------------------------------------------------

struct CodeSymbolIndex {
    /// Generated-bundle registry size the snapshot was taken at. Runtime-owned
    /// registrations are intentionally absent from this index.
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
/// The binary search already guarantees the NEXT registered function starts
/// above `ip`, so the only open question is whether `ip` is inside THIS
/// function or past its end — and the registry has no end addresses, it names
/// starts. [`MAX_FUNCTION_SPAN`] is that missing bound, and it is what stops
/// an address well past the last registered function (every runtime Rust
/// frame, on a link layout that places the archives after the generated
/// objects) from being reported under that function's name.
///
/// The index contains only the codegen-only image registry, so runtime builtin
/// thunks cannot become predecessors at all. The remaining residual is within
/// the bundle: an address inside an UNREGISTERED generated function that sits
/// within the span of a registered one resolves to the preceding registered
/// name. That is the same shape as the collector's own function table
/// (`stack_maps_index.rs`: "a function with no safepoints is absent … so an
/// `ip` inside one resolves to the previous mapped function"). Closing it
/// needs per-function extents, which this registry and Mach-O do not expose.
fn symbol_for_ip(index: &CodeSymbolIndex, ip: usize) -> Option<(usize, &Arc<[u8]>)> {
    let at = index.entries.partition_point(|(addr, _)| *addr <= ip);
    let at = at.checked_sub(1)?;
    let (start, name) = &index.entries[at];
    if ip - *start > MAX_FUNCTION_SPAN {
        return None;
    }
    Some((*start, name))
}

fn name_for_ip(index: &CodeSymbolIndex, ip: usize) -> Option<&Arc<[u8]>> {
    symbol_for_ip(index, ip).map(|(_, name)| name)
}

fn with_index<R>(f: impl FnOnce(&CodeSymbolIndex) -> R) -> Option<R> {
    let mut slot = index_slot().lock().ok()?;
    let current_len = crate::builtins::bundle_function_name_registry_len()?;
    let stale = match slot.as_ref() {
        Some(index) => index.source_len != current_len,
        None => true,
    };
    if stale {
        let mut entries = crate::builtins::bundle_function_name_registry_entries()?;
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
        if diag_enabled() {
            eprintln!(
                "[stackdiag] resolve: registry_entries={} first={:#x} last={:#x}",
                index.entries.len(),
                index.entries.first().map(|(a, _)| *a).unwrap_or(0),
                index.entries.last().map(|(a, _)| *a).unwrap_or(0)
            );
            for pc in &pcs {
                let hit = name_for_ip(index, pc.saturating_sub(1))
                    .and_then(|n| std::str::from_utf8(n).ok().map(|s| s.to_string()));
                eprintln!("[stackdiag]   ip={pc:#x} -> {hit:?}");
            }
        }
        let mut out = String::new();
        let mut rendered = 0usize;
        for pc in &pcs {
            if rendered >= RENDER_LIMIT {
                break;
            }
            // A return address points AFTER the call instruction; on a tail
            // position that byte can belong to the next function, so resolve
            // the call site itself.
            let lookup_ip = pc.saturating_sub(1);
            let Some((start, name)) = symbol_for_ip(index, lookup_ip) else {
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
            // `at <name> (<anonymous>)`, not a bare `at <name>`: V8 already
            // spells an unknown script position `(<anonymous>)`, and keeping
            // the `name (location)` shape is what lets the stack-parsing
            // libraries in real bundles (`error-stack-parser`,
            // `source-map-support`) read the name out of the frame at all.
            out.push_str("    at ");
            out.push_str(name);
            if lookup_ip.saturating_sub(start) > MAX_FUNCTION_SPAN / 2 {
                // The registry has starts, not exact extents. Preserve the JS
                // name inside the accepted span, but expose the raw address in
                // the suspicious outer half so a residual mis-name cannot
                // send an audit after the named function without a clue.
                out.push_str(" [");
                out.push_str(&format!("{pc:#x}"));
                out.push(']');
            }
            out.push_str(" (<anonymous>)");
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

/// #9486: build the `frames` payload for an error being constructed — the
/// encoded native return addresses, plus the recorded #5247 line when there is
/// one. Returns an empty vector when there is nothing to record, in which case
/// no string is allocated at all.
pub(crate) fn capture_frames_payload() -> Vec<u8> {
    let (blob, len) = capture_encoded();
    let recorded = recorded_stack_frame();
    if len == 0 && recorded.is_none() {
        return Vec::new();
    }
    let recorded = recorded.unwrap_or_default();
    let mut out = Vec::with_capacity(len + recorded.len() + 1);
    out.extend_from_slice(&blob[..len]);
    if !recorded.is_empty() {
        out.push(b'\n');
        out.extend_from_slice(recorded.as_bytes());
    }
    out
}

/// #9486: render the frame lines of a `.stack` from a captured `frames`
/// payload. The recorded #5247 call site comes first (it is the innermost
/// position we know), then the resolved native frames outward.
pub(crate) fn frames_payload_to_lines(payload: &[u8]) -> String {
    let (blob, recorded) = match payload.iter().position(|b| *b == b'\n') {
        Some(at) => (&payload[..at], std::str::from_utf8(&payload[at + 1..]).ok()),
        None => (payload, None),
    };
    let resolved = render_frames(blob);
    match (recorded, resolved) {
        (Some(line), Some(frames)) => format!("{line}\n{frames}"),
        (Some(line), None) => line.to_string(),
        (None, Some(frames)) => frames,
        (None, None) => "    at <anonymous>".to_string(),
    }
}

/// #9486: format-and-memoise half of [`js_error_get_stack`].
pub(crate) unsafe fn materialize_error_stack(error: *mut ErrorHeader) -> *mut StringHeader {
    if !(*error).stack.is_null() {
        return (*error).stack;
    }
    let name = read_string_header_owned((*error).name);
    let message = read_string_header_owned((*error).message);
    let head = if name.is_empty() {
        message
    } else if message.is_empty() {
        name
    } else {
        format!("{name}: {message}")
    };
    let head = if head.is_empty() {
        "Error".to_string()
    } else {
        head
    };
    let payload = read_string_header_owned((*error).frames);
    let text = format!("{head}\n{}", frames_payload_to_lines(payload.as_bytes()));

    // The string birth can collect, and `error` is a bare pointer: root it
    // across the allocation and re-read it afterwards, or a moving scavenge
    // leaves the memoising store writing into from-space.
    let scope = crate::gc::RuntimeHandleScope::new();
    let error_handle = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(error as i64));
    let stack_ptr = js_string_from_bytes(text.as_ptr(), text.len() as u32);
    let stack_handle = scope.root_string_ptr(stack_ptr);
    let error =
        crate::value::js_nanbox_get_pointer(error_handle.get_nanbox_f64()) as *mut ErrorHeader;
    stack_handle.with_mut_ptr::<StringHeader, _>(|stack_ptr| {
        crate::gc::runtime_store_gc_heap_word_slot(
            error as usize,
            &(*error).stack as *const _ as usize,
            stack_ptr as u64,
        );
        // The capture has served its purpose; releasing it keeps a long-lived
        // error from pinning 128 bytes of encoded addresses forever.
        crate::gc::runtime_store_gc_heap_word_slot(
            error as usize,
            &(*error).frames as *const _ as usize,
            0,
        );
        stack_ptr
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[inline(never)]
    fn kept_runtime_symbol_probe(value: usize) -> usize {
        std::hint::black_box(value)
            .wrapping_mul(0x9486)
            .wrapping_add(17)
    }

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
        assert!(
            decode_pcs(b"AAAAAAA!").is_empty(),
            "character outside the alphabet"
        );
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

    /// Builtin/getter thunks use the owned name-registration entry point. The
    /// stack index must use only codegen's image registry, or the lower-bound
    /// lookup gives every nearby runtime function the thunk's JS name.
    #[test]
    fn describe_ip_does_not_name_runtime_code_after_a_builtin_thunk() {
        const THUNK_START: usize = 0x0000_7e00_0000_0000;
        const THUNK_NAME: &[u8] = b"get years";
        const BUNDLE_START: usize = THUNK_START - 2 * MAX_FUNCTION_SPAN;
        const BUNDLE_NAME: &[u8] = b"bundleWitness9979";
        unsafe {
            crate::builtins::js_register_function_name_static(
                BUNDLE_START as *const u8,
                BUNDLE_NAME.as_ptr(),
                BUNDLE_NAME.len() as u32,
            );
            crate::builtins::js_register_function_name(
                THUNK_START as *const u8,
                THUNK_NAME.as_ptr(),
                THUNK_NAME.len() as u32,
            );
        }
        // Tests run with one thread for this process-global registry. Force a
        // post-registration snapshot so changing the snapshot source back to
        // the combined registry is a real, deterministic sabotage.
        *index_slot().lock().expect("code-symbol index lock") = None;

        assert_eq!(
            crate::builtins::function_name_for_ptr(THUNK_START).as_deref(),
            Some("get years"),
            "the excluded thunk must still have its reflection name"
        );
        assert_eq!(describe_ip(BUNDLE_START + 32), "js:bundleWitness9979");
        let ip = THUNK_START + 32;
        let last_bundle_start =
            with_index(|index| index.entries.last().map(|(start, _)| *start)).flatten();
        assert!(
            last_bundle_start.is_none_or(|start| ip.saturating_sub(start) > MAX_FUNCTION_SPAN),
            "the fake runtime thunk must sit outside the generated bundle range"
        );
        let description = describe_ip(ip);
        assert!(
            !description.starts_with("js:") && !description.contains("get years"),
            "runtime code after an owned builtin thunk was misnamed: {description}"
        );

        let mut blob = [0u8; MAX_CAPTURED_FRAMES * PC_CHARS];
        let len = encode_pcs(&[BUNDLE_START + 32, ip], &mut blob);
        assert_eq!(
            render_frames(&blob[..len]).as_deref(),
            Some("    at bundleWitness9979 (<anonymous>)"),
            "rendering must retain the bundle frame and omit the runtime thunk"
        );
        let far_ip = BUNDLE_START + MAX_FUNCTION_SPAN / 2 + 2;
        let len = encode_pcs(&[far_ip], &mut blob);
        assert_eq!(
            render_frames(&blob[..len]),
            Some(format!(
                "    at bundleWitness9979 [{far_ip:#x}] (<anonymous>)"
            )),
            "the rendered stack must preserve a suspicious frame's address"
        );
    }

    /// The outer half of the heuristic containment span is deliberately still
    /// accepted, but it must carry the original address so a residual false
    /// association is visible to the reader.
    #[test]
    fn suspicious_js_name_keeps_the_raw_ip_visible() {
        let start = 0x1000;
        let ip = start + MAX_FUNCTION_SPAN / 2 + 2;
        assert_eq!(
            format_js_description(ip, start, "maybeFunction"),
            format!("js:maybeFunction@{ip:#x}")
        );
    }

    /// The shipping default identifies executable-image misses by their PIE
    /// offset and must never enter the `nm` spawn path. The child process gives
    /// the cached flag a deterministic unset environment; removing the gate is
    /// sabotage-proved by the spawn counter becoming non-zero.
    #[cfg(unix)]
    #[test]
    fn describe_ip_defaults_to_an_executable_offset_without_spawning_nm() {
        const CHILD_ENV: &str = "PERRY_TEST_STACK_SYMBOLS_DEFAULT_CHILD";
        if std::env::var_os(CHILD_ENV).is_none() {
            let status =
                std::process::Command::new(std::env::current_exe().expect("current test binary"))
                    .arg("describe_ip_defaults_to_an_executable_offset_without_spawning_nm")
                    .arg("--nocapture")
                    .env_remove("PERRY_STACK_SYMBOLS")
                    .env(CHILD_ENV, "1")
                    .status()
                    .expect("launch isolated default stack-symbol witness");
            assert!(status.success(), "default stack-symbol witness failed");
            return;
        }

        assert!(!stack_symbols_enabled(), "the default child must be OFF");
        assert_eq!(
            STATIC_SYMBOL_SPAWN_ATTEMPTS.load(std::sync::atomic::Ordering::Relaxed),
            0
        );
        let ip = kept_runtime_symbol_probe as *const () as usize;
        assert_ne!(std::hint::black_box(kept_runtime_symbol_probe(3)), 0);
        let info = dladdr_info(ip).expect("the probe must belong to the main executable image");
        assert!(
            info.dli_sname.is_null(),
            "the probe unexpectedly has a dynamic symbol; this test must exercise the dladdr miss"
        );
        let image_base = executable_image_base().expect("the executable base must resolve");
        assert_eq!(info.dli_fbase as usize, image_base);

        let description = describe_ip(ip);
        assert_eq!(description, format!("rt+{:#x}", ip - image_base));
        assert_eq!(
            STATIC_SYMBOL_SPAWN_ATTEMPTS.load(std::sync::atomic::Ordering::Relaxed),
            0,
            "the default address fallback must not attempt to spawn `nm`"
        );
    }

    /// A kept local Rust symbol is absent from the dynamic symbol table but is
    /// present in the executable's static `t` table. This isolated child opts
    /// into `PERRY_STACK_SYMBOLS`; no in-process test mutates the cached flag.
    #[cfg(unix)]
    #[test]
    fn describe_ip_names_a_kept_runtime_symbol_when_nm_is_opted_in() {
        const CHILD_ENV: &str = "PERRY_TEST_STACK_SYMBOLS_NM_CHILD";
        if std::env::var_os(CHILD_ENV).is_none() {
            let status =
                std::process::Command::new(std::env::current_exe().expect("current test binary"))
                    .arg("describe_ip_names_a_kept_runtime_symbol_when_nm_is_opted_in")
                    .arg("--nocapture")
                    .env("PERRY_STACK_SYMBOLS", "1")
                    .env(CHILD_ENV, "1")
                    .status()
                    .expect("launch isolated opt-in stack-symbol witness");
            assert!(status.success(), "opt-in stack-symbol witness failed");
            return;
        }

        assert!(stack_symbols_enabled(), "the opt-in child must be ON");
        let ip = kept_runtime_symbol_probe as *const () as usize;
        assert_ne!(std::hint::black_box(kept_runtime_symbol_probe(3)), 0);
        let info = dladdr_info(ip).expect("the probe must belong to the main executable image");
        assert!(
            info.dli_sname.is_null(),
            "the probe unexpectedly has a dynamic symbol; this test must exercise the dladdr miss"
        );

        let description = describe_ip(ip);
        assert_eq!(
            STATIC_SYMBOL_SPAWN_ATTEMPTS.load(std::sync::atomic::Ordering::Relaxed),
            1,
            "the opted-in path must attempt exactly one `nm` spawn"
        );
        if static_symbol_index().is_none() {
            eprintln!("skipped symbol assertion: `nm -nC` could not read this test binary");
            return;
        }
        assert!(
            description.starts_with("rt:"),
            "static fallback did not identify a runtime symbol: {description}"
        );
        assert!(
            description.contains("kept_runtime_symbol_probe"),
            "static fallback did not demangle the probe name: {description}"
        );
        assert!(
            description.ends_with("+0x0"),
            "the exact function start should have offset zero: {description}"
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
