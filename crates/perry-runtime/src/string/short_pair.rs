//! Two-part concatenation of short ASCII strings into an inline (SSO) result.
//!
//! `a + b` where both operands are at most five ASCII bytes in total is the
//! hottest string shape in key and name building: `"abcdefgh".charAt(f) +
//! String(r + 1)`, `from + to`, `uci + promotion`. The general pairwise path
//! ([`super::concat::js_string_concat_box`]) unpacks each operand into a
//! scratch buffer byte by byte, scans it for ASCII byte by byte, and packs the
//! bytes back into an inline value: about 180 instructions for a result that
//! is one `or` of two shifted payloads.
//!
//! This arm builds the result from the operands' bits directly. It answers
//! only where the general path would produce an inline result, and produces
//! the same bits:
//! - the general path inlines exactly when both operands are ASCII and the
//!   total is at most `SHORT_STRING_MAX_LEN` bytes, with operand `a`'s bytes
//!   first; so does this arm;
//! - an operand is admitted only when it is an inline string, or a heap string
//!   of at most five bytes whose header already proves it ASCII
//!   (`utf16_len == byte_len` and `STRING_FLAG_WTF8_VALIDATED`, the same proof
//!   `str_bytes_ascii_from_jsvalue` accepts without a scan);
//! - everything else (a non-ASCII byte, a longer result, a heap string without
//!   the proof, a non-string) answers `None` and the caller takes its general
//!   path unchanged.
//!
//! No allocation, no collection point, no rooting: the result is an
//! immediate and the operands are only read.
//!
//! Switch: `PERRY_STRING_FAST_PATHS=0` (also `off`, `false`, `no`) turns the
//! arm off at run time; every concatenation then takes the general path. The
//! variable is read once, at the first concatenation that asks (no startup
//! cost).

use std::sync::atomic::{AtomicU8, Ordering};

use super::{StringHeader, STRING_FLAG_WTF8_VALIDATED};
use crate::value::{
    POINTER_MASK, SHORT_STRING_LEN_SHIFT, SHORT_STRING_MAX_LEN, SHORT_STRING_TAG, STRING_TAG,
    TAG_MASK,
};

/// The high bit of each of the five payload bytes: an inline payload with any
/// of them set holds a non-ASCII byte.
const PAYLOAD_HIGH_BITS: u64 = 0x0000_0080_8080_8080;

const SWITCH_UNRESOLVED: u8 = 0;
const SWITCH_ON: u8 = 1;
const SWITCH_OFF: u8 = 2;

static SWITCH: AtomicU8 = AtomicU8::new(SWITCH_UNRESOLVED);

/// Whether the short-pair arm is on (`PERRY_STRING_FAST_PATHS`, default on).
#[inline(always)]
pub(crate) fn short_pairs_enabled() -> bool {
    match SWITCH.load(Ordering::Relaxed) {
        SWITCH_ON => true,
        SWITCH_OFF => false,
        _ => resolve_switch(),
    }
}

#[cold]
#[inline(never)]
fn resolve_switch() -> bool {
    let on = switch_from_value(std::env::var("PERRY_STRING_FAST_PATHS").ok().as_deref());
    probe::arm_from_env();
    SWITCH.store(if on { SWITCH_ON } else { SWITCH_OFF }, Ordering::Relaxed);
    on
}

/// Pure parse of the switch, so both states are testable without touching the
/// process environment. Default ON; only an explicit off-spelling turns it off.
pub(crate) fn switch_from_value(raw: Option<&str>) -> bool {
    crate::gc::env_default_on_from_value(raw)
}

/// Mask of the low `len` payload bytes (`len <= 5`).
#[inline(always)]
fn low_bytes_mask(len: u64) -> u64 {
    (1u64 << (len * 8)) - 1
}

