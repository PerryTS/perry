//! Literal-aware concatenation (JST slice 3, design B).
//!
//! `"item-" + (i + r) + ";"` used to hand `js_string_concat_chain` a heap
//! `StringHeader` for each literal and let it re-derive everything about them
//! (tag dispatch, header loads, a failed all-heap attempt, a root per literal).
//! Codegen already knows the literal bytes, their length and their UTF-16
//! length, and the bytes are constants in the program's rodata. This module
//! takes them as operands of that form: `(ptr, byte_len | utf16_len << 32)`
//! next to ordinary value operands, so the work per call is classification of
//! the value operands, one allocation and the copies.
//!
//! Nothing here is a cache or a table. `js_string_concat_lit_value_box` is the
//! two-operand `"literal" + number` form and consults the process concat memo
//! exactly where `concat_formatted_number` does.

use super::concat::{
    canonicalize_surrogate_pairs, concat_memo_admit, concat_memo_insert, concat_memo_lookup,
    concat_memo_note_hit, concat_memo_should_probe, concat_memo_slot_and_tag, copy_bytes_small,
    decimal_len, fast_itoa_i64, fast_itoa_u32, format_number_into, js_string_concat, write_decimal,
    CONCAT_MEMO_MAX_BYTES,
};
use super::*;

/// One operand slot as codegen writes it. A value operand is
/// `{ NaN-boxed bits, CONCAT_PART_VALUE }`. A literal operand is
/// `{ address of its rodata bytes, byte_len | utf16_len << 32 }`; codegen
/// admits only source literals, which are valid UTF-8 with no lone surrogate.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ConcatPartSlot {
    pub word: u64,
    pub meta: u64,
}

/// `meta` of a value operand. A literal's `meta` carries a UTF-16 length below
/// 2^31 in its high word, so it can never equal this.
pub const CONCAT_PART_VALUE: u64 = u64::MAX;

const MAX_PARTS: usize = 32;

#[inline(always)]
fn lit_blen(meta: u64) -> u32 {
    meta as u32
}

#[inline(always)]
fn lit_units(meta: u64) -> u32 {
    ((meta >> 32) & 0x7FFF_FFFF) as u32
}

/// `js_string_concat_lit_value_box` only: the literal's own header would pass
/// `concat_formatted_number`'s memo admission. Today a literal that lives in
/// the program image carries static header flags (`perry_image_literal_*`),
/// which fail that test, so the memo is consulted for a literal prefix only in
/// a build whose literals are created at run time. Codegen knows which build it
/// is emitting and sets this bit accordingly; the runtime does not guess.
pub const CONCAT_LIT_MEMO_ELIGIBLE: u64 = 1 << 63;

/// N-way concatenation whose literal operands are constant data.
///
/// Returns a fresh shared (`refcount == 0`) heap string, like
/// `js_string_concat_chain`. Value operands that are heap strings, inline
/// strings or numbers stay on the fast path; any other kind (a class id,
/// `undefined`, a BigInt, ...) takes the general arm, which converts it the way
/// the chain's general arm does.
#[no_mangle]
pub extern "C" fn js_string_concat_parts(
    parts: *const ConcatPartSlot,
    n: i32,
) -> *mut StringHeader {
    let n = (n.max(0) as usize).min(MAX_PARTS);
    if n == 0 || parts.is_null() {
        return crate::string::js_string_from_bytes(b"".as_ptr(), 0);
    }
    if n <= 4 {
        concat_parts_sized::<4>(parts, n)
    } else if n <= 8 {
        concat_parts_sized::<8>(parts, n)
    } else {
        concat_parts_sized::<MAX_PARTS>(parts, n)
    }
}

#[inline]
fn concat_parts_sized<const M: usize>(parts: *const ConcatPartSlot, n: usize) -> *mut StringHeader {
    if let Some(result) = concat_parts_simple::<M>(parts, n) {
        return result;
    }
    concat_parts_general::<M>(parts, n)
}

