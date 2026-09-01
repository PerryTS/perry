//! String concatenation: pairwise, fused with NaN-boxed value, and n-way chain.

use super::intern::{
    concat_content_matches, fnv1a_concat, with_intern_table, INTERN_MAX_BYTE_LEN, INTERN_TABLE_MASK,
};
use super::*;

/// SSO-aware string concatenation: takes both operands as NaN-boxed f64
/// values, returns the result as an SSO `f64` when total ≤
/// `SHORT_STRING_MAX_LEN` (zero heap alloc), or as a heap `STRING_TAG`-
/// boxed pointer otherwise.
///
/// This is the engine-style fast path for `s + t` in code where both
/// operands are statically-typed strings. The previous lowering had
/// codegen `unbox_str_handle` each operand (which materialises SSO →
/// heap, defeating the whole SSO win), call `js_string_concat`
/// (heap-only), then re-NaN-box the result. For ABC451D's recursive
/// `before + after` (1.4M concats with 1-9 byte operands, all SSO), that
/// was 3 heap allocations per concat. The new path keeps SSO inline
/// throughout — for the common case where both operands AND the
/// result fit SSO (≤ 5 bytes total), there's literally zero heap
/// allocation. Result is returned NaN-boxed so callers don't need a
/// follow-up wrap.
/// Canonicalize a freshly-built concatenation result for WTF-8: a lone high
/// surrogate immediately followed by a lone low surrogate (which can newly
/// arise when two surrogate-bearing strings are joined, e.g.
/// `String.fromCharCode(0xD83D) + String.fromCharCode(0xDE00)` or
/// `"\uD83D" + "\uDE00"`) must be stored as the astral code point's 4-byte
/// UTF-8, so `codePointAt` / console output match JS. Each surrogate is a
/// 3-byte WTF-8 sequence: high = `ED A0..AF 80..BF`, low = `ED B0..BF 80..BF`.
///
/// Cheap-gated by the caller on `STRING_FLAG_HAS_LONE_SURROGATES`: ordinary
/// (ASCII / valid-UTF-8) concatenations never reach here. When no adjacent
/// pair exists (a genuinely lone surrogate), the input pointer is returned
/// unchanged; only an actual merge allocates a new string. `utf16_len` is
/// unaffected (a pair and its astral form are both 2 code units). (#4793)
pub(crate) fn canonicalize_surrogate_pairs(ptr: *mut StringHeader) -> *mut StringHeader {
    if !is_valid_string_ptr(ptr) {
        return ptr;
    }
    let (blen, u16len, flags) = unsafe { ((*ptr).byte_len, (*ptr).utf16_len, (*ptr).flags) };
    if flags & STRING_FLAG_HAS_LONE_SURROGATES == 0 || blen < 6 {
        return ptr;
    }
    let bytes = unsafe { std::slice::from_raw_parts(string_data(ptr), blen as usize) };

    // First pass: is there any adjacent high→low surrogate pair? Avoid all
    // allocation for the common single-lone-surrogate result.
    let is_high = |s: &[u8]| s[0] == 0xED && (0xA0..=0xAF).contains(&s[1]);
    let is_low = |s: &[u8]| s[0] == 0xED && (0xB0..=0xBF).contains(&s[1]);
    let mut has_pair = false;
    let mut i = 0usize;
    while i + 6 <= bytes.len() {
        if is_high(&bytes[i..]) && is_low(&bytes[i + 3..]) {
            has_pair = true;
            break;
        }
        i += 1;
    }
    if !has_pair {
        return ptr;
    }

    // Second pass: rebuild, merging each high→low pair into 4-byte UTF-8 and
    // tracking whether any surrogate remains lone (to keep the flag accurate).
    let mut out: Vec<u8> = Vec::with_capacity(blen as usize);
    let mut still_has_lone = false;
    let mut i = 0usize;
    while i < bytes.len() {
        let rem = &bytes[i..];
        if rem.len() >= 3 && rem[0] == 0xED && (0xA0..=0xBF).contains(&rem[1]) {
            // A surrogate sequence. Try to pair a high with a following low.
            if is_high(rem) && rem.len() >= 6 && is_low(&rem[3..]) {
                let hi = ((rem[0] as u32 & 0x0F) << 12)
                    | ((rem[1] as u32 & 0x3F) << 6)
                    | (rem[2] as u32 & 0x3F);
                let lo = ((rem[3] as u32 & 0x0F) << 12)
                    | ((rem[4] as u32 & 0x3F) << 6)
                    | (rem[5] as u32 & 0x3F);
                let astral = 0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00);
                let ch = unsafe { char::from_u32_unchecked(astral) };
                let mut buf = [0u8; 4];
                out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                i += 6;
                continue;
            }
            // Lone surrogate — copy verbatim, remember the flag stays set.
            still_has_lone = true;
            out.extend_from_slice(&rem[..3]);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }

    let new_flags = if still_has_lone {
        STRING_FLAG_HAS_LONE_SURROGATES
    } else {
        0
    };
    js_string_from_bytes_known_utf16(out.as_ptr(), out.len() as u32, u16len, new_flags)
}

/// True when the `len` bytes at `data` are all ASCII (`< 0x80`), or the slice
/// is empty/null. Used to decide whether a concat result may be stored inline
/// as an SSO value (whose length tag doubles as the JS `.length`).
#[inline]
fn bytes_all_ascii(data: *const u8, len: u32) -> bool {
    if data.is_null() || len == 0 {
        return true;
    }
    unsafe { std::slice::from_raw_parts(data, len as usize) }
        .iter()
        .all(|&b| b < 0x80)
}

/// `ptr::copy_nonoverlapping` with a byte loop for short payloads: the libc
/// `memmove`/`memcpy` PLT call costs more than the copy itself for the
/// digit-and-slug-sized strings the concat hot paths assemble (a 3-byte
/// prefix + 3 digits was paying two `_platform_memmove` calls per op, ~24%
/// of the `"id-" + i` loop in `sample`). 16 is past every SSO/digit shape
/// while keeping the loop trivially unrollable.
///
/// # Safety
/// Same contract as `ptr::copy_nonoverlapping`: both regions valid for
/// `len` bytes, non-overlapping.
#[inline(always)]
pub(crate) unsafe fn copy_bytes_small(src: *const u8, dst: *mut u8, len: usize) {
    // Overlapping-window chunk copies, NOT a byte loop: LLVM's loop-idiom
    // pass recognises a plain byte loop and emits the very `memcpy` call
    // this helper exists to avoid (verified in `sample`: the loop version
    // still showed `_platform_memmove`). Each arm reads/writes two windows
    // that both lie inside `[0, len)`, so nothing outside the regions is
    // touched even when the windows overlap each other.
    if len >= 16 {
        ptr::copy_nonoverlapping(src, dst, len);
    } else if len >= 8 {
        let head = src.cast::<u64>().read_unaligned();
        let tail = src.add(len - 8).cast::<u64>().read_unaligned();
        dst.cast::<u64>().write_unaligned(head);
        dst.add(len - 8).cast::<u64>().write_unaligned(tail);
    } else if len >= 4 {
        let head = src.cast::<u32>().read_unaligned();
        let tail = src.add(len - 4).cast::<u32>().read_unaligned();
        dst.cast::<u32>().write_unaligned(head);
        dst.add(len - 4).cast::<u32>().write_unaligned(tail);
    } else if len >= 2 {
        let head = src.cast::<u16>().read_unaligned();
        let tail = src.add(len - 2).cast::<u16>().read_unaligned();
        dst.cast::<u16>().write_unaligned(head);
        dst.add(len - 2).cast::<u16>().write_unaligned(tail);
    } else if len == 1 {
        // GC_STORE_AUDIT(POINTER_FREE): string payload BYTES, not JSValues —
        // this helper copies UTF-8 into freshly allocated storage, so no slot
        // here can hold a heap edge and no barrier applies. The wider arms
        // above do the same copy through `write_unaligned`.
        *dst = *src;
    }
}

