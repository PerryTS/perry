//! Legacy callbacks and origin-explicit callbacks share allocator ownership.
use super::*;
use perry_abi::component_host::{OriginCallback, OriginResourceCallback};

fn origin() -> u64 {
    crate::current_invocation().map_or(0, |id| id.as_u64())
}

#[derive(Clone, Copy)]
pub(super) enum ImportCallback {
    Legacy(Callback),
    Origin(OriginCallback),
}
impl ImportCallback {
    pub unsafe fn call(
        self,
        context: u64,
        interface: &str,
        function: &str,
        args: &[u8],
        output: *mut Buffer,
    ) -> i32 {
        match self {
            Self::Legacy(callback) => callback(
                context,
                interface.as_ptr(),
                interface.len(),
                function.as_ptr(),
                function.len(),
                args.as_ptr(),
                args.len(),
                output,
            ),
            Self::Origin(callback) => callback(
                context,
                origin(),
                interface.as_ptr(),
                interface.len(),
                function.as_ptr(),
                function.len(),
                args.as_ptr(),
                args.len(),
                output,
            ),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum LifecycleCallback {
    Legacy(ResourceCallback),
    Origin(OriginResourceCallback),
}
impl LifecycleCallback {
    pub unsafe fn call(self, context: u64, event: &[u8], output: *mut Buffer) -> i32 {
        match self {
            Self::Legacy(callback) => callback(context, event.as_ptr(), event.len(), output),
            Self::Origin(callback) => {
                callback(context, origin(), event.as_ptr(), event.len(), output)
            }
        }
    }
}