/// A value operand that is an exact `u32` is formatted straight into the
/// result: its digit count sizes the allocation and its digits are written at
/// the copy position, so it needs neither a scratch buffer nor a second copy.
#[inline(always)]
fn exact_u32(value: f64) -> Option<u32> {
    if (0.0..=u32::MAX as f64).contains(&value) {
        // SAFETY: the range test above admits only finite values in
        // `0..=u32::MAX`, which an `i64` holds exactly.
        let n = unsafe { value.to_int_unchecked::<i64>() };
        if n as f64 == value {
            return Some(n as u32);
        }
    }
    None
}

/// Literals, heap strings, inline strings and numbers: no user code, and no
/// allocation before the single result allocation.
#[inline]
fn concat_parts_simple<const M: usize>(
    parts: *const ConcatPartSlot,
    n: usize,
) -> Option<*mut StringHeader> {
    let mut scratch: [core::mem::MaybeUninit<[u8; 32]>; M] = [core::mem::MaybeUninit::uninit(); M];
    // `piece_ptrs[i]` is null for a part written as digits of `digits[i]`.
    let mut piece_ptrs: [*const u8; M] = [ptr::null(); M];
    let mut piece_lens: [u32; M] = [0; M];
    let mut digits: [u32; M] = [0; M];
    let mut flags = STRING_FLAG_WTF8_VALIDATED;
    // 32 parts of at most `u32::MAX` bytes cannot overflow a u64; the sum is
    // checked once, below.
    let mut total_blen: u64 = 0;
    let mut total_u16: u64 = 0;
    for i in 0..n {
        let slot = unsafe { *parts.add(i) };
        if slot.meta != CONCAT_PART_VALUE {
            let blen = lit_blen(slot.meta);
            piece_ptrs[i] = slot.word as *const u8;
            piece_lens[i] = blen;
            total_blen += blen as u64;
            total_u16 += lit_units(slot.meta) as u64;
            continue;
        }
        let bits = slot.word;
        if bits >> 48 == 0x7FFF {
            let header = (bits & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader;
            if !is_valid_string_ptr(header) {
                return None;
            }
            let blen = unsafe { (*header).byte_len };
            // An empty operand contributes neither bytes nor provenance.
            if blen == 0 {
                continue;
            }
            piece_ptrs[i] = string_data(header);
            piece_lens[i] = blen;
            flags = combine_string_flags(flags, unsafe { (*header).flags });
            total_blen += blen as u64;
            total_u16 += unsafe { (*header).utf16_len } as u64;
            continue;
        }
        let value = crate::value::JSValue::from_bits(bits);
        if value.is_number() {
            let number = f64::from_bits(bits);
            let len = if let Some(n) = exact_u32(number) {
                digits[i] = n;
                decimal_len(n)
            } else {
                let buf = scratch[i].write([0u8; 32]);
                let len = format_number_into(number, buf);
                piece_ptrs[i] = buf.as_ptr();
                len
            };
            piece_lens[i] = len as u32;
            total_blen += len as u64;
            total_u16 += len as u64; // ASCII
            continue;
        }
        if value.is_short_string() {
            let buf = scratch[i].write([0u8; 32]);
            let mut bytes = [0u8; crate::value::SHORT_STRING_MAX_LEN];
            let len = value.short_string_to_buf(&mut bytes);
            if len == 0 {
                continue;
            }
            buf[..len].copy_from_slice(&bytes[..len]);
            let (units, part_flags) = raw_string_metadata(buf.as_ptr(), len as u32);
            flags = combine_string_flags(flags, part_flags);
            piece_ptrs[i] = buf.as_ptr();
            piece_lens[i] = len as u32;
            total_blen += len as u64;
            total_u16 += units as u64;
            continue;
        }
        return None;
    }
    if total_blen > u32::MAX as u64 || total_u16 > u32::MAX as u64 {
        return None;
    }
    let (total_blen, total_u16) = (total_blen as u32, total_u16 as u32);

    // `Some` proves nothing moved, so the borrowed heap payloads are valid.
    let (result, mut cursor) = match string_storage_alloc_no_collect(total_blen) {
        Some(allocated) => allocated,
        None => {
            return Some(concat_parts_rooted_copy::<M>(
                parts,
                n,
                &mut piece_ptrs,
                &piece_lens,
                &digits,
                total_blen,
                total_u16,
                flags,
            ))
        }
    };
    unsafe {
        init_string_header(result, total_u16, total_blen, total_blen, 0, flags);
        for i in 0..n {
            let len = piece_lens[i] as usize;
            // An empty operand has a null pointer too: it writes nothing.
            if len == 0 {
                continue;
            }
            if piece_ptrs[i].is_null() {
                write_decimal(cursor, digits[i], len);
            } else {
                // A literal or a number is a handful of bytes: keep them out
                // of libc's memcpy. Long heap payloads still take it.
                copy_bytes_small(piece_ptrs[i], cursor, len);
            }
            cursor = cursor.add(len);
        }
    }
    // Only a heap or inline operand can carry a lone surrogate, and only a
    // join of two such operands can mint a pair.
    if flags & STRING_FLAG_HAS_LONE_SURROGATES != 0 {
        Some(canonicalize_surrogate_pairs(result))
    } else {
        Some(result)
    }
}

/// The block boundary refused the no-collect allocation: root every heap
/// operand, take the collecting allocation, and re-read each from its handle.
/// Literals, digits and the stack conversions do not move.
#[cold]
#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn concat_parts_rooted_copy<const M: usize>(
    parts: *const ConcatPartSlot,
    n: usize,
    piece_ptrs: &mut [*const u8; M],
    piece_lens: &[u32; M],
    digits: &[u32; M],
    total_blen: u32,
    total_u16: u32,
    flags: u32,
) -> *mut StringHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let mut handles = [None; M];
    for i in 0..n {
        let slot = unsafe { *parts.add(i) };
        if slot.meta == CONCAT_PART_VALUE && slot.word >> 48 == 0x7FFF && piece_lens[i] != 0 {
            handles[i] = Some(
                scope.root_string_ptr((slot.word & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader),
            );
        }
    }
    let (result, mut cursor) = string_storage_alloc(total_blen);
    unsafe {
        init_string_header(result, total_u16, total_blen, total_blen, 0, flags);
        for i in 0..n {
            let len = piece_lens[i] as usize;
            if len == 0 {
                continue;
            }
            if let Some(handle) = handles[i] {
                piece_ptrs[i] = string_data(handle.get_raw_const_ptr::<StringHeader>());
            }
            if piece_ptrs[i].is_null() {
                write_decimal(cursor, digits[i], len);
            } else {
                ptr::copy_nonoverlapping(piece_ptrs[i], cursor, len);
            }
            cursor = cursor.add(len);
        }
    }
    canonicalize_surrogate_pairs(result)
}