/// SSO-aware pairwise `a + b` for two operands the codegen believes are
/// strings. Both operands arrive NaN-boxed so an SSO operand stays inline, and
/// the result is NaN-boxed too — SSO when the total fits five ASCII bytes, a
/// heap `StringHeader` otherwise.
///
/// **A non-string operand is delegated, not treated as empty.** Perry does not
/// validate declared types at runtime, so "the codegen believes these are
/// strings" is a claim about an annotation, not about the bits. This function
/// used to `unwrap_or((null, 0))` such an operand, which silently rendered
/// `"ab" + 42` as `"ab"`; the codegen then had to route every possibly-lying
/// operand around it (see the `dother`/`cold` arms of the self-append lowering
/// in `lower_string_concat.rs`, whose comment says exactly that). Handing the
/// pair to [`js_dynamic_string_or_number_add`] instead gives a lie the full
/// spec answer — `ToPrimitive`, string concat when either side really is a
/// string, numeric add when neither is — so a static string proof is now a
/// PERFORMANCE claim that cannot change a program's output.
///
/// [`js_dynamic_string_or_number_add`]: crate::value::js_dynamic_string_or_number_add
#[no_mangle]
pub extern "C" fn js_string_concat_box(l_value: f64, r_value: f64) -> f64 {
    let mut scratch_l = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let mut scratch_r = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    // A PLAIN-F64 small-integer operand next to a string operand is the
    // template-literal hot shape (`` `id-${i}` ``): itoa it into a stack
    // buffer and flow it through the same SSO/heap assembly the two-string
    // case uses, instead of handing the pair to the full dynamic `+` (whose
    // ToPrimitive round trip and `format!` formatting were the template
    // path's entire overhead). Only exact spec-identical cases are taken:
    // an integral value in 0..=999_999_999 prints identically under itoa
    // and `Number::toString`; anything else — fractional, negative, huge,
    // NaN-boxed — keeps the dynamic arm. One side must still be a REAL
    // string, so the annotation-lie semantics of the dynamic arm are
    // unchanged for number+number.
    #[inline]
    fn itoa_operand(bits_value: f64, buf: &mut [u8; 32]) -> Option<(*const u8, u32)> {
        let bits = bits_value.to_bits();
        let tag = bits >> 48;
        let is_plain_f64 = tag < 0x7FF8 || (tag == 0x7FF8 && (bits & 0x000F_FFFF_FFFF_FFFF) == 0);
        if is_plain_f64 && bits_value.fract() == 0.0 && (0.0..=999_999_999.0).contains(&bits_value)
        {
            let len = fast_itoa_u32(bits_value as u32, buf);
            Some((buf.as_ptr(), len as u32))
        } else {
            None
        }
    }
    let l_str = str_bytes_from_jsvalue(l_value, &mut scratch_l);
    let r_str = str_bytes_from_jsvalue(r_value, &mut scratch_r);
    if let (Some(l), Some(r)) = (l_str, r_str) {
        // Two real strings: straight to assembly, no number buffer touched
        // (the itoa scratch below would cost this path a 32-byte memset).
        return concat_byte_parts(l, r);
    }
    // Exactly one side can be a string here, so a single itoa buffer serves
    // both mixed arms; it must outlive the call since `concat_byte_parts`
    // reads through the raw pointer.
    let mut num_buf = [0u8; 32];
    match (l_str, r_str) {
        (Some(l), None) => {
            if let Some(r) = itoa_operand(r_value, &mut num_buf) {
                return concat_byte_parts(l, r);
            }
        }
        (None, Some(r)) => {
            if let Some(l) = itoa_operand(l_value, &mut num_buf) {
                return concat_byte_parts(l, r);
            }
        }
        _ => {}
    }
    // `str_bytes_from_jsvalue` returns `None` for exactly the non-string
    // values, so every remaining pair — number+number included — is the
    // annotation-lie arm and nothing else.
    unsafe { crate::value::js_dynamic_string_or_number_add(l_value, r_value) }
}

/// Memo for short ASCII concat results (`"field_" + j`, `"row" + i`, slug and
/// id building — the shape that dominates computed property keys).
///
/// A result of 6..=`CONCAT_MEMO_MAX_BYTES` bytes is past SSO's 5-byte ceiling,
/// so today every one of them heap-allocates. In a build-then-key loop that is
/// pure garbage: measured on `bench_object_property`, 200k such allocations per
/// run, whose only consumer is the property-key lookup that reads their bytes
/// and drops them. They cost a young collection, and that collection costs far
/// more than the allocations themselves (a scavenge copying 248 KB takes ~7 ms,
/// most of it fixed per-collection work).
///
/// Returning the SAME string object for equal bytes is unobservable: JS strings
/// are primitives, `===` compares by value, and a concat result is born
/// `refcount = 0` (shared), which is exactly the marker that forbids the
/// in-place `+=` append — so a memo hit can never be mutated under its other
/// holders. That the results are already shared is what makes this sound
/// without any new discipline at the call sites.
///
/// Bounded and evicting on collision, so it can never grow: entries are strong
/// GC roots (`scan_concat_memo_roots_mut`) rather than pinned, so an evicted
/// string becomes collectable immediately. Pinning would have made a long
/// program's distinct short concats accumulate forever — a memory-parity
/// regression traded for wall time, which is the wrong trade
/// (`memory-parity-directive`).
const CONCAT_MEMO_SIZE: usize = 512;
const CONCAT_MEMO_MAX_BYTES: u32 = 12;

// Candidates per governor window.
const MEMO_WINDOW: u32 = 4096;
// Earn the probe: at least a quarter of a window's candidates must hit.
const MEMO_MIN_HIT_SHIFT: u32 = 2;
// A hostile workload ends up probing one window in 2^8 rather than one in two.
const MEMO_MAX_BACKOFF: u32 = 8;

const GOV_ENABLED: u64 = 1 << 63;
const GOV_HIT_SHIFT: u32 = 32;
const GOV_COUNT_MASK: u64 = 0xFFFF_FFFF;

static MEMO_GOV: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1 << 63);
static MEMO_SKIP_WINDOWS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static MEMO_BACKOFF: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Count this candidate and report whether the memo path is worth walking.
/// Closes a window every `MEMO_WINDOW` candidates.
#[inline]
fn concat_memo_should_probe() -> bool {
    use std::sync::atomic::Ordering::Relaxed;
    let v = MEMO_GOV.load(Relaxed);
    let count = (v & GOV_COUNT_MASK) + 1;
    if count < MEMO_WINDOW as u64 {
        MEMO_GOV.store((v & !GOV_COUNT_MASK) | count, Relaxed);
        return v & GOV_ENABLED != 0;
    }
    let hits = (v >> GOV_HIT_SHIFT) & 0x7FFF_FFFF;
    let now_enabled = if v & GOV_ENABLED != 0 {
        if (hits << MEMO_MIN_HIT_SHIFT) >= count {
            MEMO_BACKOFF.store(0, Relaxed);
            true
        } else {
            let b = (MEMO_BACKOFF.load(Relaxed) + 1).min(MEMO_MAX_BACKOFF);
            MEMO_BACKOFF.store(b, Relaxed);
            MEMO_SKIP_WINDOWS.store(1u32 << b, Relaxed);
            false
        }
    } else {
        // Serving out a backoff; expire it into a probation window so a
        // workload that turns hot later is still picked up.
        let left = MEMO_SKIP_WINDOWS.load(Relaxed).saturating_sub(1);
        MEMO_SKIP_WINDOWS.store(left, Relaxed);
        left == 0
    };
    MEMO_GOV.store(if now_enabled { GOV_ENABLED } else { 0 }, Relaxed);
    now_enabled
}

#[inline]
fn concat_memo_note_hit() {
    MEMO_GOV.fetch_add(1u64 << GOV_HIT_SHIFT, std::sync::atomic::Ordering::Relaxed);
}

#[cfg(test)]
pub(crate) fn test_reset_memo_governor() {
    use std::sync::atomic::Ordering::Relaxed;
    MEMO_GOV.store(GOV_ENABLED, Relaxed);
    MEMO_SKIP_WINDOWS.store(0, Relaxed);
    MEMO_BACKOFF.store(0, Relaxed);
    CONCAT_MEMO_TAGS.with(|t| unsafe { (*t.get()).fill(0) });
}

#[cfg(test)]
pub(crate) fn test_memo_enabled() -> bool {
    MEMO_GOV.load(std::sync::atomic::Ordering::Relaxed) & GOV_ENABLED != 0
}

#[cfg(test)]
pub(crate) fn test_memo_window() -> u32 {
    MEMO_WINDOW
}

#[cfg(test)]
pub(crate) fn test_memo_should_probe() -> bool {
    concat_memo_should_probe()
}

#[cfg(test)]
pub(crate) fn test_memo_note_hit() {
    concat_memo_note_hit();
}

// Admission doorkeeper (#9391). A result must be observed TWICE before it
// earns a memo entry.
//
// Without this the memo is a net loss on exactly the workload it looks most
// applicable to. `bench_gc_pressure` builds `"item_" + i` half a million
// times with every result distinct: the memo missed 100% of the time, yet
// inserted on every miss — keeping up to 512 strings alive as strong GC
// roots, promoting garbage that should have died young, and adding a root
// scan per collection, all for zero hits. Measured 18 ms with the memo
// against 12 ms without.
//
// A one-byte hash tag per slot fixes it by construction rather than by
// governor: a never-repeated key writes its tag and leaves, so it never costs
// an allocation or a root, while a reused key matches its own tag on the
// second occurrence and is admitted then. No windows, no hit-rate counters,
// and no disabled state that can get stuck off. The tags are plain bytes —
// never addresses — so they are invisible to the collector.
//
// A tag collision between two distinct keys only admits one of them a little
// early; the entry itself is still content-compared on lookup, so admission
// can never produce a wrong string.
thread_local! {
    static CONCAT_MEMO_TAGS: std::cell::UnsafeCell<[u8; CONCAT_MEMO_SIZE]> =
        const { std::cell::UnsafeCell::new([0u8; CONCAT_MEMO_SIZE]) };
}

