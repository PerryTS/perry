//! Protocol operations name a logical session or a phase-C client transport.
//! A server session's ordinary Socket edge is its connection ownership.

use perry_ext_net::native_transport::{self as net, RootedSocket};
use perry_ffi::turnloop_net as tl;

#[derive(Clone, Copy, Debug)]
pub(crate) enum Target {
    Client(i64),
    Session(i64),
}
impl From<i64> for Target {
    fn from(id: i64) -> Self {
        Self::Client(id)
    }
}
impl Target {
    pub(super) fn socket(self) -> Option<RootedSocket> {
        let Self::Session(session) = self else {
            return None;
        };
        let session =
            perry_ffi::get_handle::<crate::server::http2_server::Http2SessionHandle>(session)?;
        if session.owner_agent != perry_ffi::agent_post::current_agent() {
            return None;
        }
        let socket = RootedSocket::new(session.socket_value);
        session
            .socket_incarnation
            .as_ref()
            .filter(|handle| net::matches(socket.value(), handle))?;
        Some(socket)
    }
    pub(crate) fn is_live(self) -> bool {
        match self {
            Self::Client(id) => super::conn::peek(id, |codec| !codec.destroyed).unwrap_or(false),
            Self::Session(_) => self.socket().is_some(),
        }
    }
    pub(super) fn cancel_timer(self) {
        match self {
            Self::Client(id) => {
                let _ = tl::timer_cancel(id);
            }
            Self::Session(_) => {
                if let Some(socket) = self.socket() {
                    net::deadline_cancel(socket.value());
                }
            }
        }
    }
    pub(super) fn destroy(self) {
        match self {
            Self::Client(id) => {
                let _ = tl::close(id);
            }
            Self::Session(_) => {
                if let Some(socket) = self.socket() {
                    net::destroy(socket.value());
                }
            }
        }
    }
    pub(super) fn shutdown(self) {
        if let Some(socket) = self.socket() {
            if net::shutdown(socket.value(), 0).is_err() {
                net::destroy(socket.value());
            }
        }
    }
}