/// `(payload, byte_len)` of a short ASCII string operand, or `None`.
///
/// The payload holds the operand's bytes in inline-string order (byte 0 in the
/// low bits), with every byte above `byte_len` zero.
#[inline(always)]
fn short_ascii_operand(bits: u64) -> Option<(u64, u64)> {
    let tag = bits & TAG_MASK;
    if tag == SHORT_STRING_TAG {
        let len = (bits >> SHORT_STRING_LEN_SHIFT) & 0xFF;
        if len > SHORT_STRING_MAX_LEN as u64 {
            return None;
        }
        let payload = bits & low_bytes_mask(len);
        if payload & PAYLOAD_HIGH_BITS != 0 {
            return None;
        }
        return Some((payload, len));
    }
    if tag != STRING_TAG {
        return None;
    }
    let hdr = (bits & POINTER_MASK) as *const StringHeader;
    if hdr.is_null() {
        // The general path reads a null string pointer as the empty string.
        return Some((0, 0));
    }
    // SAFETY: a STRING_TAG value with a non-null payload is a live
    // `StringHeader` (the same trust `str_bytes_ascii_from_jsvalue` extends);
    // only the header and at most five payload bytes are read.
    unsafe {
        let len = (*hdr).byte_len;
        if len as usize > SHORT_STRING_MAX_LEN
            || (*hdr).utf16_len != len
            || (*hdr).flags & STRING_FLAG_WTF8_VALIDATED == 0
        {
            return None;
        }
        let data = super::string_data(hdr);
        let mut payload = 0u64;
        for i in 0..len as usize {
            payload |= (*data.add(i) as u64) << (i * 8);
        }
        if payload & PAYLOAD_HIGH_BITS != 0 {
            return None;
        }
        Some((payload, len as u64))
    }
}

/// `a + b` as an inline string when both operands are short ASCII strings and
/// the result fits inline; `None` otherwise (the caller's general path then
/// answers). Identical bits to what the general path builds for these inputs.
#[inline(always)]
pub(crate) fn concat_short_pair(a: f64, b: f64) -> Option<f64> {
    let result = short_pair_bits(a.to_bits(), b.to_bits());
    probe::count(result.is_some());
    result.map(f64::from_bits)
}

#[inline(always)]
fn short_pair_bits(a: u64, b: u64) -> Option<u64> {
    // Both operands inline: one tag test each, one length sum (a sum of two
    // length bytes is at most 5 only when each is), one ASCII test over both
    // payloads.
    if a & TAG_MASK == SHORT_STRING_TAG && b & TAG_MASK == SHORT_STRING_TAG {
        let la = (a >> SHORT_STRING_LEN_SHIFT) & 0xFF;
        let lb = (b >> SHORT_STRING_LEN_SHIFT) & 0xFF;
        let total = la + lb;
        if total > SHORT_STRING_MAX_LEN as u64 {
            return None;
        }
        let pa = a & low_bytes_mask(la);
        let pb = b & low_bytes_mask(lb);
        if (pa | pb) & PAYLOAD_HIGH_BITS != 0 {
            return None;
        }
        return Some(SHORT_STRING_TAG | (total << SHORT_STRING_LEN_SHIFT) | pa | (pb << (la * 8)));
    }
    let (pa, la) = short_ascii_operand(a)?;
    let (pb, lb) = short_ascii_operand(b)?;
    let total = la + lb;
    if total > SHORT_STRING_MAX_LEN as u64 {
        return None;
    }
    Some(SHORT_STRING_TAG | (total << SHORT_STRING_LEN_SHIFT) | pa | (pb << (la * 8)))
}

/// Probe: `PERRY_STRING_PROBE=1` on an instrumented or debug-profile runtime
/// (`perry_hot_diag`) prints one `PERRY_STRING_PROBE short_pairs=N declined=M`
/// line on stderr at exit: the concatenations this arm answered and the ones
/// it was asked and declined. A release runtime compiles the probe out.
#[cfg(perry_hot_diag)]
mod probe {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    static ARMED: AtomicBool = AtomicBool::new(false);
    static TAKEN: AtomicU64 = AtomicU64::new(0);
    static DECLINED: AtomicU64 = AtomicU64::new(0);
    static REPORT: std::sync::Once = std::sync::Once::new();

    pub(super) fn arm_from_env() {
        if crate::gc::env_flag_enabled("PERRY_STRING_PROBE") {
            REPORT.call_once(|| unsafe {
                ARMED.store(true, Ordering::Relaxed);
                libc::atexit(report);
            });
        }
    }

    #[inline(always)]
    pub(super) fn count(taken: bool) {
        if ARMED.load(Ordering::Relaxed) {
            let counter = if taken { &TAKEN } else { &DECLINED };
            counter.fetch_add(1, Ordering::Relaxed);
        }
    }

    extern "C" fn report() {
        eprintln!(
            "PERRY_STRING_PROBE short_pairs={} declined={}",
            TAKEN.load(Ordering::Relaxed),
            DECLINED.load(Ordering::Relaxed)
        );
    }
}

#[cfg(not(perry_hot_diag))]
mod probe {
    #[inline(always)]
    pub(super) fn arm_from_env() {}
    #[inline(always)]
    pub(super) fn count(_taken: bool) {}
}

#[cfg(test)]
#[path = "short_pair_tests.rs"]
mod tests;