/// Returns true when this result has been seen before and may be admitted.
/// Otherwise records it and declines, so the first sighting costs nothing but
/// a byte.
#[inline]
fn concat_memo_admit(slot: usize, tag: u8) -> bool {
    CONCAT_MEMO_TAGS.with(|t| unsafe {
        let slot_tag = &mut (*t.get())[slot];
        if *slot_tag == tag {
            true
        } else {
            *slot_tag = tag;
            false
        }
    })
}

thread_local! {
    static CONCAT_MEMO: std::cell::UnsafeCell<[*mut StringHeader; CONCAT_MEMO_SIZE]> =
        const { std::cell::UnsafeCell::new([std::ptr::null_mut(); CONCAT_MEMO_SIZE]) };
}

/// Slot and admission tag from one hash walk. The tag is a different slice of
/// the same digest, so two keys sharing a slot rarely share a tag.
#[inline]
fn concat_memo_slot_and_tag(bytes: &[u8]) -> (usize, u8) {
    let h = concat_memo_hash(bytes);
    // FNV-1a avalanches poorly in its high bits, so slicing a tag straight out
    // of `h >> 32` gave two distinct keys the same tag about half the time —
    // measured 256,516 admissions in 501,000 probes where ~1/128 was intended,
    // which defeats the doorkeeper entirely. A splitmix64 finalizer first.
    let mut z = h;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^= z >> 31;
    (
        (z as usize) & (CONCAT_MEMO_SIZE - 1),
        // Never 0: that is the "no key seen here yet" sentinel.
        (((z >> 40) as u8) | 1),
    )
}

#[inline]
fn concat_memo_hash(bytes: &[u8]) -> u64 {
    // FNV-1a over the result bytes. Content-addressed, so two different
    // operand splits that produce the same string share one entry.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

/// A cached string with exactly these bytes, or null. The byte compare makes
/// a hash collision a miss, never a wrong answer.
#[inline]
fn concat_memo_lookup(slot: usize, bytes: &[u8]) -> *mut StringHeader {
    let cached = CONCAT_MEMO.with(|c| unsafe { (*c.get())[slot] });
    if cached.is_null() {
        return std::ptr::null_mut();
    }
    unsafe {
        if (*cached).byte_len as usize != bytes.len() {
            return std::ptr::null_mut();
        }
        let data = crate::string::string_data(cached);
        if std::slice::from_raw_parts(data, bytes.len()) != bytes {
            return std::ptr::null_mut();
        }
    }
    cached
}

#[inline]
fn concat_memo_insert(slot: usize, ptr: *mut StringHeader) {
    CONCAT_MEMO.with(|c| unsafe {
        // GC_STORE_AUDIT(ROOT): CONCAT_MEMO is scanned by scan_concat_memo_roots_mut.
        crate::gc::runtime_store_root_raw_mut_ptr_slot(&raw mut (*c.get())[slot], ptr);
    });
}

/// GC root scanner for [`CONCAT_MEMO`]. Same contract as the small-int cache:
/// the entries are strong roots, so evacuation rewrites them rather than
/// leaving the memo holding freed strings.
pub fn scan_concat_memo_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    CONCAT_MEMO.with(|c| unsafe {
        for slot in (*c.get()).iter_mut() {
            let mut addr = *slot as usize;
            if visitor.visit_tagged_usize_slot(&mut addr, crate::value::STRING_TAG) {
                *slot = addr as *mut StringHeader;
            }
        }
    });
}

pub fn scan_concat_memo_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
    scan_concat_memo_roots_mut(&mut visitor);
}

#[cfg(test)]
pub(crate) fn test_clear_concat_memo() {
    CONCAT_MEMO.with(|c| unsafe {
        for slot in (*c.get()).iter_mut() {
            // GC_STORE_AUDIT(ROOT): test cleanup clears CONCAT_MEMO roots.
            crate::gc::runtime_store_root_raw_mut_ptr_slot(&raw mut *slot, std::ptr::null_mut());
        }
    });
}

/// Shared tail of [`js_string_concat_box`]: assemble two raw byte slices
/// (each a real string's payload or an itoa'd integer) into an SSO immediate
/// when the total fits five ASCII bytes, a heap `StringHeader` otherwise.
#[inline(always)]
fn concat_byte_parts(l: (*const u8, u32), r: (*const u8, u32)) -> f64 {
    let total_blen = l.1 + r.1;

    // SSO encodes its length tag as the JS `.length`, so it is only sound for
    // ASCII operands (byte length == UTF-16 length). A non-ASCII operand —
    // multi-byte UTF-8 or a WTF-8 lone surrogate — must take the heap path so
    // the result's `utf16_len` is computed, not assumed equal to `byte_len`
    // (#4793: `("é"+"x").length` was 3, `("\uD800"+"x").length` was 4).
    let both_ascii = bytes_all_ascii(l.0, l.1) && bytes_all_ascii(r.0, r.1);

    // SSO fast path — assemble the result inline when it fits (≤ 5
    // bytes). Pure bit arithmetic, no heap touch.
    if both_ascii && total_blen as usize <= crate::value::SHORT_STRING_MAX_LEN {
        unsafe {
            let mut payload: u64 = 0;
            for i in 0..l.1 as usize {
                payload |= (*l.0.add(i) as u64) << (i * 8);
            }
            for i in 0..r.1 as usize {
                payload |= (*r.0.add(i) as u64) << ((l.1 as usize + i) * 8);
            }
            let len_bits = (total_blen as u64) << crate::value::SHORT_STRING_LEN_SHIFT;
            return f64::from_bits(crate::value::SHORT_STRING_TAG | len_bits | payload);
        }
    }

    // Memo probe, ahead of the allocation: assemble the result into a stack
    // buffer and look it up by content. Restricted to short ASCII results, so
    // `flags`/`utf16_len` are trivially `0`/`total_blen` and the surrogate
    // canonicalization below is a no-op — the cached string is bit-identical
    // to what the heap path would have built.
    let memoizable =
        both_ascii && total_blen <= CONCAT_MEMO_MAX_BYTES && concat_memo_should_probe();
    let mut memo_buf = [0u8; CONCAT_MEMO_MAX_BYTES as usize];
    let mut memo_slot = 0usize;
    let mut memo_admitted = false;
    if memoizable {
        unsafe {
            if l.1 > 0 {
                std::ptr::copy_nonoverlapping(l.0, memo_buf.as_mut_ptr(), l.1 as usize);
            }
            if r.1 > 0 {
                std::ptr::copy_nonoverlapping(
                    r.0,
                    memo_buf.as_mut_ptr().add(l.1 as usize),
                    r.1 as usize,
                );
            }
        }
        let bytes = &memo_buf[..total_blen as usize];
        let (slot, tag) = concat_memo_slot_and_tag(bytes);
        memo_slot = slot;
        let hit = concat_memo_lookup(memo_slot, bytes);
        if !hit.is_null() {
            concat_memo_note_hit();
            return f64::from_bits(crate::value::JSValue::string_ptr(hit).bits());
        }
        // Same admission rule as the `"prefix" + i` arm: a first sighting
        // costs a tag byte, never a rooted string. See `concat_memo_admit`.
        memo_admitted = concat_memo_admit(memo_slot, tag);
    }

    // Heap path — allocate a StringHeader and memcpy. Decode both
    // operands' byte slices via `str_bytes_from_jsvalue` (already done
    // above) and write directly into the new header's payload region.
    let (ptr, data_ptr) = string_storage_alloc(total_blen);
    unsafe {
        // ASCII-fast utf16 length: count bytes < 0x80 in both slices in
        // one pass. Most concat results are pure ASCII (number formatting,
        // ID building, slug construction, etc.); falling back to the
        // full Grisu-style codepoint walk for non-ASCII keeps spec
        // compliance for the edge case.
        let l_slice = if !l.0.is_null() {
            std::slice::from_raw_parts(l.0, l.1 as usize)
        } else {
            &[]
        };
        let r_slice = if !r.0.is_null() {
            std::slice::from_raw_parts(r.0, r.1 as usize)
        } else {
            &[]
        };
        let (utf16_len, flags) = if l_slice.is_ascii() && r_slice.is_ascii() {
            (total_blen, 0)
        } else {
            // Sum each operand's UTF-16 length independently (concatenating two
            // strings never merges code units across the boundary). Carry the
            // lone-surrogate flag forward when an operand is WTF-8 so
            // `isWellFormed()` / `JSON.stringify` stay correct on the result.
            let mut u16 = 0u32;
            let mut flags = 0u32;
            if !l_slice.is_empty() {
                u16 += compute_utf16_len(l.0, l.1);
                if str::from_utf8(l_slice).is_err() {
                    flags |= STRING_FLAG_HAS_LONE_SURROGATES;
                }
            }
            if !r_slice.is_empty() {
                u16 += compute_utf16_len(r.0, r.1);
                if str::from_utf8(r_slice).is_err() {
                    flags |= STRING_FLAG_HAS_LONE_SURROGATES;
                }
            }
            (u16, flags)
        };

        init_string_header(ptr, utf16_len, total_blen, total_blen, 0, flags);
        if !l_slice.is_empty() {
            copy_bytes_small(l.0, data_ptr, l.1 as usize);
        }
        if !r_slice.is_empty() {
            copy_bytes_small(r.0, data_ptr.add(l.1 as usize), r.1 as usize);
        }
        // Merge any surrogate pair newly formed across the join boundary
        // (no-op unless the result carries the lone-surrogate flag).
        let ptr = canonicalize_surrogate_pairs(ptr);
        if memoizable && memo_admitted {
            // Publish only after the header and payload are fully written:
            // the memo is a GC root, so a half-built entry would be traced.
            concat_memo_insert(memo_slot, ptr);
        }
        // NaN-box as STRING_TAG.
        f64::from_bits(crate::value::JSValue::string_ptr(ptr).bits())
    }
}

