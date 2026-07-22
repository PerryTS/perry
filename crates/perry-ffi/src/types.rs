//! Canonical Perry FFI ABI types.
//!
//! These layouts are the public contract wrapper crates compile
//! against. The runtime may own the allocation and implementation,
//! but wrappers should name these types through `perry-ffi` rather
//! than importing runtime internals.

/// Length of the fixed BigInt limb array.
pub const BIGINT_LIMBS: usize = 16;

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

/// Header for a runtime-allocated JS array.
#[repr(C)]
pub struct ArrayHeader {
    /// Number of elements currently in the array.
    pub length: u32,
    /// Allocated element capacity.
    pub capacity: u32,
}

/// Header for a runtime-allocated JS object.
#[repr(C)]
pub struct ObjectHeader {
    /// Runtime object type discriminator.
    pub object_type: u32,
    /// Runtime class identifier.
    pub class_id: u32,
    /// Runtime parent class identifier, or zero when absent.
    pub parent_class_id: u32,
    /// Number of inline fields.
    pub field_count: u32,
    /// Runtime array of object keys, or null for class instances.
    pub keys_array: *mut ArrayHeader,
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

    #[test]
    fn object_header_matches_runtime() {
        assert_layout!(ObjectHeader, perry_runtime::ObjectHeader);
        assert_eq!(
            offset_of!(ObjectHeader, object_type),
            offset_of!(perry_runtime::ObjectHeader, object_type)
        );
        assert_eq!(
            offset_of!(ObjectHeader, class_id),
            offset_of!(perry_runtime::ObjectHeader, class_id)
        );
        assert_eq!(
            offset_of!(ObjectHeader, parent_class_id),
            offset_of!(perry_runtime::ObjectHeader, parent_class_id)
        );
        assert_eq!(
            offset_of!(ObjectHeader, field_count),
            offset_of!(perry_runtime::ObjectHeader, field_count)
        );
        assert_eq!(
            offset_of!(ObjectHeader, keys_array),
            offset_of!(perry_runtime::ObjectHeader, keys_array)
        );
        assert_eq!(
            offset_of!(ObjectHeader, meta),
            offset_of!(perry_runtime::ObjectHeader, meta)
        );
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
