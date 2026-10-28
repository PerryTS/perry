use crate::{
    Result,
    backend::{PollInfo, Wake},
};
use std::{os::fd::RawFd, sync::Arc, time::Duration};
#[derive(Clone, Copy)]
pub(crate) struct Ready {
    pub key: u64,
    pub read: bool,
    pub write: bool,
    /// kqueue EVFILT_VNODE flags for a filesystem watch; zero otherwise.
    #[cfg_attr(turnloop_backend = "epoll", allow(dead_code))]
    pub vnode: u32,
}
pub(crate) trait Poller: Sized {
    type W: Wake;
    fn new(capacity: usize) -> Result<Self>;
    fn waker(&self) -> Arc<Self::W>;
    fn register(&mut self, fd: RawFd, key: u64) -> Result<()>;
    fn deregister(&mut self, fd: RawFd) -> Result<()>;
    fn process(&mut self, pid: u32, key: u64) -> Result<Option<std::os::fd::OwnedFd>>;
    fn remove_process(&mut self, pid: u32, fd: Option<RawFd>);
    fn wait(&mut self, timeout: Option<Duration>, out: &mut Vec<Ready>) -> Result<PollInfo>;
    fn fd(&self) -> RawFd;
}
pub(crate) fn timespec(d: Duration) -> libc::timespec {
    libc::timespec {
        tv_sec: d.as_secs().min(libc::time_t::MAX as u64) as libc::time_t,
        tv_nsec: d.subsec_nanos().into(),
    }
}
pub(crate) fn last_error() -> crate::Error {
    std::io::Error::last_os_error().into()
}
