//! Canonical Perry FFI ABI types.
//!
//! These layouts are the public contract wrapper crates compile
//! against. The runtime may own the allocation and implementation,
//! but wrappers should name these types through `perry-ffi` rather
//! than importing runtime internals.

/// Length of the fixed BigInt limb array.
pub const BIGINT_LIMBS: usize = 16;

/// Revision of the [`ObjectHeader`] ABI this crate mirrors.
///
/// Bump on ANY change to `ObjectHeader`'s size, field set, or field offsets,
/// and bump `perry_runtime::perry_object_header_abi_revision()` in the same
/// commit — `object_header_abi_revision_matches_the_pinned_layout` fails
/// otherwise.
///
/// It exists because `perry-ffi` is **published to crates.io**: a wrapper built
/// against an older mirror and linked by `perry compile` against a newer
/// runtime reads the wrong offsets with no diagnostic at all. An out-of-tree
/// wrapper should assert
/// `perry_ffi::OBJECT_HEADER_ABI_REVISION == perry_object_header_abi_revision()`
/// (declared `extern "C" fn() -> u32`) once at startup and refuse to run on a
/// mismatch.
///
/// * 1 — `{object_type, class_id, parent_class_id, field_count, keys_array, meta}`,
///   32 bytes on LP64.
/// * 2 — `{class_id, parent_class_id, keys_array, meta}`, 24 bytes on LP64 (#8113).
/// * 3 — `{class_id, parent_class_id, meta}`, 16 bytes on LP64/ILP32 (#8047).
pub const OBJECT_HEADER_ABI_REVISION: u32 = 3;

/// Revision of the [`StringHeader`] ABI this crate mirrors.
///
/// Bump on ANY change to `StringHeader`'s size, field set, field offsets, or
/// representation, and on any change to the meaning of the payload returned by
/// `read_bytes`. Bump `perry_runtime::perry_string_header_abi_revision()` in the
/// same commit — `string_header_abi_revision_matches_the_pinned_layout` fails
/// otherwise.
///
/// It exists because `perry-ffi` is **published to crates.io**: a wrapper built
/// against an older mirror and linked by `perry compile` against a newer
/// runtime could otherwise read the wrong payload bytes with no diagnostic. An
/// out-of-tree wrapper should assert
/// `perry_ffi::STRING_HEADER_ABI_REVISION == perry_string_header_abi_revision()`
/// (declared `extern "C" fn() -> u32`) once at startup and refuse to run on a
/// mismatch.
///
/// * 1 — `{utf16_len, byte_len, capacity, refcount, flags}`, 20 bytes; the
///   payload returned by `read_bytes` begins immediately after the header.
pub const STRING_HEADER_ABI_REVISION: u32 = 1;

/// Header for a runtime-allocated JS string.
#[repr(C)]
pub struct StringHeader {
    /// Length in UTF-16 code units, matching JavaScript `.length`.
    pub utf16_len: u32,
    /// Length in bytes of the payload that follows this header.
    pub byte_len: u32,
    /// Allocated byte capacity for the payload.
    pub capacity: u32,
    /// Runtime reference hint used by string append paths.
    pub refcount: u32,
    /// Runtime string flags.
    pub flags: u32,
}

const _: () = assert!(std::mem::size_of::<StringHeader>() == 20);

/// Header for a runtime-allocated JS array.
#[repr(C)]
pub struct ArrayHeader {
    /// Number of elements currently in the array.
    pub length: u32,
    /// Allocated element capacity.
    pub capacity: u32,
}

/// Header for a runtime-allocated JS object.
///
/// # ABI revision 3 (#8047) — BREAKING for out-of-tree mirrors
///
/// Revision 1 opened with `object_type: u32` and carried `field_count: u32`.
/// Both were derivable and both are gone; `class_id` moved from offset 4 to 0,
/// the shape word from 8 to 4, and the struct shrank from 32 to 24 bytes on
/// LP64 (16 on ILP32). Revision 3 removes the derived keys mirror and is 16
/// bytes on both pointer widths.
///
/// A wrapper compiled against an older mirror and linked against a revision-3
/// runtime reads shifted fields and/or starts the field region at the wrong
/// offset with **no compile error**. Revision 1 cannot detect this
/// retroactively — it references no version symbol — so those consumers must
/// recompile. From revision 2 on, [`OBJECT_HEADER_ABI_REVISION`] gives the tripwire:
/// assert it against the runtime's
/// `perry_object_header_abi_revision()` at startup, and a future layout change
/// is caught instead of silently misread.
#[repr(C)]
pub struct ObjectHeader {
    /// Runtime class identifier. Offset 0 since ABI revision 2 (#8113).
    pub class_id: u32,
    /// Runtime parent class identifier during allocation, then the runtime
    /// ShapeId after shape stamping. Never authoritative parent data.
    pub parent_class_id: u32,
    #[cfg(target_pointer_width = "32")]
    _slot_alignment_padding: u32,
    /// Per-object metadata record (#6759 Phase B), or null when the object
    /// has none. Opaque to FFI consumers — never dereferenced across the
    /// boundary, mirrored only so the header size and field-region offset
    /// stay in lockstep with the runtime.
    pub meta: *mut core::ffi::c_void,
}

