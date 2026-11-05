use super::{bool_result, unsupported};
use crate::{Error, ErrorKind, Notifier, Result, Signal};
use std::{
    ptr,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering},
    },
};
use windows_sys::Win32::System::Console::*;
use windows_sys::Win32::System::Threading::{INFINITE, Sleep};

static SLOTS: [AtomicPtr<Ticket>; 1024] = [const { AtomicPtr::new(ptr::null_mut()) }; 1024];
static ACTIVE: AtomicUsize = AtomicUsize::new(0);
// Whether `handler` is installed. Installation is permanent, as in libuv's
// uv__signals_init: SetConsoleCtrlHandler (add or remove) blocks while any control
// handler is running (windows-2025 run 34934409445), and a subscribed Hup holds its
// handler forever. Removing it on the last subscription's Drop therefore deadlocked the
// cleanup a close handler exists to allow. With no subscription the installed handler
// returns FALSE, exactly as if it were absent, so older and newer host handlers and
// the default handler still run.
static REGISTRATION: Mutex<bool> = Mutex::new(false);
struct Ticket {
    signal: Signal,
    pending: AtomicBool,
    notifier: Notifier,
}
pub(super) struct Subscription {
    ticket: *mut Ticket,
    index: usize,
}

pub(super) fn dispatch(signal: Signal) -> bool {
    ACTIVE.fetch_add(1, Ordering::SeqCst);
    let mut handled = false;
    for slot in &SLOTS {
        let pointer = slot.load(Ordering::SeqCst);
        if pointer.is_null() {
            continue;
        }
        // SAFETY: removal nulls the slot before waiting for every active dispatcher;
        // the Box and its notifier remain live through this entire access.
        let ticket = unsafe { &*pointer };
        if ticket.signal == signal {
            handled = true;
            ticket.pending.store(true, Ordering::Release);
            let _ = ticket.notifier.notify();
        }
    }
    ACTIVE.fetch_sub(1, Ordering::SeqCst);
    handled
}
unsafe extern "system" fn handler(control: u32) -> i32 {
    handle_control(control, || {
        // Like libuv, keep the control thread alive so a later host turn can
        // observe Hup before Windows' bounded close timeout expires. This is
        // outside dispatch's ACTIVE guard: subscription teardown must not wait
        // for the lifetime of this sleeping control thread.
        // SAFETY: Sleep has no pointer/lifetime requirements; Windows owns this
        // handler thread and terminates the process after the close-time budget.
        unsafe {
            Sleep(INFINITE);
        }
    })
}
fn handle_control(control: u32, hold_close: impl FnOnce()) -> i32 {
    let signal = match control {
        CTRL_C_EVENT => Signal::Int,
        CTRL_BREAK_EVENT => Signal::Break,
        CTRL_CLOSE_EVENT => Signal::Hup,
        _ => return 0,
    };
    let handled = dispatch(signal);
    if control == CTRL_CLOSE_EVENT && handled {
        // Windows dispatches newest handler first. No supported API can both
        // continue to older unknown host handlers and hold this thread. We match
        // libuv only when Hup is subscribed; otherwise return FALSE to the host.
        // https://github.com/libuv/libuv/blob/v1.52.1/src/win/signal.c
        hold_close();
    }
    i32::from(handled)
}
impl Subscription {
    pub(super) fn new(signal: Signal, notifier: Notifier) -> Result<Self> {
        if !matches!(
            signal,
            Signal::Int | Signal::Break | Signal::Hup | Signal::WinCh
        ) {
            return Err(unsupported());
        }
        let mut installed = REGISTRATION.lock().unwrap_or_else(|e| e.into_inner());
        let index = SLOTS
            .iter()
            .position(|s| s.load(Ordering::SeqCst).is_null())
            .ok_or(Error::new(ErrorKind::ResourceLimit))?;
        if !*installed {
            // SAFETY: process-lifetime function pointer; handler has no locks or allocations.
            bool_result(unsafe { SetConsoleCtrlHandler(Some(handler), 1) })?;
            *installed = true;
        }
        let ticket = Box::into_raw(Box::new(Ticket {
            signal,
            pending: AtomicBool::new(false),
            notifier,
        }));
        SLOTS[index].store(ticket, Ordering::SeqCst);
        Ok(Self { ticket, index })
    }
    pub(super) fn ready(&self) -> bool {
        self.ticket().pending.load(Ordering::Acquire)
    }
    pub(super) fn take(&self) -> bool {
        self.ticket().pending.swap(false, Ordering::AcqRel)
    }
    fn ticket(&self) -> &Ticket {
        // SAFETY: this subscription owns the into_raw allocation until Drop joins
        // dispatchers. Every concurrent access is shared and fields are atomic.
        unsafe { &*self.ticket }
    }
}
impl Drop for Subscription {
    fn drop(&mut self) {
        // The installed handler stays registered (see REGISTRATION); unpublishing
        // the ticket is enough for it to stop claiming this signal.
        let _slots = REGISTRATION.lock().unwrap_or_else(|e| e.into_inner());
        SLOTS[self.index].store(ptr::null_mut(), Ordering::SeqCst);
        while ACTIVE.load(Ordering::SeqCst) != 0 {
            std::thread::yield_now();
        }
        // SAFETY: uniquely owned into_raw allocation, unpublished before all
        // dispatchers joined. No reference to this ticket can remain live.
        drop(unsafe { Box::from_raw(self.ticket) });
    }
}

#[cfg(all(test, not(loom)))]
mod tests {
    use super::*;
    #[test]
    fn close_dispatch_releases_tickets_before_holding_the_control_thread() {
        let driver = crate::Loop::new(crate::Config::default()).expect("notifier owner");
        let hup = Subscription::new(Signal::Hup, driver.notifier()).expect("Hup");
        let (entered, receive) = std::sync::mpsc::channel();
        let (release, wait) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            handle_control(CTRL_CLOSE_EVENT, || {
                entered.send(()).expect("hold entered after dispatch");
                wait.recv_timeout(std::time::Duration::from_secs(5))
                    .expect("test releases hold");
            })
        });
        receive
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("close dispatch ran");
        assert!(hup.take(), "Hup published before hold");
        assert!(!hup.take());
        drop(hup); // must not join the still-held handler thread
        assert!(!thread.is_finished());
        release.send(()).expect("release synthetic hold");
        assert_eq!(thread.join().expect("handler result"), 1);
        assert_eq!(
            handle_control(CTRL_CLOSE_EVENT, || panic!("unsubscribed close must chain")),
            0
        );
    }
    #[test]
    fn moved_subscription_keeps_its_published_ticket_alive() {
        let driver = crate::Loop::new(crate::Config::default()).expect("notifier owner");
        let first = Subscription::new(Signal::WinCh, driver.notifier()).expect("first");
        let second = Subscription::new(Signal::WinCh, driver.notifier()).expect("second");
        let mut subscriptions = vec![first];
        assert_eq!(subscriptions.capacity(), 1);
        subscriptions.push(second); // move both subscriptions by growing the vector
        assert!(subscriptions.capacity() > 1);
        let mut deliveries = 0;
        for _ in 0..100 {
            assert!(dispatch(Signal::WinCh));
            for subscription in &subscriptions {
                assert!(subscription.ready());
                assert!(subscription.take());
                assert!(!subscription.take());
                deliveries += 1;
            }
        }
        assert_eq!(deliveries, 200);
        drop(subscriptions);
        assert!(
            !dispatch(Signal::WinCh),
            "removed tickets cannot receive signals"
        );
    }
}
