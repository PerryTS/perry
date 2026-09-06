//! Exact string output with native scalar planning and no scratch buffer.
//! Escaping requires valid UTF-8; unescaped plans preserve the caller's existing
//! raw byte/count semantics after its escape and incomplete-tail checks.

use crate::string::{init_string_header, string_data, string_storage_alloc, StringHeader};

#[derive(Clone, Copy)]
pub(super) struct Plan {
    source_bytes: u32,
    pub(super) bytes: u32,
    pub(super) units: u32,
}

impl Plan {
    /// The caller has checked that quoting preserves these bytes and units.
    #[inline]
    pub(super) fn unescaped(bytes: u32, units: u32) -> Option<Self> {
        Self::with_expansion(bytes, units, 0)
    }

    #[inline(never)]
    pub(super) fn new(source: &[u8], units: u32) -> Option<Self> {
        let len = u32::try_from(source.len()).ok()?;
        // Raw internal strings and synthetic SSO values need not be valid UTF-8.
        // Never infer validity from the string flags, which can propagate by OR.
        std::str::from_utf8(source).ok()?;
        let extra = count_expansion(source);
        Self::with_expansion(len, units, extra)
    }

    fn with_expansion(source_bytes: u32, units: u32, extra: u64) -> Option<Self> {
        let expansion = u32::try_from(extra.checked_add(2)?).ok()?;
        Some(Self {
            source_bytes,
            bytes: source_bytes.checked_add(expansion)?,
            units: units.checked_add(expansion)?,
        })
    }

    /// Source must be rederived after the final allocation, with its original
    /// bytes unchanged. Output has `self.bytes` writable, nonoverlapping bytes.
    /// No allocation, callbacks or collection may occur during this write.
    #[inline]
    pub(super) unsafe fn write(self, source: *const u8, output: *mut u8) -> usize {
        // The same String variant covers both modes. Ordinary scalar emitters
        // keep their two-way String/Inline dispatch and no escaping loop body.
        if self.bytes - 2 == self.source_bytes {
            output.write(b'"');
            std::ptr::copy_nonoverlapping(source, output.add(1), self.source_bytes as usize);
            output.add(self.bytes as usize - 1).write(b'"');
            self.bytes as usize
        } else {
            self.write_escaped(source, output)
        }
    }

    #[inline(never)]
    unsafe fn write_escaped(self, source: *const u8, output: *mut u8) -> usize {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        output.write(b'"');
        let mut at = 1usize;
        for i in 0..self.source_bytes as usize {
            let b = source.add(i).read();
            if b >= 0x20 && b != b'"' && b != b'\\' {
                output.add(at).write(b);
                at += 1;
                continue;
            }
            output.add(at).write(b'\\');
            let short = match b {
                b'"' | b'\\' => b,
                b'\n' => b'n',
                b'\r' => b'r',
                b'\t' => b't',
                8 => b'b',
                12 => b'f',
                _ => 0,
            };
            if short != 0 {
                output.add(at + 1).write(short);
                at += 2;
            } else {
                output.add(at + 1).write(b'u');
                output.add(at + 2).write(b'0');
                output.add(at + 3).write(b'0');
                output.add(at + 4).write(HEX[(b >> 4) as usize]);
                output.add(at + 5).write(HEX[(b & 15) as usize]);
                at += 6;
            }
        }
        output.add(at).write(b'"');
        debug_assert_eq!(at + 1, self.bytes as usize);
        at + 1
    }
}

// Only ASCII controls, quotes and backslashes expand valid UTF-8. Controls
// use six output bytes except the five two-byte short escapes.
const EXPANSION: [u8; 256] = {
    let mut table = [0; 256];
    let mut i = 0;
    while i < 32 {
        table[i] = 5;
        i += 1;
    }
    table[8] = 1;
    table[9] = 1;
    table[10] = 1;
    table[12] = 1;
    table[13] = 1;
    table[b'"' as usize] = 1;
    table[b'\\' as usize] = 1;
    table
};

fn count_expansion(bytes: &[u8]) -> u64 {
    let mut at = 0;
    let mut extra = 0u64;
    #[cfg(target_arch = "aarch64")]
    unsafe {
        use std::arch::aarch64::*;
        let controls = uint8x16x2_t(vld1q_u8(EXPANSION.as_ptr()), vdupq_n_u8(5));
        while bytes.len() - at >= 16 {
            let chunk = vld1q_u8(bytes.as_ptr().add(at));
            // A table lookup yields zero for byte values outside 0..32.
            let control_extra = vqtbl2q_u8(controls, chunk);
            let punctuation = vorrq_u8(
                vceqq_u8(chunk, vdupq_n_u8(b'"')),
                vceqq_u8(chunk, vdupq_n_u8(b'\\')),
            );
            let expansion = vorrq_u8(control_extra, vandq_u8(punctuation, vdupq_n_u8(1)));
            // At most 16 * 5 = 80, so the byte reduction cannot overflow.
            extra += vaddvq_u8(expansion) as u64;
            at += 16;
        }
    }
    for &b in &bytes[at..] {
        extra += EXPANSION[b as usize] as u64;
    }
    extra
}

#[inline(never)]
pub(super) unsafe fn quote(source: *const StringHeader) -> Option<*mut StringHeader> {
    let bytes = std::slice::from_raw_parts(string_data(source), (*source).byte_len as usize);
    let plan = Plan::new(bytes, (*source).utf16_len)?;
    let scope = crate::gc::RuntimeHandleScope::new();
    let input = scope.root_string_ptr(source);
    let (result, output) = string_storage_alloc(plan.bytes);
    let source = input.get_raw_const_ptr::<StringHeader>();
    init_string_header(result, plan.units, plan.bytes, plan.bytes, 0, 0);
    plan.write(string_data(source), output);
    Some(result)
}

#[cfg(test)]
#[path = "stringify_escaped_output_tests.rs"]
mod tests;
