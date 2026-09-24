//! Process shutdown signals on turnloop's process-wide signal service.

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use turnloop::{Handle, Signal, Token};

use super::{io_error, try_with_reactor, with_current, with_reactor, Event, UNROUTED};

/// Which shutdown request arrived.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShutdownSignal {
    /// SIGINT, or console Ctrl-C on Windows.
    Interrupt,
    /// SIGTERM. Windows has no console equivalent, so it never arrives there.
    Terminate,
}

impl ShutdownSignal {
    /// The conventional `128 + signo` exit status for this signal.
    pub fn exit_code(self) -> i32 {
        match self {
            Self::Interrupt => 130,
            Self::Terminate => 143,
        }
    }
}

/// Wait for the first SIGINT or SIGTERM delivered to the process.
///
/// Subscribing is per loop and does not steal the signal from other
/// subscribers: every subscribed turnloop loop gets its own completion.
pub fn shutdown_signal() -> ShutdownSignalFuture {
    ShutdownSignalFuture { subs: None }
}

struct Subscription {
    kind: ShutdownSignal,
    handle: Handle,
    token: u64,
}

struct Subscriptions {
    reactor: u64,
    list: Vec<Subscription>,
}

/// Future returned by [`shutdown_signal`].
#[must_use = "futures do nothing unless awaited"]
pub struct ShutdownSignalFuture {
    subs: Option<Subscriptions>,
}

impl Future for ShutdownSignalFuture {
    type Output = io::Result<ShutdownSignal>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        if this.subs.is_none() {
            let subscribed = with_current(|reactor| {
                let mut list = Vec::new();
                let mut last_error = None;
                for (kind, signal) in [
                    (ShutdownSignal::Interrupt, Signal::Int),
                    (ShutdownSignal::Terminate, Signal::Term),
                ] {
                    let token = reactor.register();
                    match reactor.driver.signal_start(signal, Token(token)) {
                        Ok(handle) => list.push(Subscription {
                            kind,
                            handle,
                            token,
                        }),
                        // SIGTERM is `Unsupported` on Windows; one
                        // subscription is enough to wait on.
                        Err(e) => {
                            reactor.forget(token);
                            last_error = Some(e);
                        }
                    }
                }
                match (list.is_empty(), last_error) {
                    (true, Some(e)) => Err(io_error(e)),
                    _ => Ok(Subscriptions {
                        reactor: reactor.id(),
                        list,
                    }),
                }
            });
            match subscribed {
                Ok(subs) => this.subs = Some(subs),
                Err(e) => return Poll::Ready(Err(e)),
            }
        }
        let subs = this.subs.as_ref().expect("subscribed above");
        let arrived = with_reactor(subs.reactor, |reactor| {
            let mut arrived = None;
            for sub in &subs.list {
                if let Some(Event::Signal) = reactor.take_event(sub.token, cx) {
                    arrived.get_or_insert(sub.kind);
                }
            }
            arrived
        });
        match arrived {
            Some(kind) => {
                this.unsubscribe();
                Poll::Ready(Ok(kind))
            }
            None => Poll::Pending,
        }
    }
}

impl ShutdownSignalFuture {
    fn unsubscribe(&mut self) {
        if let Some(subs) = self.subs.take() {
            try_with_reactor(subs.reactor, |reactor| {
                for sub in &subs.list {
                    reactor.forget(sub.token);
                    let _ = reactor.driver.signal_stop(sub.handle, UNROUTED);
                }
            });
        }
    }
}

impl Drop for ShutdownSignalFuture {
    fn drop(&mut self) {
        self.unsubscribe();
    }
}
