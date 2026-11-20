//! Native completion table injected into the optional component host.
use perry_component_io::{Embedding, Provider};
struct Runtime;
impl Embedding for Runtime {
    fn with_driver<R>(
        f: impl FnOnce(&mut turnloop::Loop) -> turnloop::Result<R>,
    ) -> turnloop::Result<R> {
        crate::event_pump::with_net_driver(f)
            .unwrap_or_else(|| Err(turnloop::Error::new(turnloop::ErrorKind::Unsupported)))
    }
    fn turn_and_take(
        operations: &[turnloop::OpId],
        closing: &[turnloop::Handle],
        timeout: turnloop::Timeout,
    ) -> turnloop::Result<Vec<turnloop::Completion>> {
        crate::event_pump::component_turn_and_take(operations, closing, timeout)
    }
}
#[no_mangle]
pub extern "C" fn js_runtime_component_io_ops() -> *const perry_abi::component_io::Ops {
    &Provider::<Runtime>::OPS
}
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_COMPONENT_IO: extern "C" fn() -> *const perry_abi::component_io::Ops =
    js_runtime_component_io_ops;
