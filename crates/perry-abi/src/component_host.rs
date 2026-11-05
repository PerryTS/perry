//! Versioned C boundary for the optional Component Model engine.
//! Byte buffers are freed by the allocator that produced them. Neither WIT
//! values nor callbacks expose Perry heap addresses to component guests.
// Combined command, host-resource, HTTP reactor and deferred-cleanup operations.
// ABI 11 also requires stale-safe canonical writer collection commands.
// ABI 12 adds pre-initialization HTTP profiling and typed persistent handoff.
pub const ABI_VERSION: u32 = 12;
/// Queued/in-flight native calls; returned background contexts have a separate
/// runtime retention limit and do not occupy native admission permits.
pub const MAX_PENDING_CALLS: usize = 64;

pub const OK: i32 = 0;
pub const ERROR: i32 = 1;
/// The store or resource was released, but its destructor failed. Embeddings
/// must invalidate the released object before propagating the original error.
/// Ordinary ERROR means release was rejected (for example reentry or an
/// invalid resource token); it does not authorize invalidating a live object.
pub const RELEASED_ERROR: i32 = 2;
/// Deferred cleanup encountered an active call; the embedding retains its
/// request and retries after that call. No resource was changed.
pub const DEFERRED: i32 = 3;
/// A native HTTP accept/completion is still waiting; its registered waker will
/// notify the owning agent. Polling never invokes component or JS code.
pub const HTTP_PENDING: i32 = 4;
/// An owned request is ready for exactly one incoming-handler invocation.
pub const HTTP_READY: i32 = 5;
/// A component call/import is suspended; wakeups only notify the owning agent.
pub const ASYNC_PENDING: i32 = 6;
/// Async HTTP handling completed with an HTTP/validation error while keeping
/// its Store live. No call remains pending; the embedding must not cancel it.
pub const HTTP_FAILURE: i32 = 7;
/// A process-lifetime native wake function. Context is an agent identifier,
/// never a Perry heap address. May be called on the native I/O driver thread.
pub type WakeCallback = unsafe extern "C" fn(agent: u64);

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
/// Synchronous resource lifecycle events. Lower/Lift only update private
/// ownership state; Drop may call a JS destructor. Exceptions return normally.
pub type ResourceCallback = unsafe extern "C" fn(
    context: u64,
    event: *const u8,
    event_len: usize,
    output: *mut Buffer,
) -> i32;

/// Persistent-driver callback. `context` belongs to this poll only; `origin`
/// identifies the submitting invocation across polls and post-return work.
/// Origin zero denotes initialization or component-wide retirement. Neither
/// integer is a guest-visible pointer. Catch exceptions and return normally.
pub type OriginCallback = unsafe extern "C" fn(
    context: u64,
    origin: u64,
    interface: *const u8,
    interface_len: usize,
    function: *const u8,
    function_len: usize,
    args: *const u8,
    args_len: usize,
    output: *mut Buffer,
) -> i32;
pub type OriginResourceCallback = unsafe extern "C" fn(
    context: u64,
    origin: u64,
    event: *const u8,
    event_len: usize,
    output: *mut Buffer,
) -> i32;