/// Header for a runtime-allocated Buffer or Uint8Array payload.
#[repr(C)]
pub struct BufferHeader {
    /// Length in bytes.
    pub length: u32,
    /// Allocated byte capacity.
    pub capacity: u32,
}

/// Header for a runtime-allocated BigInt.
#[repr(C)]
pub struct BigIntHeader {
    /// Fixed little-endian 1024-bit limb storage.
    pub limbs: [u64; BIGINT_LIMBS],
}

/// Header for a runtime-allocated JS closure.
#[repr(C)]
pub struct ClosureHeader {
    /// Pointer to the compiled closure body.
    pub func_ptr: *const u8,
    /// Number of captured values, including runtime flag bits.
    pub capture_count: u32,
    /// Runtime closure type tag.
    pub type_tag: u32,
}

/// Opaque runtime-allocated Promise handle.
///
/// Wrappers only pass `*mut Promise` across the ABI; they must not
/// inspect or allocate this type directly.
#[repr(C)]
pub struct Promise {
    _private: [u8; 0],
}

/// Opaque runtime-owned native async completion token.
///
/// Wrappers pass `*mut NativeAsyncCompletion` through the perry-ffi async
/// helpers; they must not inspect or allocate this type directly.
#[repr(C)]
pub struct NativeAsyncCompletion {
    _private: [u8; 0],
}

#[cfg(all(test, feature = "runtime-link"))]
mod layout_tests {
    use super::*;
    use std::mem::{align_of, offset_of, size_of};

    macro_rules! assert_layout {
        ($ffi:ty, $runtime:ty) => {
            assert_eq!(size_of::<$ffi>(), size_of::<$runtime>());
            assert_eq!(align_of::<$ffi>(), align_of::<$runtime>());
        };
    }

    #[test]
    fn string_header_matches_runtime() {
        assert_layout!(StringHeader, perry_runtime::StringHeader);
        assert_eq!(
            offset_of!(StringHeader, utf16_len),
            offset_of!(perry_runtime::StringHeader, utf16_len)
        );
        assert_eq!(
            offset_of!(StringHeader, byte_len),
            offset_of!(perry_runtime::StringHeader, byte_len)
        );
        assert_eq!(
            offset_of!(StringHeader, capacity),
            offset_of!(perry_runtime::StringHeader, capacity)
        );
        assert_eq!(
            offset_of!(StringHeader, refcount),
            offset_of!(perry_runtime::StringHeader, refcount)
        );
        assert_eq!(
            offset_of!(StringHeader, flags),
            offset_of!(perry_runtime::StringHeader, flags)
        );
    }

    /// Pin both copies of the revision and the absolute published layout. The
    /// mirror test above catches one-sided struct drift; these assertions also
    /// catch both structs changing without the required revision bump.
    #[test]
    fn string_header_abi_revision_matches_the_pinned_layout() {
        assert_eq!(STRING_HEADER_ABI_REVISION, 1);
        assert_eq!(
            STRING_HEADER_ABI_REVISION,
            perry_runtime::perry_string_header_abi_revision(),
            "the runtime and the published mirror disagree about the string header ABI \
             revision — bump BOTH, in the same commit, and say so in the \
             changelog: perry-ffi is published to crates.io"
        );
        assert_eq!(size_of::<StringHeader>(), 20);
        assert_eq!(offset_of!(StringHeader, utf16_len), 0);
        assert_eq!(offset_of!(StringHeader, byte_len), 4);
        assert_eq!(offset_of!(StringHeader, capacity), 8);
        assert_eq!(offset_of!(StringHeader, refcount), 12);
        assert_eq!(offset_of!(StringHeader, flags), 16);
    }