/// Every other value kind. The converted text of a value operand is a rooted
/// heap string; a literal or a number is borrowed from rodata or the stack.
#[cold]
#[inline(never)]
fn concat_parts_general<const M: usize>(
    parts: *const ConcatPartSlot,
    n: usize,
) -> *mut StringHeader {
    let mut num_bufs: [core::mem::MaybeUninit<[u8; 32]>; M] = [core::mem::MaybeUninit::uninit(); M];
    let scope = crate::gc::RuntimeHandleScope::new();
    let mut handles = [None; M];
    let mut piece_ptrs: [*const u8; M] = [ptr::null(); M];
    let mut piece_lens: [u32; M] = [0; M];
    let mut flags = STRING_FLAG_WTF8_VALIDATED;
    let mut total_blen: u32 = 0;
    let mut total_u16: u32 = 0;

    for i in 0..n {
        let slot = unsafe { *parts.add(i) };
        if slot.meta != CONCAT_PART_VALUE {
            let blen = lit_blen(slot.meta);
            piece_ptrs[i] = slot.word as *const u8;
            piece_lens[i] = blen;
            total_blen = total_blen.saturating_add(blen);
            total_u16 = total_u16.saturating_add(lit_units(slot.meta));
            continue;
        }
        let value = f64::from_bits(slot.word);
        let bits = slot.word;
        let tag = bits >> 48;

        if tag == 0x7FFF {
            let header = (bits & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader;
            if is_valid_string_ptr(header) {
                let blen = unsafe { (*header).byte_len };
                if blen > 0 {
                    handles[i] = Some(scope.root_string_ptr(header));
                    piece_lens[i] = blen;
                    flags = combine_string_flags(flags, unsafe { (*header).flags });
                    total_blen = total_blen.saturating_add(blen);
                    total_u16 = total_u16.saturating_add(unsafe { (*header).utf16_len });
                }
                continue;
            }
        }
        if crate::value::JSValue::from_bits(bits).is_number() {
            let len = format_number_into(value, num_bufs[i].write([0u8; 32]));
            piece_ptrs[i] = num_bufs[i].as_ptr() as *const u8;
            piece_lens[i] = len as u32;
            total_blen = total_blen.saturating_add(len as u32);
            total_u16 = total_u16.saturating_add(len as u32);
            continue;
        }
        // A registered class id stringifies as its function source, so only an
        // ordinary INT32 takes the digit arm.
        if tag == 0x7FFE && !crate::object::is_class_id_registered((bits & 0xFFFF_FFFF) as u32) {
            let v = (bits & 0xFFFF_FFFF) as u32 as i32;
            let len = {
                let buf = num_bufs[i].write([0u8; 32]);
                if v >= 0 {
                    fast_itoa_u32(v as u32, buf)
                } else {
                    fast_itoa_i64(v as i64, buf)
                }
            };
            piece_ptrs[i] = num_bufs[i].as_ptr() as *const u8;
            piece_lens[i] = len as u32;
            total_blen = total_blen.saturating_add(len as u32);
            total_u16 = total_u16.saturating_add(len as u32);
            continue;
        }
        // Inline strings, class ids, undefined, null, booleans, BigInts,
        // objects: the shared ToString.
        let converted = crate::value::js_jsvalue_to_string(value);
        if is_valid_string_ptr(converted) {
            let blen = unsafe { (*converted).byte_len };
            if blen > 0 {
                handles[i] = Some(scope.root_string_ptr(converted));
                piece_lens[i] = blen;
                flags = combine_string_flags(flags, unsafe { (*converted).flags });
                total_blen = total_blen.saturating_add(blen);
                total_u16 = total_u16.saturating_add(unsafe { (*converted).utf16_len });
            }
        }
    }

    let (result, mut cursor) = string_storage_alloc(total_blen);
    unsafe {
        init_string_header(result, total_u16, total_blen, total_blen, 0, flags);
        for i in 0..n {
            let len = piece_lens[i] as usize;
            if len == 0 {
                continue;
            }
            if let Some(handle) = handles[i] {
                let piece = handle.get_raw_const_ptr::<StringHeader>();
                if is_valid_string_ptr(piece) {
                    ptr::copy_nonoverlapping(string_data(piece), cursor, len);
                    cursor = cursor.add(len);
                }
            } else if !piece_ptrs[i].is_null() {
                ptr::copy_nonoverlapping(piece_ptrs[i], cursor, len);
                cursor = cursor.add(len);
            }
        }
    }
    canonicalize_surrogate_pairs(result)
}

/// NaN-box-returning `"literal" + value` for a non-empty ASCII source literal,
/// the literal-operand twin of `js_string_concat_value_box`: the same answers
/// (SSO immediate for an exact-u32 result of at most five ASCII bytes, heap
/// `STRING_TAG` otherwise), the same memo consultation, minus the per-call
/// validation of a prefix codegen already knows. The literal is ASCII, so
/// `meta`'s UTF-16 length equals its byte length; bit 63 is
/// [`CONCAT_LIT_MEMO_ELIGIBLE`].
#[no_mangle]
pub extern "C" fn js_string_concat_lit_value_box(lit: *const u8, meta: u64, value: f64) -> f64 {
    let prefix_blen = lit_blen(meta) as usize;
    let bits = value.to_bits();
    if crate::value::JSValue::from_bits(bits).is_number() {
        let mut num_buf = [0u8; 32];
        let exact_u32 = (0.0..=u32::MAX as f64).contains(&value) && (value as u32) as f64 == value;
        let num_len = if exact_u32 {
            fast_itoa_u32(value as u32, &mut num_buf)
        } else {
            format_number_into(value, &mut num_buf)
        };
        // Same admission as `js_string_concat_value_box`: only an exact u32
        // can become an inline string.
        if exact_u32
            && prefix_blen < crate::value::SHORT_STRING_MAX_LEN
            && prefix_blen + num_len <= crate::value::SHORT_STRING_MAX_LEN
        {
            let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
            unsafe {
                copy_bytes_small(lit, sso.as_mut_ptr(), prefix_blen);
                copy_bytes_small(num_buf.as_ptr(), sso.as_mut_ptr().add(prefix_blen), num_len);
            }
            return f64::from_bits(
                crate::value::JSValue::short_string_unchecked(&sso[..prefix_blen + num_len]).bits(),
            );
        }
        let ptr = concat_literal_formatted_number(lit, meta, &num_buf, num_len);
        return f64::from_bits(crate::value::js_nanbox_string(ptr as i64).to_bits());
    }
    // Not a number: ToString the value first (it may run user code and
    // collect), root that text, then build the prefix string and join.
    let value_str = crate::value::js_jsvalue_to_string(value);
    let scope = crate::gc::RuntimeHandleScope::new();
    let value_handle = scope.root_string_ptr(value_str);
    let prefix = crate::string::js_string_from_bytes_known_utf16(
        lit,
        prefix_blen as u32,
        lit_units(meta),
        STRING_FLAG_WTF8_VALIDATED,
    );
    let joined = js_string_concat(prefix, value_handle.get_raw_const_ptr::<StringHeader>());
    f64::from_bits(crate::value::js_nanbox_string(joined as i64).to_bits())
}

/// `concat_formatted_number` for a literal prefix: the identical memo
/// protocol, with the prefix facts (ASCII, no surrogates, length) taken from
/// codegen instead of validated per call.
fn concat_literal_formatted_number(
    lit: *const u8,
    meta: u64,
    num_buf: &[u8; 32],
    num_len: usize,
) -> *mut StringHeader {
    let prefix_blen = lit_blen(meta) as usize;
    let total_blen = prefix_blen + num_len;
    let memoizable = meta & CONCAT_LIT_MEMO_ELIGIBLE != 0
        && total_blen <= CONCAT_MEMO_MAX_BYTES as usize
        && concat_memo_should_probe();
    let mut memo_buf = [0u8; CONCAT_MEMO_MAX_BYTES as usize];
    let mut memo_slot = 0usize;
    let mut memo_admitted = false;
    if memoizable {
        unsafe {
            copy_bytes_small(lit, memo_buf.as_mut_ptr(), prefix_blen);
            copy_bytes_small(
                num_buf.as_ptr(),
                memo_buf.as_mut_ptr().add(prefix_blen),
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
        memo_admitted = concat_memo_admit(memo_slot, tag);
    }
    // Prefer the open nursery block; a refusal needs no root because the
    // prefix is rodata and the digits are on the stack.
    let (result, data) = match string_storage_alloc_no_collect(total_blen as u32) {
        Some(allocated) => allocated,
        None => string_storage_alloc(total_blen as u32),
    };
    unsafe {
        // `concat_formatted_number` copies the prefix header's flags; a source
        // literal's are the validated proof alone.
        init_string_header(
            result,
            total_blen as u32,
            total_blen as u32,
            total_blen as u32,
            0,
            STRING_FLAG_WTF8_VALIDATED,
        );
        copy_bytes_small(lit, data, prefix_blen);
        copy_bytes_small(num_buf.as_ptr(), data.add(prefix_blen), num_len);
    }
    if memoizable && memo_admitted {
        concat_memo_insert(memo_slot, result);
    }
    result
}

#[cfg(test)]
#[path = "concat_parts_tests.rs"]
mod tests;