/// Concatenate two strings
///
/// v0.5.78x perf: consolidate the eight is_valid_string_ptr checks into
/// two (one per input) and read all per-input fields in a single unsafe
/// block. The compiler should CSE the calls but visible source-level
/// duplication makes the codegen path harder to follow and adds a
/// real per-call cost on hot paths (1M concats / 24 ms = 24 ns each).
#[no_mangle]
pub extern "C" fn js_string_concat(
    a: *const StringHeader,
    b: *const StringHeader,
) -> *mut StringHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let a_handle = scope.root_string_ptr(a);
    let b_handle = scope.root_string_ptr(b);

    // Snapshot all validity-gated reads from `a` in one pass. For invalid
    // pointers this stays at the zero-defaults so the rest of the function
    // sees a "behaves like an empty string" view.
    let a_valid = is_valid_string_ptr(a);
    let b_valid = is_valid_string_ptr(b);
    let (blen_a, u16len_a, flags_a) = if a_valid {
        unsafe { ((*a).byte_len, (*a).utf16_len, (*a).flags) }
    } else {
        (0, 0, 0)
    };
    let (blen_b, u16len_b, flags_b) = if b_valid {
        unsafe { ((*b).byte_len, (*b).utf16_len, (*b).flags) }
    } else {
        (0, 0, 0)
    };
    let total_blen = blen_a + blen_b;

    // Intern fast path: if result is short enough, check the intern table
    // before allocating. Repeated property-name concatenations like
    // "field_" + j return the existing interned pointer — zero allocation.
    if total_blen > 0 && total_blen <= INTERN_MAX_BYTE_LEN {
        unsafe {
            let hash = fnv1a_concat(a, blen_a, b, blen_b);
            let slot = (hash as usize) & INTERN_TABLE_MASK;
            let hit = with_intern_table(|table| {
                let entry = &(*table)[slot];
                if entry.string_ptr != 0 && entry.hash == hash {
                    let existing = entry.string_ptr as *const StringHeader;
                    if is_valid_string_ptr(existing)
                        && (*existing).byte_len == total_blen
                        && concat_content_matches(a, blen_a, b, blen_b, existing)
                    {
                        return Some(existing);
                    }
                }
                None
            });
            if let Some(existing) = hit {
                return existing as *mut StringHeader;
            }
        }
    }

    let (ptr, data_ptr) = string_storage_alloc(total_blen);
    let a = a_handle.get_raw_const_ptr::<StringHeader>();
    let b = b_handle.get_raw_const_ptr::<StringHeader>();

    unsafe {
        init_string_header(
            ptr,
            u16len_a + u16len_b,
            total_blen,
            total_blen,
            0,
            flags_a | flags_b,
        );

        if a_valid && blen_a > 0 {
            ptr::copy_nonoverlapping(string_data(a), data_ptr, blen_a as usize);
        }
        if b_valid && blen_b > 0 {
            ptr::copy_nonoverlapping(
                string_data(b),
                data_ptr.add(blen_a as usize),
                blen_b as usize,
            );
        }

        canonicalize_surrogate_pairs(ptr)
    }
}

