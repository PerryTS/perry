//! Object-container sniffing for archive members — extracted from
//! `strip_dedup.rs`, which had crossed the 2000-line size gate.
//!
//! Both predicates read the object's own magic bytes. The archive file
//! NAME must never be used for this: the well-known wrapper path is handed
//! perry's intermediate `_<lib>_nosharedeps.lib`, which carries that
//! extension on every host.

use std::path::Path;

/// True if `path` is an ELF object file (first four bytes `0x7F 'E' 'L' 'F'`).
/// Used to skip panic/unwind-symbol localization on ELF, where localizing
/// `rust_eh_personality` / `DW.ref.rust_eh_personality` breaks PIE relocations
/// (see [`RUST_PANIC_UNWIND_SYMBOL_PARTS`]).
pub(super) fn object_is_elf(path: &Path) -> bool {
    use std::io::Read;
    let mut magic = [0u8; 4];
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut magic))
        .map(|_| magic == [0x7f, b'E', b'L', b'F'])
        .unwrap_or(false)
}

/// True only when `path` is positively identified as a COFF object.
///
/// Container format must be read from the object's own bytes: the well-known
/// wrapper path receives perry's intermediate archive, which is named
/// `_<lib>_nosharedeps.lib` regardless of host, so a file-extension check
/// misidentifies Mach-O and ELF archives as COFF. Anything unreadable, ELF, or
/// Mach-O (thin or fat, either endianness) answers `false`, keeping the
/// Windows-only `--redefine-sym` / `--format=coff` handling off every other
/// platform.
pub(super) fn object_is_coff(path: &Path) -> bool {
    use std::io::Read;
    let mut magic = [0u8; 4];
    if std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut magic))
        .is_err()
    {
        return false;
    }
    if magic == [0x7f, b'E', b'L', b'F'] {
        return false;
    }
    // Mach-O: 0xfeedface / 0xfeedfacf in either byte order, plus the
    // big-endian fat-archive magic 0xcafebabe.
    if matches!(u32::from_le_bytes(magic), 0xfeed_face | 0xfeed_facf)
        || matches!(
            u32::from_be_bytes(magic),
            0xfeed_face | 0xfeed_facf | 0xcafe_babe
        )
    {
        return false;
    }
    // COFF anonymous/"bigobj" header, emitted by MSVC for large objects.
    if magic == [0x00, 0x00, 0xff, 0xff] {
        return true;
    }
    // Ordinary COFF starts with its little-endian machine word.
    matches!(
        u16::from_le_bytes([magic[0], magic[1]]),
        0x014c | 0x8664 | 0xaa64 | 0x01c0 | 0x01c4 | 0x0200 | 0x6264
    )
}