#[macro_export]
macro_rules! component_host_entry_points {
    ($m:ident) => { $m! {
        perry_component_host_run_command(bytes: *const u8, len: usize, options: *const u8, options_len: usize, output: *mut $crate::component_host::Buffer) -> i32;
        perry_component_host_buffer_drop(buffer: $crate::component_host::Buffer) -> ();
        perry_component_host_new(bytes: *const u8, len: usize, contract: *const u8, contract_len: usize, options: *const u8, options_len: usize, callback: Option<$crate::component_host::Callback>, free_callback: Option<$crate::component_host::FreeCallback>, resource_callback: Option<$crate::component_host::ResourceCallback>, context: u64, error: *mut $crate::component_host::Buffer) -> *mut core::ffi::c_void;
        perry_component_host_call(handle: *mut core::ffi::c_void, interface: *const u8, interface_len: usize, function: *const u8, function_len: usize, args: *const u8, args_len: usize, context: u64, output: *mut $crate::component_host::Buffer) -> i32;
        perry_component_host_call_async_start(handle: *mut core::ffi::c_void, interface: *const u8, interface_len: usize, function: *const u8, function_len: usize, args: *const u8, args_len: usize, context: u64, cleanup_context: u64, wake: Option<$crate::component_host::WakeCallback>, agent: u64, output: *mut $crate::component_host::Buffer) -> i32;
        perry_component_host_call_async_poll(handle: *mut core::ffi::c_void, context: u64, cleanup_context: u64, wake: Option<$crate::component_host::WakeCallback>, agent: u64, output: *mut $crate::component_host::Buffer) -> i32;
        perry_component_host_async_import_resolve(handle: *mut core::ffi::c_void, request: u64, success: bool, value: *const u8, value_len: usize, output: *mut $crate::component_host::Buffer) -> i32;
        perry_component_host_dispose(handle: *mut core::ffi::c_void, context: u64, error: *mut $crate::component_host::Buffer) -> i32;
        perry_component_host_resource_drop(handle: *mut core::ffi::c_void, token: *const u8, token_len: usize, context: u64, error: *mut $crate::component_host::Buffer) -> i32;
        perry_component_host_resource_collect(handle: *mut core::ffi::c_void, request: *const u8, request_len: usize, context: u64, error: *mut $crate::component_host::Buffer) -> i32;
        perry_component_http_server_new(options: *const u8, options_len: usize, output: *mut $crate::component_host::Buffer) -> *mut core::ffi::c_void;
        perry_component_http_server_drop(server: *mut core::ffi::c_void) -> ();
        perry_component_http_accept(server: *mut core::ffi::c_void, output: *mut $crate::component_host::Buffer) -> *mut core::ffi::c_void;
        perry_component_http_poll(operation: *mut core::ffi::c_void, wake: Option<$crate::component_host::WakeCallback>, agent: u64, output: *mut $crate::component_host::Buffer) -> i32;
        perry_component_http_handle(handle: *mut core::ffi::c_void, operation: *mut core::ffi::c_void, interface: *const u8, interface_len: usize, context: u64, cleanup_context: u64, output: *mut $crate::component_host::Buffer) -> i32;
        perry_component_http_handle_async_start(handle: *mut core::ffi::c_void, operation: *mut core::ffi::c_void, interface: *const u8, interface_len: usize, context: u64, cleanup_context: u64, wake: Option<$crate::component_host::WakeCallback>, agent: u64, output: *mut $crate::component_host::Buffer) -> i32;
        perry_component_http_drop(operation: *mut core::ffi::c_void) -> ();
        perry_component_host_interrupt(handle: *mut core::ffi::c_void) -> ();
        perry_component_host_drop(handle: *mut core::ffi::c_void) -> ();
        perry_component_driver_new(bytes: *const u8, len: usize, contract: *const u8, contract_len: usize, options: *const u8, options_len: usize, callback: Option<$crate::component_host::OriginCallback>, free_callback: Option<$crate::component_host::FreeCallback>, resource_callback: Option<$crate::component_host::OriginResourceCallback>, context: u64, error: *mut $crate::component_host::Buffer) -> *mut core::ffi::c_void;
        perry_component_driver_types(handle: *mut core::ffi::c_void, output: *mut $crate::component_host::Buffer) -> i32;
        perry_component_driver_http_profile(bytes: *const u8, len: usize, contract: *const u8, contract_len: usize, output: *mut $crate::component_host::Buffer) -> i32;
        perry_component_driver_http_submit(handle: *mut core::ffi::c_void, operation: *mut core::ffi::c_void, interface: *const u8, interface_len: usize, output: *mut $crate::component_host::Buffer) -> i32;
        perry_component_driver_submit(handle: *mut core::ffi::c_void, request: *const u8, request_len: usize, output: *mut $crate::component_host::Buffer) -> i32;
        perry_component_driver_poll(handle: *mut core::ffi::c_void, context: u64, cleanup_context: u64, wake: Option<$crate::component_host::WakeCallback>, agent: u64, output: *mut $crate::component_host::Buffer) -> i32;
        perry_component_driver_cancel(handle: *mut core::ffi::c_void, invocation: u64, output: *mut $crate::component_host::Buffer) -> i32;
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