/// Fused string + NaN-boxed value concatenation (issue #58).
///
/// `"item_" + i` currently requires two gc_malloc calls:
///   1. `js_jsvalue_to_string(i)` → intermediate StringHeader
///   2. `js_string_concat(prefix, intermediate)` → result StringHeader
///
/// This function collapses both into a single allocation when the value
/// is a number (the common case for `"str" + i` patterns in loops).
/// For non-number values, it falls back to js_jsvalue_to_string + concat.
///
/// The number formatting uses `itoa` for integers and a stack buffer for
/// `format!`, eliminating the Rust heap allocation from `format!()`.
#[no_mangle]
pub extern "C" fn js_string_concat_value(
    prefix: *const StringHeader,
    value: f64,
) -> *mut StringHeader {
    // #6655: `prefix` is a raw movable heap pointer held across GC-capable
    // operations — an allocation on the fast path below, and
    // `js_jsvalue_to_string(value)` (an arbitrary user `toString`) on the
    // slow path. Rooting used to happen unconditionally here, but the scope
    // setup + `root_string_ptr` measured ~7% of the `"id-" + i` loop, and the
    // NUMBER arm only needs it when its allocation cannot use the already-open
    // nursery block: `string_storage_alloc_no_collect`'s `Some` contract is
    // "nothing on the heap moved", which keeps every raw read of `prefix`
    // valid with no root at all. So each arm roots for itself: the number arm
    // only in its block-boundary fallback, the user-`toString` arm always.
    let prefix_blen = if is_valid_string_ptr(prefix) {
        unsafe { (*prefix).byte_len }
    } else {
        0
    };
    let prefix_u16 = if is_valid_string_ptr(prefix) {
        unsafe { (*prefix).utf16_len }
    } else {
        0
    };

    // Fast path: value is a number (no NaN-boxing tag in upper 16 bits → plain f64).
    // This covers the hot `"item_" + i` pattern.
    let bits = value.to_bits();
    let tag = bits >> 48;
    let is_plain_f64 = tag < 0x7FF8 || (tag == 0x7FF8 && (bits & 0x000F_FFFF_FFFF_FFFF) == 0);

    if is_plain_f64 {
        // Format the number into a stack buffer
        let mut num_buf = [0u8; 32]; // max f64 string is ~24 chars
        let num_len: usize;

        if value.fract() == 0.0 && value.abs() < 1e15 && !value.is_nan() && !value.is_infinite() {
            // Integer path: format directly without Rust heap allocation
            let n = value as i64;
            if (0..=999_999_999).contains(&n) {
                // Fast itoa for common positive integers
                num_len = fast_itoa_u32(n as u32, &mut num_buf);
            } else {
                let s = format!("{}", n);
                let len = s.len().min(num_buf.len());
                num_buf[..len].copy_from_slice(&s.as_bytes()[..len]);
                num_len = len;
            }
        } else if value.is_nan() {
            num_buf[..3].copy_from_slice(b"NaN");
            num_len = 3;
        } else if value.is_infinite() {
            if value > 0.0 {
                num_buf[..8].copy_from_slice(b"Infinity");
                num_len = 8;
            } else {
                num_buf[..9].copy_from_slice(b"-Infinity");
                num_len = 9;
            }
        } else if value == 0.0 {
            num_buf[0] = b'0';
            num_len = 1;
        } else {
            // #3987: match ECMAScript NumberToString (scientific notation for
            // |n| >= 1e21 / < 1e-6) instead of Rust's full-decimal `{}`.
            let s = super::format::js_format_f64(value);
            let len = s.len().min(num_buf.len());
            num_buf[..len].copy_from_slice(&s.as_bytes()[..len]);
            num_len = len;
        }

        // Single allocation for prefix + number string. `Some` from the
        // no-collect allocator means the open nursery block served it and
        // nothing moved — `prefix` stays valid raw. `None` (block boundary,
        // large size, free-list latch) takes the original rooted path:
        // `string_storage_alloc` → `arena_alloc_gc` can collect and evacuate,
        // so the incoming `prefix` may have moved — re-read it from its
        // handle before touching the header or copying the payload (#6655).
        let total_blen = prefix_blen as usize + num_len;

        // Memo probe, ahead of the allocation. This is the `"prefix" + i`
        // shape (computed property keys, id building) and it is where the
        // allocations actually come from: `js_string_concat_value_box`'s SSO
        // arm handles results up to 5 bytes, so everything longer lands here
        // and heap-allocates. Restricted to a plain ASCII prefix so the cached
        // string is bit-identical to what the block below would build
        // (flags == 0, utf16_len == byte_len).
        let memoizable = total_blen <= CONCAT_MEMO_MAX_BYTES as usize
            && is_valid_string_ptr(prefix)
            && prefix_u16 == prefix_blen
            && unsafe { (*prefix).flags == 0 }
            && bytes_all_ascii(string_data(prefix), prefix_blen)
            && concat_memo_should_probe();
        let mut memo_buf = [0u8; CONCAT_MEMO_MAX_BYTES as usize];
        let mut memo_slot = 0usize;
        let mut memo_admitted = false;
        if memoizable {
            unsafe {
                if prefix_blen > 0 {
                    copy_bytes_small(
                        string_data(prefix),
                        memo_buf.as_mut_ptr(),
                        prefix_blen as usize,
                    );
                }
                copy_bytes_small(
                    num_buf.as_ptr(),
                    memo_buf.as_mut_ptr().add(prefix_blen as usize),
                    num_len,
                );
            }
            let bytes = &memo_buf[..total_blen];
            let (slot, tag) = concat_memo_slot_and_tag(bytes);
            memo_slot = slot;
            let hit = concat_memo_lookup(memo_slot, bytes);
            if !hit.is_null() {
                concat_memo_note_hit();
                return hit;
            }
            // Missed: admit only a result we have seen before, so a stream of
            // never-repeated keys costs a tag byte instead of a rooted string.
            memo_admitted = concat_memo_admit(memo_slot, tag);
        }

        let (ptr, data_ptr, prefix) =
            match crate::string::string_storage_alloc_no_collect(total_blen as u32) {
                Some((ptr, data_ptr)) => (ptr, data_ptr, prefix),
                None => {
                    let scope = crate::gc::RuntimeHandleScope::new();
                    let prefix_handle = scope.root_string_ptr(prefix);
                    let (ptr, data_ptr) = string_storage_alloc(total_blen as u32);
                    (
                        ptr,
                        data_ptr,
                        prefix_handle.get_raw_const_ptr::<StringHeader>(),
                    )
                }
            };

        unsafe {
            // Both prefix and number digits are ASCII, so utf16_len == byte_len for the number part
            let flags = if is_valid_string_ptr(prefix) {
                (*prefix).flags
            } else {
                0
            };
            init_string_header(
                ptr,
                prefix_u16 + num_len as u32,
                total_blen as u32,
                total_blen as u32,
                0,
                flags,
            );

            if is_valid_string_ptr(prefix) && prefix_blen > 0 {
                copy_bytes_small(string_data(prefix), data_ptr, prefix_blen as usize);
            }
            copy_bytes_small(
                num_buf.as_ptr(),
                data_ptr.add(prefix_blen as usize),
                num_len,
            );
        }

        if memoizable && memo_admitted {
            // Publish only after header and payload are written: the memo is a
            // GC root, so a half-built entry would be traced.
            concat_memo_insert(memo_slot, ptr);
        }
        return ptr;
    }

    // Slow path: non-number value — fall back to js_jsvalue_to_string + js_string_concat.
    // `js_jsvalue_to_string` can run a user `toString` and collect, so root
    // `prefix` across it and reload from the handle afterwards (#6655).
    let scope = crate::gc::RuntimeHandleScope::new();
    let prefix_handle = scope.root_string_ptr(prefix);
    let value_str = crate::value::js_jsvalue_to_string(value);
    js_string_concat(prefix_handle.get_raw_const_ptr::<StringHeader>(), value_str)
}

/// NaN-box-returning twin of [`js_string_concat_value`]: SSO immediate when
/// the result fits (≤ 5 ASCII bytes), heap `STRING_TAG` box otherwise.
///
/// The SSO arm matters twice over. It removes the per-iteration allocation
/// from the `"k" + i` computed-key pattern — but more importantly it makes
/// the result's BITS content-stable: `"k" + 42` yields the identical f64
/// every evaluation. Every key-keyed cache downstream (the dynamic-key
/// write IC's ways, the megamorphic write stub, read plans) compares key
/// VALUE bits, so a heap pointer minted fresh per iteration can never hit —
/// which is exactly what the stub-cache counters showed (600k inserts,
/// 1.19M probes, 0 hits, 99% `way_key_neq`). ASCII-only for the same
/// `utf16_len` soundness reason as `js_string_concat_box`'s SSO arm.
#[no_mangle]
pub extern "C" fn js_string_concat_value_box(prefix: *const StringHeader, value: f64) -> f64 {
    // Same "plain f64" test as `js_string_concat_value`'s fast path; the SSO
    // arm additionally wants a small non-negative integer so the digit count
    // comes from `fast_itoa_u32`.
    let bits = value.to_bits();
    let tag = bits >> 48;
    let is_plain_f64 = tag < 0x7FF8 || (tag == 0x7FF8 && (bits & 0x000F_FFFF_FFFF_FFFF) == 0);
    if is_plain_f64
        && value.fract() == 0.0
        && (0.0..=999_999_999.0).contains(&value)
        && is_valid_string_ptr(prefix)
    {
        let prefix_blen = unsafe { (*prefix).byte_len } as usize;
        if prefix_blen < crate::value::SHORT_STRING_MAX_LEN {
            let mut num_buf = [0u8; 32];
            let num_len = fast_itoa_u32(value as u32, &mut num_buf);
            if prefix_blen + num_len <= crate::value::SHORT_STRING_MAX_LEN {
                let data = string_data(prefix);
                if bytes_all_ascii(data, prefix_blen as u32) {
                    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
                    unsafe {
                        copy_bytes_small(data, sso.as_mut_ptr(), prefix_blen);
                        copy_bytes_small(
                            num_buf.as_ptr(),
                            sso.as_mut_ptr().add(prefix_blen),
                            num_len,
                        );
                    }
                    return f64::from_bits(
                        crate::value::JSValue::short_string_unchecked(
                            &sso[..prefix_blen + num_len],
                        )
                        .bits(),
                    );
                }
            }
        }
    }
    let ptr = js_string_concat_value(prefix, value);
    f64::from_bits(crate::value::js_nanbox_string(ptr as i64).to_bits())
}

/// Ceiling on the per-call part count. Must match `CONCAT_CHAIN_MAX_PARTS` in
/// `perry-codegen/src/lower_string_concat.rs`. The cap keeps the stack scratch
/// bounded so a pathological fold cannot overflow the stack.
const CONCAT_CHAIN_MAX_PARTS: usize = 32;

/// N-way string concatenation (v0.5.771).
///
/// Replaces a left-spine of `Binary { Add }` string-concat nodes with a
/// single allocation. Pre-fix `id + "," + name + "," + email + "," + score
/// + "," + ternary + ",2026-05-09"` lowers to nine nested `js_string_concat`
/// calls — each allocates a fresh StringHeader, copies the accumulating
/// prefix, then copies the next part. Total work is quadratic in the
/// number of parts: 9 allocs, ~225 bytes copied per row for the
/// `string_concat_csv` kernel.
///
/// This function does the entire chain in one pass:
///   1. Walk the parts, recording (data_ptr, byte_len) for strings and
///      formatting numbers into a small-int cache or per-part stack buffer.
///   2. Sum the byte lengths.
///   3. One arena allocation sized to the total.
///   4. Copy each part's bytes into the destination.
///
/// `parts` is an array of `n` NaN-boxed `f64` values. The codegen-side
/// fold in `Expr::Binary { Add }` flattens left-spines of string-typed
/// adds and emits this call instead of the pairwise chain.
///
/// Returns a fresh shared (refcount=0) StringHeader. Callers NaN-box
/// with STRING_TAG via the standard `nanbox_string_inline` helper.
#[no_mangle]
pub extern "C" fn js_string_concat_chain(parts: *const f64, n: i32) -> *mut StringHeader {
    let n = (n as usize).min(CONCAT_CHAIN_MAX_PARTS);
    if n == 0 || parts.is_null() {
        return crate::string::js_string_from_bytes(b"".as_ptr(), 0);
    }

    // ★ Size the stack scratch to the chain actually being built. One
    // `MAX_PARTS = 32` shape made EVERY call pay ~2 KB of stack
    // initialisation — the release disassembly opens `sub sp, sp, #0x7e0`,
    // then `memset(sp+0x20, _, 0x400)` for `num_bufs`, then 32 `str xzr` for
    // the handle array — whether the chain had 32 parts or 2. Real chains are
    // 2-4 parts: `seen = seen + "[" + names[i] + "]"` in an environment-lookup
    // loop is four, and was memsetting 2 KB per append.
    if n <= 4 {
        concat_chain_sized::<4>(parts, n)
    } else if n <= 8 {
        concat_chain_sized::<8>(parts, n)
    } else {
        concat_chain_sized::<CONCAT_CHAIN_MAX_PARTS>(parts, n)
    }
}

