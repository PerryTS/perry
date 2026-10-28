pub(super) use crate::event_pump::service_errors::invalid_input;
#[cfg(not(any(unix, windows)))]
pub(super) use crate::event_pump::service_errors::unsupported;
pub use crate::event_pump::service_errors::{
    from_os, libuv_errno, map_error, os_code_for_name, NodeError,
};
