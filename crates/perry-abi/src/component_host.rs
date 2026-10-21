//! Versioned C boundary for the optional Component Model engine.
//! Byte buffers are freed by the allocator that produced them. Neither WIT
//! values nor callbacks expose Perry heap addresses to component guests.
pub const ABI_VERSION: u32 = 1;
pub const OK: i32 = 0;
pub const ERROR: i32 = 1;
/// The store or resource was released, but its destructor failed. Embeddings
/// must invalidate the released object before propagating the original error.
/// Ordinary ERROR means release was rejected (for example reentry or an
/// invalid resource token); it does not authorize invalidating a live object.
pub const RELEASED_ERROR: i32 = 2;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Buffer {
    pub data: *mut u8,
    pub len: usize,
}

impl Default for Buffer {
    fn default() -> Self {
        Self {
            data: core::ptr::null_mut(),
            len: 0,
        }
    }
}

/// Return OK and JSON for a result, or ERROR and a UTF-8 error message.
/// A void import returns JSON null. `context` is live only during the enclosing
/// host call. The callback must catch JS exceptions and return normally.
pub type Callback = unsafe extern "C" fn(
    context: u64,
    interface: *const u8,
    interface_len: usize,
    function: *const u8,
    function_len: usize,
    args: *const u8,
    args_len: usize,
    output: *mut Buffer,
) -> i32;
pub type FreeCallback = unsafe extern "C" fn(Buffer);

#[macro_export]
macro_rules! component_host_entry_points {
    ($m:ident) => { $m! {
        perry_component_host_buffer_drop(buffer: $crate::component_host::Buffer) -> ();
        perry_component_host_new(bytes: *const u8, len: usize, contract: *const u8, contract_len: usize, options: *const u8, options_len: usize, callback: Option<$crate::component_host::Callback>, free_callback: Option<$crate::component_host::FreeCallback>, context: u64, error: *mut $crate::component_host::Buffer) -> *mut core::ffi::c_void;
        perry_component_host_call(handle: *mut core::ffi::c_void, interface: *const u8, interface_len: usize, function: *const u8, function_len: usize, args: *const u8, args_len: usize, context: u64, output: *mut $crate::component_host::Buffer) -> i32;
        perry_component_host_dispose(handle: *mut core::ffi::c_void, context: u64, error: *mut $crate::component_host::Buffer) -> i32;
        perry_component_host_resource_drop(handle: *mut core::ffi::c_void, token: *const u8, token_len: usize, context: u64, error: *mut $crate::component_host::Buffer) -> i32;
        perry_component_host_interrupt(handle: *mut core::ffi::c_void) -> ();
        perry_component_host_drop(handle: *mut core::ffi::c_void) -> ();
    } };
}

macro_rules! define_ops {
    ($($name:ident($($arg:ident: $ty:ty),*) -> $ret:ty;)*) => {
        #[repr(C)]
        pub struct ComponentHostOps {
            pub abi_version: u32,
            pub size: usize,
            $(pub $name: unsafe extern "C" fn($($ty),*) -> $ret,)*
        }
    };
}
component_host_entry_points!(define_ops);