/// N-way concat for `s = s + a + b + ...`, where `parts[0]` is an owner read
/// of `s`. An all-heap-string chain can copy the suffix pieces straight into a
/// unique accumulator, or allocate the complete result once when it cannot.
/// Other value shapes retain the ordinary concat-chain semantics.
#[no_mangle]
pub extern "C" fn js_string_append_chain(parts: *const f64, n: i32) -> *mut StringHeader {
    let n = (n as usize).min(CONCAT_CHAIN_MAX_PARTS);
    if n < 2 || parts.is_null() {
        return js_string_concat_chain(parts, n as i32);
    }
    if n <= 4 {
        append_chain_all_heap_strings::<4>(parts, n)
    } else if n <= 8 {
        append_chain_all_heap_strings::<8>(parts, n)
    } else {
        append_chain_all_heap_strings::<CONCAT_CHAIN_MAX_PARTS>(parts, n)
    }
}

/// All-heap-string fast path for [`js_string_append_chain`]. Falling back to
/// `js_string_concat_chain` preserves dynamic/SSO coercion and the `s + s`
/// overlap case without adding those branches to the hot copy loop.
fn append_chain_all_heap_strings<const MAX_PARTS: usize>(
    parts: *const f64,
    n: usize,
) -> *mut StringHeader {
    let mut piece_ptrs: [*const StringHeader; MAX_PARTS] = [std::ptr::null(); MAX_PARTS];
    let mut piece_lens: [u32; MAX_PARTS] = [0; MAX_PARTS];
    let mut total_blen = 0u32;
    let mut total_u16 = 0u32;
    let mut piece_flags = 0u32;

    for i in 0..n {
        let bits = unsafe { *parts.add(i) }.to_bits();
        if bits >> 48 != 0x7FFF {
            return js_string_concat_chain(parts, n as i32);
        }
        let piece = (bits & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader;
        if !is_valid_string_ptr(piece) || (i > 0 && piece == piece_ptrs[0]) {
            return js_string_concat_chain(parts, n as i32);
        }
        let blen = unsafe { (*piece).byte_len };
        piece_ptrs[i] = piece;
        piece_lens[i] = blen;
        total_blen = total_blen.saturating_add(blen);
        total_u16 = total_u16.saturating_add(unsafe { (*piece).utf16_len });
        piece_flags |= unsafe { (*piece).flags };
    }

    let dest = piece_ptrs[0] as *mut StringHeader;
    let dest_blen = piece_lens[0];
    let suffix_blen = total_blen.saturating_sub(dest_blen);
    if suffix_blen == 0 {
        return dest;
    }

    unsafe {
        if (*dest).refcount == 1 && total_blen <= (*dest).capacity {
            let mut cursor = (string_data(dest) as *mut u8).add(dest_blen as usize);
            for i in 1..n {
                let len = piece_lens[i] as usize;
                ptr::copy_nonoverlapping(string_data(piece_ptrs[i]), cursor, len);
                cursor = cursor.add(len);
            }
            (*dest).byte_len = total_blen;
            (*dest).utf16_len = total_u16;
            (*dest).flags |= piece_flags;
            return if piece_flags & STRING_FLAG_HAS_LONE_SURROGATES != 0 {
                canonicalize_surrogate_pairs(dest)
            } else {
                dest
            };
        }
    }

    // An empty one-frame accumulator keeps exact capacity: no RSS-for-speed
    // reserve. Once a non-empty accumulator grows, match js_string_append's
    // existing geometric capacity so later iterations remain amortized.
    let capacity = if dest_blen == 0 {
        total_blen
    } else {
        total_blen.saturating_mul(2).max(32)
    };

    if let Some((result, cursor)) = string_storage_alloc_no_collect(capacity) {
        return unsafe {
            init_string_header(result, total_u16, total_blen, capacity, 1, piece_flags);
            copy_heap_chain(&piece_ptrs, &piece_lens, n, cursor);
            if piece_flags & STRING_FLAG_HAS_LONE_SURROGATES != 0 {
                canonicalize_surrogate_pairs(result)
            } else {
                result
            }
        };
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let mut handles = [None; MAX_PARTS];
    for i in 0..n {
        handles[i] = Some(scope.root_string_ptr(piece_ptrs[i]));
    }
    let (result, mut cursor) = string_storage_alloc(capacity);
    unsafe {
        init_string_header(result, total_u16, total_blen, capacity, 1, piece_flags);
        for i in 0..n {
            let len = piece_lens[i] as usize;
            if len == 0 {
                continue;
            }
            handles[i]
                .expect("append-chain string handle")
                .with_const_ptr::<StringHeader, _>(|piece| {
                    ptr::copy_nonoverlapping(string_data(piece), cursor, len);
                });
            cursor = cursor.add(len);
        }
        if piece_flags & STRING_FLAG_HAS_LONE_SURROGATES != 0 {
            canonicalize_surrogate_pairs(result)
        } else {
            result
        }
    }
}

unsafe fn copy_heap_chain<const MAX_PARTS: usize>(
    piece_ptrs: &[*const StringHeader; MAX_PARTS],
    piece_lens: &[u32; MAX_PARTS],
    n: usize,
    mut cursor: *mut u8,
) {
    for i in 0..n {
        let len = piece_lens[i] as usize;
        if len == 0 {
            continue;
        }
        unsafe {
            ptr::copy_nonoverlapping(string_data(piece_ptrs[i]), cursor, len);
            cursor = cursor.add(len);
        }
    }
}

#[cfg(test)]
thread_local! {
/// #7912 counter: how many chains took the unrooted fast path below. A gate
/// that cannot see its subject run is not a gate — the unit tests assert this
/// moves, so a refactor that quietly stops taking the fast path is red rather
/// than "still correct, just slow again".
    pub(crate) static CONCAT_CHAIN_NO_COLLECT_HITS: std::cell::Cell<u64> =
        const { std::cell::Cell::new(0) };
}

#[cfg(test)]
#[inline]
fn note_no_collect_hit() {
    CONCAT_CHAIN_NO_COLLECT_HITS.with(|c| c.set(c.get() + 1));
}

#[cfg(not(test))]
#[inline(always)]
fn note_no_collect_hit() {}

/// The unrooted arm of [`concat_chain_sized`]: every part is already a live
/// heap string, so classification touches no allocator at all, and the one
/// allocation it needs is taken through the **no-collect** entry point.
///
/// ★ Why this is sound, stated as the invariant it rests on:
///
/// > `string_storage_alloc_no_collect` returns `Some` only when the request
/// > was served by bumping the nursery block that was already open. That is
/// > the one path through `arena_cell_alloc` that precedes `gc_check_trigger`,
/// > so a `Some` is a proof that no collection ran and therefore that nothing
/// > moved.
///
/// With that proof in hand the transient handle stack is not merely
/// unnecessary, it is unreachable work: the pointers read in the sizing loop
/// are still the pointers to copy from. `None` (block full, oversized result)
/// re-enters the rooted path below, which behaves exactly as it always did —
/// and re-reads its operands from `parts`, which is still valid because
/// nothing has collected *yet* either.
///
/// The rooted path costs ~2N thread-local + `RefCell` round trips per call
/// (N `root_string_ptr` pushes in the classify loop, N
/// `get_raw_const_ptr` reads in the copy loop) plus the scope's own two.
/// On Darwin every one of those is an `_tlv_get_addr` call. On a
/// tree-walking-interpreter workload whose environment lookup appends
/// `seen = seen + "[" + names[i] + "]"` per frame, that bookkeeping measured
/// **12.6%** of total run time — more than the concatenation it was
/// protecting.
#[inline]
fn concat_chain_all_heap_strings_no_collect<const MAX_PARTS: usize>(
    parts: *const f64,
    n: usize,
) -> Option<*mut StringHeader> {
    let mut piece_ptrs: [*const StringHeader; MAX_PARTS] = [std::ptr::null(); MAX_PARTS];
    let mut piece_lens: [u32; MAX_PARTS] = [0; MAX_PARTS];
    let mut piece_flags: u32 = 0;
    let mut total_blen: u32 = 0;
    let mut total_u16: u32 = 0;

    // Admission scan FIRST, and it touches nothing but `parts`. A chain with
    // a number, an SSO value or an object in it needs `js_jsvalue_to_string`,
    // which allocates, so it belongs on the rooted path — and it must reach
    // that path having paid only n register compares, not n cold
    // `StringHeader` loads it is about to throw away and redo.
    // STRING_TAG = 0x7FFF; `is_valid_string_ptr` is a range test, no deref.
    for i in 0..n {
        let bits = unsafe { *parts.add(i) }.to_bits();
        if bits >> 48 != 0x7FFF {
            return None;
        }
        let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader;
        if !is_valid_string_ptr(ptr) {
            return None;
        }
        piece_ptrs[i] = ptr;
    }

    for i in 0..n {
        // Mirrors the rooted loop exactly, including that an EMPTY part
        // contributes no flags: `piece_flags |= flags` sits inside its
        // `blen > 0` guard there, and a divergence here would be a
        // silent WTF-8 behaviour change rather than a slowdown.
        let ptr = piece_ptrs[i];
        let blen = unsafe { (*ptr).byte_len };
        if blen > 0 {
            piece_lens[i] = blen;
            piece_flags |= unsafe { (*ptr).flags };
            total_blen = total_blen.saturating_add(blen);
            total_u16 = total_u16.saturating_add(unsafe { (*ptr).utf16_len });
        }
    }

    let (ptr, mut cursor) = string_storage_alloc_no_collect(total_blen)?;
    note_no_collect_hit();

    unsafe {
        init_string_header(ptr, total_u16, total_blen, total_blen, 0, piece_flags);
        for i in 0..n {
            let l = piece_lens[i] as usize;
            if l == 0 {
                continue;
            }
            ptr::copy_nonoverlapping(string_data(piece_ptrs[i]), cursor, l);
            cursor = cursor.add(l);
        }
        Some(canonicalize_surrogate_pairs(ptr))
    }
}

/// The body of [`js_string_concat_chain`], monomorphised on the scratch-array
/// size. `0 < n <= MAX_PARTS` and `!parts.is_null()` are preconditions the
/// dispatcher establishes.
///
/// The `#7912` fast arm above answers first for an all-heap-string chain;
/// everything below is the original rooted path, reached when it declines.
fn concat_chain_sized<const MAX_PARTS: usize>(parts: *const f64, n: usize) -> *mut StringHeader {
    debug_assert!(n > 0 && n <= MAX_PARTS);
    if let Some(result) = concat_chain_all_heap_strings_no_collect::<MAX_PARTS>(parts, n) {
        return result;
    }
    // Per-part scratch buffer for number formatting. 32 bytes is enough
    // for any f64 string representation (max ~24 chars). Left UNINITIALISED:
    // a slot becomes readable only via `MaybeUninit::write`, on exactly the
    // two numeric arms, which are also the only arms that publish a
    // `piece_ptrs[i]` into it — so the copy loop can never read an
    // uninitialised slot.
    let mut num_bufs: [core::mem::MaybeUninit<[u8; 32]>; MAX_PARTS] =
        [core::mem::MaybeUninit::uninit(); MAX_PARTS];
    // For each part: (ptr, len, flags). ptr is either a pointer into
    // num_bufs[i] (numeric path) or null for a rooted string handle;
    // len is the byte count; flags carries STRING_FLAG_HAS_LONE_SURROGATES
    // if the part is a string with that flag set.
    let scope = crate::gc::RuntimeHandleScope::new();
    let mut piece_string_handles = [None; MAX_PARTS];
    let mut piece_ptrs: [*const u8; MAX_PARTS] = [std::ptr::null(); MAX_PARTS];
    let mut piece_lens: [u32; MAX_PARTS] = [0; MAX_PARTS];
    let mut piece_u16: [u32; MAX_PARTS] = [0; MAX_PARTS];
    let mut piece_flags: u32 = 0;
    let mut total_blen: u32 = 0;
    let mut total_u16: u32 = 0;

    // Slow-path string headers from js_jsvalue_to_string (need to keep
    // the StringHeader alive for the duration; arena strings stay live
    // since the GC won't run mid-FFI-call, and we won't trigger more
    // allocations between formatting and copying).
    for i in 0..n {
        let value = unsafe { *parts.add(i) };
        let bits = value.to_bits();
        let tag = bits >> 48;

        // STRING_TAG = 0x7FFF — heap string pointer in lower 48 bits.
        if tag == 0x7FFF {
            let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader;
            if is_valid_string_ptr(ptr) {
                let blen = unsafe { (*ptr).byte_len };
                let u16len = unsafe { (*ptr).utf16_len };
                let flags = unsafe { (*ptr).flags };
                if blen > 0 {
                    piece_string_handles[i] = Some(scope.root_string_ptr(ptr));
                    piece_lens[i] = blen;
                    piece_u16[i] = u16len;
                    piece_flags |= flags;
                    total_blen = total_blen.saturating_add(blen);
                    total_u16 = total_u16.saturating_add(u16len);
                }
                continue;
            }
        }

        // SHORT_STRING_TAG = 0x7FF9 — payload encoded inline. Materialize
        // through the slow path (rare in hot loops).
        if tag == 0x7FF9 {
            let s = crate::value::js_jsvalue_to_string(value);
            if is_valid_string_ptr(s) {
                let blen = unsafe { (*s).byte_len };
                let u16len = unsafe { (*s).utf16_len };
                let flags = unsafe { (*s).flags };
                if blen > 0 {
                    piece_string_handles[i] = Some(scope.root_string_ptr(s));
                    piece_lens[i] = blen;
                    piece_u16[i] = u16len;
                    piece_flags |= flags;
                    total_blen = total_blen.saturating_add(blen);
                    total_u16 = total_u16.saturating_add(u16len);
                }
            }
            continue;
        }

        // Plain f64 (no NaN-box tag in upper 16 bits). Format inline.
        let is_plain_f64 = tag < 0x7FF8 || (tag == 0x7FF8 && (bits & 0x000F_FFFF_FFFF_FFFF) == 0);
        if is_plain_f64 {
            let len = format_number_into(value, num_bufs[i].write([0u8; 32]));
            piece_ptrs[i] = num_bufs[i].as_ptr() as *const u8;
            piece_lens[i] = len as u32;
            piece_u16[i] = len as u32; // ASCII for all formatted numbers
            total_blen = total_blen.saturating_add(len as u32);
            total_u16 = total_u16.saturating_add(len as u32);
            continue;
        }

        // INT32_TAG = 0x7FFE — extract int from lower 32 bits. A registered
        // class id (Expr::ClassRef) stringifies via the slow path so it
        // renders as function source, not its numeric id.
        if tag == 0x7FFE && !crate::object::is_class_id_registered((bits & 0xFFFF_FFFF) as u32) {
            let v = (bits & 0xFFFF_FFFF) as u32 as i32;
            let len = {
                let buf = num_bufs[i].write([0u8; 32]);
                if v >= 0 {
                    fast_itoa_u32(v as u32, buf)
                } else {
                    let s = format!("{}", v);
                    let l = s.len().min(32);
                    buf[..l].copy_from_slice(&s.as_bytes()[..l]);
                    l
                }
            };
            piece_ptrs[i] = num_bufs[i].as_ptr() as *const u8;
            piece_lens[i] = len as u32;
            piece_u16[i] = len as u32;
            total_blen = total_blen.saturating_add(len as u32);
            total_u16 = total_u16.saturating_add(len as u32);
            continue;
        }

        // Anything else (bool, null, undefined, object, etc.) — slow path.
        let s = crate::value::js_jsvalue_to_string(value);
        if is_valid_string_ptr(s) {
            let blen = unsafe { (*s).byte_len };
            let u16len = unsafe { (*s).utf16_len };
            let flags = unsafe { (*s).flags };
            if blen > 0 {
                piece_string_handles[i] = Some(scope.root_string_ptr(s));
                piece_lens[i] = blen;
                piece_u16[i] = u16len;
                piece_flags |= flags;
                total_blen = total_blen.saturating_add(blen);
                total_u16 = total_u16.saturating_add(u16len);
            }
        }
    }

    // Single allocation for the entire result.
    let (ptr, mut cursor) = string_storage_alloc(total_blen);

    unsafe {
        init_string_header(ptr, total_u16, total_blen, total_blen, 0, piece_flags);
        for i in 0..n {
            let l = piece_lens[i] as usize;
            if l == 0 {
                continue;
            }
            if let Some(handle) = piece_string_handles[i] {
                let piece = handle.get_raw_const_ptr::<StringHeader>();
                if is_valid_string_ptr(piece) {
                    ptr::copy_nonoverlapping(string_data(piece), cursor, l);
                    cursor = cursor.add(l);
                }
            } else if !piece_ptrs[i].is_null() {
                ptr::copy_nonoverlapping(piece_ptrs[i], cursor, l);
                cursor = cursor.add(l);
            }
        }

        canonicalize_surrogate_pairs(ptr)
    }
}

/// Format an f64 into a 32-byte stack buffer using the fast paths from
/// `js_string_concat_value` / `js_value_concat_string`. Returns the number
/// of bytes written.
#[inline]
pub(crate) fn format_number_into(value: f64, buf: &mut [u8; 32]) -> usize {
    if value.fract() == 0.0 && value.abs() < 1e15 && !value.is_nan() && !value.is_infinite() {
        let n = value as i64;
        if (0..=999_999_999).contains(&n) {
            return fast_itoa_u32(n as u32, buf);
        }
        let s = format!("{}", n);
        let len = s.len().min(buf.len());
        buf[..len].copy_from_slice(&s.as_bytes()[..len]);
        return len;
    }
    if value.is_nan() {
        buf[..3].copy_from_slice(b"NaN");
        return 3;
    }
    if value.is_infinite() {
        if value > 0.0 {
            buf[..8].copy_from_slice(b"Infinity");
            return 8;
        }
        buf[..9].copy_from_slice(b"-Infinity");
        return 9;
    }
    if value == 0.0 {
        buf[0] = b'0';
        return 1;
    }
    // #3987: match ECMAScript NumberToString (scientific notation for
    // |n| >= 1e21 / < 1e-6) instead of Rust's full-decimal `{}`.
    let s = super::format::js_format_f64(value);
    let len = s.len().min(buf.len());
    buf[..len].copy_from_slice(&s.as_bytes()[..len]);
    len
}

/// Fused value + string concatenation (value on the LEFT, string on the RIGHT).
/// Handles the `i + "_suffix"` pattern.
#[no_mangle]
pub extern "C" fn js_value_concat_string(
    value: f64,
    suffix: *const StringHeader,
) -> *mut StringHeader {
    // #6655: mirror of `js_string_concat_value` — `suffix` is a raw movable
    // heap pointer held across `string_storage_alloc` (fast path) and across
    // `js_jsvalue_to_string(value)`'s user `toString` (slow path).
    let scope = crate::gc::RuntimeHandleScope::new();
    let suffix_handle = scope.root_string_ptr(suffix);
    let suffix_blen = if is_valid_string_ptr(suffix) {
        unsafe { (*suffix).byte_len }
    } else {
        0
    };
    let suffix_u16 = if is_valid_string_ptr(suffix) {
        unsafe { (*suffix).utf16_len }
    } else {
        0
    };

    let bits = value.to_bits();
    let tag = bits >> 48;
    let is_plain_f64 = tag < 0x7FF8 || (tag == 0x7FF8 && (bits & 0x000F_FFFF_FFFF_FFFF) == 0);

    if is_plain_f64 {
        let mut num_buf = [0u8; 32];
        let num_len: usize;

        if value.fract() == 0.0 && value.abs() < 1e15 && !value.is_nan() && !value.is_infinite() {
            let n = value as i64;
            if (0..=999_999_999).contains(&n) {
                num_len = fast_itoa_u32(n as u32, &mut num_buf);
            } else {
                let s = format!("{}", n);
                let len = s.len().min(num_buf.len());
                num_buf[..len].copy_from_slice(&s.as_bytes()[..len]);
                num_len = len;
            }
        } else if value.is_nan() {
            num_buf[..3].copy_from_slice(b"NaN");
            num_len = 3;
        } else if value.is_infinite() {
            if value > 0.0 {
                num_buf[..8].copy_from_slice(b"Infinity");
                num_len = 8;
            } else {
                num_buf[..9].copy_from_slice(b"-Infinity");
                num_len = 9;
            }
        } else if value == 0.0 {
            num_buf[0] = b'0';
            num_len = 1;
        } else {
            // #3987: match ECMAScript NumberToString (scientific notation for
            // |n| >= 1e21 / < 1e-6) instead of Rust's full-decimal `{}`.
            let s = super::format::js_format_f64(value);
            let len = s.len().min(num_buf.len());
            num_buf[..len].copy_from_slice(&s.as_bytes()[..len]);
            num_len = len;
        }

        let total_blen = num_len + suffix_blen as usize;
        let (ptr, data_ptr) = string_storage_alloc(total_blen as u32);
        // Re-read after the allocation: it can collect and evacuate (#6655).
        let suffix = suffix_handle.get_raw_const_ptr::<StringHeader>();

        unsafe {
            let flags = if is_valid_string_ptr(suffix) {
                (*suffix).flags
            } else {
                0
            };
            init_string_header(
                ptr,
                num_len as u32 + suffix_u16,
                total_blen as u32,
                total_blen as u32,
                0,
                flags,
            );

            ptr::copy_nonoverlapping(num_buf.as_ptr(), data_ptr, num_len);
            if is_valid_string_ptr(suffix) && suffix_blen > 0 {
                ptr::copy_nonoverlapping(
                    string_data(suffix),
                    data_ptr.add(num_len),
                    suffix_blen as usize,
                );
            }
        }

        return ptr;
    }

    // Reload `suffix` after the user `toString` (#6655).
    let value_str = crate::value::js_jsvalue_to_string(value);
    js_string_concat(value_str, suffix_handle.get_raw_const_ptr::<StringHeader>())
}

/// Resolve a value the caller has already established `is_any_string()` to a
/// raw `StringHeader*`, materialising SSO bits exactly the way the codegen's
/// `unbox_str_handle` does.
///
/// The caller must pass the result straight into a helper that roots it —
/// nothing may allocate between the SSO materialisation and that root.
#[inline]
fn string_handle_of(value: f64) -> *const StringHeader {
    let jsval = crate::value::JSValue::from_bits(value.to_bits());
    if jsval.is_string() {
        return jsval.as_string_ptr();
    }
    crate::value::js_get_string_pointer_unified(value) as *const StringHeader
}

/// `l + r` where the codegen's only evidence that `l` is a string is a
/// DECLARED type (or a receiver-blind method-name guess) — #7837 defect 1.
///
/// The fused `js_string_concat_value` cannot make this decision itself,
/// because codegen hands it an already-unboxed `StringHeader*` and the tag is
/// gone by then. So a declared-only operand is passed NaN-BOXED instead, and
/// the operator is chosen from the bits:
///
/// * `l` really is a string → the identical fused single-allocation concat the
///   strict path emits, so an honest program pays one predictable compare
///   inside a call it was already making, and no codegen diamond at all;
/// * `l` is anything else → the spec's `+`, which is what
///   `const s: string = (42 as any); s + 7` must answer (`49`, not `"427"`).
///
/// See [`js_value_add_string`] for the mirrored operand order.
#[no_mangle]
pub unsafe extern "C" fn js_string_add_value(l_value: f64, r_value: f64) -> f64 {
    if crate::value::JSValue::from_bits(l_value.to_bits()).is_any_string() {
        let handle = string_handle_of(l_value);
        let out = js_string_concat_value(handle, r_value);
        return crate::value::js_nanbox_string(out as i64);
    }
    crate::value::js_dynamic_string_or_number_add(l_value, r_value)
}

/// `l + r` where the declared-only string operand is on the RIGHT — the mirror
/// of [`js_string_add_value`], guarding `js_value_concat_string` the same way.
#[no_mangle]
pub unsafe extern "C" fn js_value_add_string(l_value: f64, r_value: f64) -> f64 {
    if crate::value::JSValue::from_bits(r_value.to_bits()).is_any_string() {
        let handle = string_handle_of(r_value);
        let out = js_value_concat_string(l_value, handle);
        return crate::value::js_nanbox_string(out as i64);
    }
    crate::value::js_dynamic_string_or_number_add(l_value, r_value)
}

/// Fast integer-to-ASCII formatting into a provided buffer.
/// Returns the number of bytes written. Digits are written to the END
/// of the buffer and then shifted to the front.
#[inline]
pub(crate) fn fast_itoa_u32(mut n: u32, buf: &mut [u8; 32]) -> usize {
    if n == 0 {
        buf[0] = b'0';
        return 1;
    }
    // Size first, then write digits in place back-to-front. The old
    // write-at-the-end-then-`copy_within` shape paid a libc `memmove` PLT
    // call per conversion (runtime-length overlapping copy — it showed up
    // as ~15% of the `"id-" + i` loop in `sample`, inlined into both
    // concat entry points).
    let len = n.ilog10() as usize + 1;
    let mut pos = len;
    while n > 0 {
        pos -= 1;
        buf[pos] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    len
}
