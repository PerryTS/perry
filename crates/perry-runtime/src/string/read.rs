//! Native readers fill their final GC-owned UTF-8 storage. The header is
//! private and empty until EOF; only then is the payload's metadata published.
use super::*;
use std::io::{self, BorrowedBuf, Read};
use std::mem::MaybeUninit;

// A streaming I/O buffer size, also used when a source supplies no size hint.
const READ_CHUNK: usize = 64 * 1024;

#[path = "read_ascii.rs"]
mod ascii;

/// Native input produces a pointer-free leaf. Existing malloc GC ownership
/// lets a minor reclaim it without an old-arena page index for every payload
/// page. The ordinary allocation trigger and runtime roots still apply.
fn input_storage_alloc(capacity: u32) -> (*mut StringHeader, *mut u8) {
    let size = std::mem::size_of::<StringHeader>() + capacity as usize;
    let header = crate::gc::gc_malloc(size, crate::gc::GC_TYPE_STRING).cast::<StringHeader>();
    (header, string_data(header).cast_mut())
}

#[derive(Default)]
struct Utf8Progress {
    checked: usize,
    units: usize,
    invalid: bool,
}

impl Utf8Progress {
    fn scan(&mut self, bytes: &[u8]) {
        if self.invalid {
            return;
        }
        let chunk = &bytes[self.checked..];
        if ascii::all_ascii(chunk) {
            self.units += chunk.len();
            self.checked = bytes.len();
        } else if provenance::is_utf8(chunk) {
            self.units += utf16_count::count(unsafe { str::from_utf8_unchecked(chunk) });
            self.checked = bytes.len();
        } else {
            let error = str::from_utf8(chunk).unwrap_err();
            let valid = &chunk[..error.valid_up_to()];
            self.units += utf16_count::count(unsafe { str::from_utf8_unchecked(valid) });
            self.checked += valid.len();
            // At most three incomplete bytes stay for `checked`'s next
            // scan. A malformed sequence needs the standard lossy decoder.
            self.invalid = error.error_len().is_some();
        }
        if self.units > MAX_STRING_LENGTH {
            throw_invalid_string_length();
        }
    }
}

/// `reader` must be a native reader which cannot enter JS or collect while
/// borrowing its output buffer (File and byte slices satisfy that contract).
/// `hint` is capacity only: EOF, short reads and size changes decide length.
pub(crate) fn read_utf8<R: Read + ?Sized>(
    reader: &mut R,
    hint: u64,
) -> io::Result<*mut StringHeader> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let mut capacity = hint.min(MAX_STRING_LENGTH as u64) as u32;
    let (header, _) = input_storage_alloc(capacity);
    unsafe { init_string_header(header, 0, 0, capacity, 0, STRING_FLAG_WTF8_VALIDATED) };
    let owner = scope.root_string_ptr(header);
    let mut len = 0usize;
    let mut progress = Utf8Progress::default();
    loop {
        if len == capacity as usize {
            // Do not double an exactly sized regular file just to find EOF.
            let mut probe = [0u8; 1];
            let n = loop {
                match reader.read(&mut probe) {
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    result => break result?,
                }
            };
            if n == 0 {
                break;
            }
            let next = capacity.saturating_mul(2).max(READ_CHUNK as u32);
            if next <= capacity {
                throw_invalid_string_length();
            }
            let (new, data) = input_storage_alloc(next);
            unsafe {
                // The old owner can move during allocation: reload its root.
                init_string_header(new, 0, 0, next, 0, STRING_FLAG_WTF8_VALIDATED);
                ptr::copy_nonoverlapping(string_data(owner.get_raw_const_ptr()), data, len);
                data.add(len).write(probe[0]);
            }
            owner.set_raw_mut_ptr(new);
            capacity = next;
            len += 1;
        } else {
            let n = unsafe {
                let data = string_data(owner.get_raw_const_ptr()).cast_mut().add(len);
                let spare = slice::from_raw_parts_mut(
                    data.cast::<MaybeUninit<u8>>(),
                    (capacity as usize - len).min(READ_CHUNK),
                );
                let mut output = BorrowedBuf::from(spare);
                loop {
                    match reader.read_buf(output.unfilled()) {
                        Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                        result => {
                            result?;
                            break;
                        }
                    }
                }
                output.filled().len()
            };
            if n == 0 {
                break;
            }
            len += n;
        }
        // Only initialized bytes are exposed, including a pending UTF-8 tail.
        let bytes = unsafe { slice::from_raw_parts(string_data(owner.get_raw_const_ptr()), len) };
        progress.scan(bytes);
    }
    let header: *mut StringHeader = owner.get_raw_mut_ptr();
    unsafe {
        // A shrinking file or overestimate leaves private spare capacity.
        // Initialize it before publishing the object to heap diagnostics.
        ptr::write_bytes(
            string_data(header).cast_mut().add(len),
            0,
            capacity as usize - len,
        );
        if progress.invalid || progress.checked != len {
            let bytes = slice::from_raw_parts(string_data(header), len);
            // Own the repaired bytes before the allocating constructor, so a
            // collection cannot invalidate a borrowed source payload.
            let text = String::from_utf8_lossy(bytes).into_owned();
            let units = utf16_count::count(&text);
            if units > MAX_STRING_LENGTH || text.len() > u32::MAX as usize {
                throw_invalid_string_length();
            }
            return Ok(js_string_from_bytes_known_utf16(
                text.as_ptr(),
                text.len() as u32,
                units as u32,
                STRING_FLAG_WTF8_VALIDATED,
            ));
        }
        init_string_header(
            header,
            progress.units as u32,
            len as u32,
            capacity,
            0,
            STRING_FLAG_WTF8_VALIDATED,
        );
    }
    Ok(header)
}

#[cfg(test)]
#[path = "read_tests.rs"]
mod tests;
