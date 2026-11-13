//! Shape publication failures terminate without Rust I/O error callbacks.
use super::ShapeId;

#[inline]
fn write(bytes: &[u8]) {
    // SAFETY: the slice remains live during this synchronous CRT write.
    unsafe { libc::write(2, bytes.as_ptr().cast(), bytes.len() as _) };
}

fn hex_bytes(value: u64) -> ([u8; 18], usize) {
    let mut bytes = [0; 18];
    bytes[..2].copy_from_slice(b"0x");
    let count = ((u64::BITS - value.leading_zeros()).div_ceil(4) as usize).max(1);
    for index in 0..count {
        bytes[index + 2] =
            b"0123456789abcdef"[((value >> ((count - index - 1) * 4)) & 15) as usize];
    }
    (bytes, count + 2)
}

fn write_hex(value: u64) {
    let (bytes, length) = hex_bytes(value);
    write(&bytes[..length]);
}

#[cold]
#[optimize(size)]
#[inline(never)]
pub(super) fn static_shape_id_refused_abort(requested: ShapeId, proto_id: u64) -> ! {
    write(b"Perry internal error: the static ShapeId ");
    write_hex(u64::from(requested.0));
    write(b" (proto ");
    write_hex(proto_id);
    write(b") was refused by the shape mint (it is outside the static band or already names other facts in this agent); a static id must name one shape\n");
    std::process::abort()
}

#[cold]
#[optimize(size)]
#[inline(never)]
pub(super) fn shape_id_exhausted_abort() -> ! {
    write(b"Perry ShapeId space exhausted; refusing to publish an untracked object shape\n");
    std::process::abort()
}

#[cold]
#[optimize(size)]
#[inline(never)]
pub(super) fn invalid_shape_facts_abort() -> ! {
    write(b"Perry internal error: refusing to publish invalid object shape facts\n");
    std::process::abort()
}

#[cfg(test)]
mod tests {
    #[test]
    fn fatal_hex_preserves_alternate_lower_hex_for_every_width() {
        for value in [0, 1, 15, 16, 255, 256, u32::MAX as u64, u64::MAX] {
            let (bytes, count) = super::hex_bytes(value);
            assert_eq!(&bytes[..count], format!("{value:#x}").as_bytes());
        }
    }
}
