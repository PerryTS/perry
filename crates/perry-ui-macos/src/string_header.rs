/// Header for heap-allocated strings (mirrors perry_runtime::string::StringHeader).
/// Defined locally to avoid pulling in the entire perry-runtime crate as a dependency,
/// which would cause duplicate symbol errors when linking with libperry_stdlib.a.
#[repr(C)]
pub struct StringHeader {
    /// Length in UTF-16 code units (JS `.length` semantics)
    pub utf16_len: u32,
    /// Length in bytes
    pub byte_len: u32,
    /// Capacity (allocated space for data)
    pub capacity: u32,
    /// Reference hint for in-place append optimization (0=shared, 1=unique)
    pub refcount: u32,
    /// Bit flags: STRING_FLAG_HAS_LONE_SURROGATES = 1
    pub flags: u32,
}