    #[test]
    fn array_header_matches_runtime() {
        assert_layout!(ArrayHeader, perry_runtime::ArrayHeader);
        assert_eq!(
            offset_of!(ArrayHeader, length),
            offset_of!(perry_runtime::ArrayHeader, length)
        );
        assert_eq!(
            offset_of!(ArrayHeader, capacity),
            offset_of!(perry_runtime::ArrayHeader, capacity)
        );
    }

    /// #8113: this test — and the whole `layout_tests` module — had **never
    /// executed**. `runtime-link` is enabled nowhere in `.github/`, and
    /// `cargo-test` is a per-package loop, so a size or padding divergence
    /// between the mirror and the runtime was invisible; only outright field
    /// DELETION went red, via `offset_of!` failing to compile. `test.yml`'s
    /// `cargo-test` job now runs
    /// `cargo test -p perry-ffi --features runtime-link` unconditionally.
    #[test]
    fn object_header_matches_runtime() {
        assert_layout!(ObjectHeader, perry_runtime::ObjectHeader);
        assert_eq!(
            offset_of!(ObjectHeader, class_id),
            offset_of!(perry_runtime::ObjectHeader, class_id)
        );
        assert_eq!(
            offset_of!(ObjectHeader, parent_class_id),
            offset_of!(perry_runtime::ObjectHeader, parent_class_id)
        );
        assert_eq!(
            offset_of!(ObjectHeader, meta),
            offset_of!(perry_runtime::ObjectHeader, meta)
        );
    }

    /// The size/padding half of the mirror contract, spelled separately so a
    /// failure names the actual problem. `assert_layout!` above already covers
    /// it, but this pins the ABSOLUTE numbers too: a mirror that tracks the
    /// runtime while BOTH drift is still an ABI break for every published
    /// consumer, and that is the case `object_header_matches_runtime` cannot
    /// see.
    #[test]
    fn object_header_abi_revision_matches_the_pinned_layout() {
        assert_eq!(OBJECT_HEADER_ABI_REVISION, 3);
        assert_eq!(
            OBJECT_HEADER_ABI_REVISION,
            perry_runtime::perry_object_header_abi_revision(),
            "the runtime and the published mirror disagree about the header ABI \
             revision — bump BOTH, in the same commit, and say so in the \
             changelog: perry-ffi is published to crates.io"
        );
        #[cfg(target_pointer_width = "64")]
        assert_eq!(size_of::<ObjectHeader>(), 16);
        #[cfg(target_pointer_width = "32")]
        assert_eq!(size_of::<ObjectHeader>(), 16);
        assert_eq!(offset_of!(ObjectHeader, class_id), 0);
        assert_eq!(offset_of!(ObjectHeader, parent_class_id), 4);
        #[cfg(target_pointer_width = "64")]
        assert_eq!(offset_of!(ObjectHeader, meta), 8);
        #[cfg(target_pointer_width = "32")]
        assert_eq!(offset_of!(ObjectHeader, meta), 12);
    }

    #[test]
    fn buffer_header_matches_runtime() {
        assert_layout!(BufferHeader, perry_runtime::BufferHeader);
        assert_eq!(
            offset_of!(BufferHeader, length),
            offset_of!(perry_runtime::BufferHeader, length)
        );
        assert_eq!(
            offset_of!(BufferHeader, capacity),
            offset_of!(perry_runtime::BufferHeader, capacity)
        );
    }

    #[test]
    fn bigint_header_matches_runtime() {
        assert_eq!(BIGINT_LIMBS, perry_runtime::bigint::BIGINT_LIMBS);
        assert_layout!(BigIntHeader, perry_runtime::BigIntHeader);
        assert_eq!(
            offset_of!(BigIntHeader, limbs),
            offset_of!(perry_runtime::BigIntHeader, limbs)
        );
    }

    #[test]
    fn closure_header_matches_runtime() {
        assert_layout!(ClosureHeader, perry_runtime::ClosureHeader);
        assert_eq!(
            offset_of!(ClosureHeader, func_ptr),
            offset_of!(perry_runtime::ClosureHeader, func_ptr)
        );
        assert_eq!(
            offset_of!(ClosureHeader, capture_count),
            offset_of!(perry_runtime::ClosureHeader, capture_count)
        );
        assert_eq!(
            offset_of!(ClosureHeader, type_tag),
            offset_of!(perry_runtime::ClosureHeader, type_tag)
        );
    }

    #[test]
    fn promise_is_pointer_abi_only() {
        assert_eq!(
            size_of::<*mut Promise>(),
            size_of::<*mut perry_runtime::promise::Promise>()
        );
        assert_eq!(
            align_of::<*mut Promise>(),
            align_of::<*mut perry_runtime::promise::Promise>()
        );
    }
}
