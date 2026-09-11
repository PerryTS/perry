//! Bounded escaped-string decoding; no managed allocation or GC entry.

use super::{decode_hex_u16, SpecializedDirectParser};

impl<const SOURCE_LENGTH: bool> SpecializedDirectParser<'_, SOURCE_LENGTH> {
    /// Decode a bounded input window into existing spare Vec storage. A JSON escape
    /// consumes at most 12 bytes and emits at most four. Starting each operation
    /// before byte 52 of a complete 64-byte window bounds every input access and
    /// every output write. The caller proves 64 spare bytes; this function never reallocates.
    pub(super) fn decode_chunk(&mut self, result: &mut Vec<u8>) -> Option<bool> {
        debug_assert!(self.input.len() - self.pos >= 64);
        let used = result.len();
        debug_assert!(result.capacity() - used >= 64);
        let output = unsafe { result.as_mut_ptr().add(used) };
        let mut written = 0usize;
        let mut pos = self.pos;
        let limit = pos + 52;
        macro_rules! invalid {
            () => {{
                self.pos = pos;
                self.valid = false;
                return None;
            }};
        }
        macro_rules! put {
            ($byte:expr) => {{
                unsafe {
                    // GC_STORE_AUDIT(POINTER_FREE): JSON byte-buffer payload.
                    output.add(written).write($byte);
                }
                written += 1;
            }};
        }
        while pos < limit {
            let ch = unsafe { *self.input.get_unchecked(pos) };
            pos += 1;
            match ch {
                b'"' => {
                    self.pos = pos;
                    unsafe {
                        result.set_len(used + written);
                    }
                    return Some(true);
                }
                b'\\' => {
                    let esc = unsafe { *self.input.get_unchecked(pos) };
                    pos += 1;
                    match esc {
                        b'"' => put!(b'"'),
                        b'\\' => put!(b'\\'),
                        b'/' => put!(b'/'),
                        b'n' => put!(b'\n'),
                        b'r' => put!(b'\r'),
                        b't' => put!(b'\t'),
                        b'b' => put!(0x08),
                        b'f' => put!(0x0c),
                        b'u' => {
                            let Some(code) =
                                decode_hex_u16(unsafe { self.input.get_unchecked(pos..pos + 4) })
                            else {
                                invalid!();
                            };
                            pos += 4;
                            let low = if (0xd800..=0xdbff).contains(&code)
                                && self.input[pos] == b'\\'
                                && self.input[pos + 1] == b'u'
                            {
                                decode_hex_u16(unsafe {
                                    self.input.get_unchecked(pos + 2..pos + 6)
                                })
                                .filter(|low| (0xdc00..=0xdfff).contains(low))
                            } else {
                                None
                            };
                            let mut bytes = [0u8; 4];
                            let length = if let Some(low) = low {
                                pos += 6;
                                let scalar = 0x10000
                                    + ((code as u32 - 0xd800) << 10)
                                    + (low as u32 - 0xdc00);
                                char::from_u32(scalar)
                                    .unwrap()
                                    .encode_utf8(&mut bytes)
                                    .len()
                            } else if (0xd800..=0xdfff).contains(&code) {
                                bytes[0] = 0xe0 | (code >> 12) as u8;
                                bytes[1] = 0x80 | ((code >> 6) & 0x3f) as u8;
                                bytes[2] = 0x80 | (code & 0x3f) as u8;
                                3
                            } else {
                                char::from_u32(code as u32)
                                    .unwrap()
                                    .encode_utf8(&mut bytes)
                                    .len()
                            };
                            unsafe {
                                std::ptr::copy_nonoverlapping(
                                    bytes.as_ptr(),
                                    output.add(written),
                                    length,
                                );
                            }
                            written += length;
                        }
                        _ => invalid!(),
                    }
                }
                c if c < 0x20 => invalid!(),
                _ => {
                    // Only ordinary text pays for word scanning. Escape-heavy
                    // strings keep their direct escape dispatch above.
                    const LOW: u64 = 0x0101_0101_0101_0101;
                    const HIGH: u64 = 0x8080_8080_8080_8080;
                    let raw = unsafe {
                        self.input
                            .as_ptr()
                            .add(pos - 1)
                            .cast::<u64>()
                            .read_unaligned()
                    };
                    let word = u64::from_le(raw);
                    let quotes = word ^ 0x2222_2222_2222_2222;
                    let slashes = word ^ 0x5c5c_5c5c_5c5c_5c5c;
                    let mask = ((quotes.wrapping_sub(LOW) & !quotes)
                        | (slashes.wrapping_sub(LOW) & !slashes)
                        | (word.wrapping_sub(0x2020_2020_2020_2020) & !word))
                        & HIGH;
                    // The first byte is ordinary. Before the first marked
                    // little-endian lane no earlier special can cause a borrow.
                    let plain = mask.trailing_zeros() as usize / 8;
                    debug_assert!(plain != 0);
                    unsafe {
                        // GC_STORE_AUDIT(POINTER_FREE): decoded JSON bytes in spare Vec storage.
                        output.add(written).cast::<u64>().write_unaligned(raw);
                    }
                    // The operation started before offset 52; loading/storing
                    // eight bytes fits the complete 64-byte input/output window.
                    // Commit only the ordinary prefix, and let the loop bound
                    // defer any escape reached at or beyond offset 52.
                    pos += plain - 1;
                    written += plain;
                }
            }
        }
        self.pos = pos;
        unsafe {
            result.set_len(used + written);
        }
        Some(false)
    }
}
