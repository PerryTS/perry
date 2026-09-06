//! Exact escaped-string output with native scalar planning and no scratch buffer.
//! Invalid UTF-8 (including WTF-8 surrogates) retains the general serializer.

use crate::string::{init_string_header, string_data, string_storage_alloc, StringHeader};

#[derive(Clone, Copy)]
pub(super) struct Plan {
    source_bytes: u32,
    pub(super) bytes: u32,
    pub(super) units: u32,
}

impl Plan {
    #[inline(never)]
    pub(super) fn new(source: &[u8], units: u32) -> Option<Self> {
        let len = u32::try_from(source.len()).ok()?;
        // Raw internal strings and synthetic SSO values need not be valid UTF-8.
        // Never infer validity from the string flags, which can propagate by OR.
        std::str::from_utf8(source).ok()?;
        let extra = source.iter().fold(0u64, |extra, &b| {
            let control = b < 0x20;
            let short = matches!(b, b'\n' | b'\r' | b'\t' | 8 | 12);
            extra + u64::from(control || b == b'"' || b == b'\\') + 4 * u64::from(control && !short)
        });
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
    pub(super) unsafe fn write(self, source: *const u8, output: *mut u8) -> usize {
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
