//! Timers: [`sleep`] and [`timeout`] on turnloop one-shot timers.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use turnloop::Token;

use super::{try_with_reactor, with_current, with_reactor, Event};

/// Wait until `duration` has elapsed.
pub fn sleep(duration: Duration) -> Sleep {
    Sleep {
        deadline: Instant::now() + duration,
        timer: None,
        fired: false,
    }
}

/// A turnloop timer registered on first poll.
struct Registration {
    reactor: u64,
    token: u64,
    handle: turnloop::Handle,
}

/// Future returned by [`sleep`].
#[must_use = "futures do nothing unless awaited"]
pub struct Sleep {
    deadline: Instant,
    timer: Option<Registration>,
    fired: bool,
}

impl Sleep {
    fn release(&mut self) {
        if let Some(timer) = self.timer.take() {
            try_with_reactor(timer.reactor, |reactor| {
                reactor.forget(timer.token);
                reactor.close(timer.handle);
            });
        }
    }
}

impl Future for Sleep {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let this = self.get_mut();
        if this.fired {
            return Poll::Ready(());
        }
        let fired = match &this.timer {
            None => {
                let remaining = this.deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    true
                } else {
                    let timer = with_current(|reactor| {
                        let token = reactor.register();
                        let at = reactor.driver.now() + remaining;
                        match reactor.driver.timer(at, None, Token(token)) {
                            Ok(handle) => {
                                // Arm the waker before the first turn can fire it.
                                let _ = reactor.take_event(token, cx);
                                Ok(Registration {
                                    reactor: reactor.id(),
                                    token,
                                    handle,
                                })
                            }
                            Err(e) => {
                                reactor.forget(token);
                                Err(e)
                            }
                        }
                    });
                    match timer {
                        Ok(timer) => {
                            this.timer = Some(timer);
                            false
                        }
                        Err(e) => panic!("perry_container_compose::rt::sleep: timer: {e}"),
                    }
                }
            }
            Some(timer) => with_reactor(timer.reactor, |reactor| {
                matches!(
                    reactor.take_event(timer.token, cx),
                    Some(Event::Timer | Event::Failed(_))
                )
            }),
        };
        if fired {
            this.fired = true;
            this.release();
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

impl Drop for Sleep {
    fn drop(&mut self) {
        self.release();
    }
}

/// Error returned by [`timeout`] when the deadline passes first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Elapsed(());

impl fmt::Display for Elapsed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("deadline has elapsed")
    }
}

impl std::error::Error for Elapsed {}

/// Require `future` to finish within `duration`. The future is polled first,
/// so one that is already ready wins even at a zero duration; on expiry it is
/// dropped, which for [`super::Command`] terminates the child.
pub fn timeout<F: Future>(duration: Duration, future: F) -> Timeout<F> {
    Timeout {
        future: Box::pin(future),
        sleep: sleep(duration),
    }
}

/// Future returned by [`timeout`].
#[must_use = "futures do nothing unless awaited"]
pub struct Timeout<F> {
    future: Pin<Box<F>>,
    sleep: Sleep,
}

impl<F: Future> Future for Timeout<F> {
    type Output = Result<F::Output, Elapsed>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        if let Poll::Ready(output) = this.future.as_mut().poll(cx) {
            return Poll::Ready(Ok(output));
        }
        match Pin::new(&mut this.sleep).poll(cx) {
            Poll::Ready(()) => Poll::Ready(Err(Elapsed(()))),
            Poll::Pending => Poll::Pending,
        }
    }
}
