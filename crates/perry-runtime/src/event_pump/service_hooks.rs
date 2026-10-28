//! Optional services reached by the common agent loop.
//! Their existing module installers fill these slots before submitting work.
use crate::feature_hooks::Hook;

static NET_DISPATCH: Hook<fn(turnloop::Completion)> = Hook::empty();
static NET_SHUTDOWN: Hook<fn()> = Hook::empty();
static NET_STATS: Hook<fn()> = Hook::empty();
static NET_LIVE_HANDLES: Hook<fn() -> usize> = Hook::empty();

pub(crate) fn install_net() {
    NET_DISPATCH.set(crate::turnloop_net::dispatch);
    NET_SHUTDOWN.set(crate::turnloop_net::shutdown_current_thread);
    NET_STATS.set(crate::turnloop_net::census::print_once);
    NET_LIVE_HANDLES.set(crate::turnloop_net::live_handles);
}
pub(super) fn net_dispatch(completion: turnloop::Completion) {
    if let Some(dispatch) = NET_DISPATCH.get() {
        dispatch(completion);
    }
}
pub(super) fn net_shutdown() {
    if let Some(shutdown) = NET_SHUTDOWN.get() {
        shutdown();
    }
}
pub(super) fn net_stats() {
    if let Some(print) = NET_STATS.get() {
        print();
    }
}
#[cfg_attr(not(debug_assertions), allow(dead_code))]
pub(super) fn net_live_handles() -> usize {
    NET_LIVE_HANDLES.get().map_or(0, |count| count())
}
