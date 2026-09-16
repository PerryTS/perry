//! The crate's background async driver — one parked thread, no tokio.
//!
//! Two of this crate's features are long-lived D-Bus services: the
//! StatusNotifierItem tray (`tray.rs`, via `ksni`) and the MPRIS media
//! player (`media_playback.rs`, via `mpris-server`). Both are async, and
//! until the turnloop migration both reached for tokio — `ksni` and
//! `mpris-server` were pulled in with their `tokio` features and this
//! crate carried a direct `tokio` dependency purely to own a runtime for
//! them.
//!
//! Neither crate needs it:
//!
//! * `ksni`'s `async-io` feature is a full alternative to its `tokio`
//!   default (the two are mutually exclusive — `ksni::compat` has a
//!   `compile_error!` if both are on). In that mode `ksni` carries its
//!   OWN `async_executor`-backed driver thread, started on demand, and
//!   builds its zbus connection with `internal_executor(false)` so the
//!   connection is ticked on that same thread. Nothing has to supply it
//!   with a runtime.
//! * `mpris-server`'s `tokio` feature is opt-in (it only forwards to
//!   `zbus/tokio`) and is not in its defaults. With it off, zbus runs on
//!   `async-io` and starts its own `zbus::Connection executor` thread
//!   for the connection's socket task.
//!
//! What is left over is the small amount of driving Perry does itself:
//!
//! * [`block_on`] — run a future to completion on the CALLING thread.
//!   Used for the one-shot handshakes (`ksni`'s `spawn()`,
//!   `mpris_server::Server::new()`) that the old code ran through
//!   `Runtime::block_on`, so the thread each of those blocks is
//!   unchanged. `clipboard.rs` does the same thing through glib's
//!   `MainContext::block_on`; this is the equivalent for a future that
//!   is not tied to the GTK main context.
//! * [`spawn_detached`] — fire-and-forget, the replacement for
//!   `Runtime::spawn`. The tray's property refreshes must not block the
//!   GTK main thread (they wait on `ksni`'s service loop, which is a
//!   D-Bus round trip), and they must still run when no GTK main loop is
//!   turning yet — a tray icon can be configured before `app.run()`.
//!   That rules out glib's `MainContext::spawn_local`, so this module
//!   keeps one thread of its own.
//!
//! The thread is started lazily on the first [`spawn_detached`] call and
//! then parks in `Executor::run` forever, which is what the old
//! `Builder::new_multi_thread().worker_threads(1)` runtime did. A
//! program that never creates a tray never starts it.

#![cfg(target_os = "linux")]

use async_executor::Executor;
use futures_lite::FutureExt;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::sync::OnceLock;

/// The shared executor, and the thread that ticks it.
///
/// `None` means the driver thread could not be started; callers degrade
/// to "the update does not happen" rather than failing the JS call, the
/// same way the old code degraded when `Runtime` construction failed.
fn executor() -> Option<&'static Executor<'static>> {
    // `Executor::new` is `const`, so this needs no allocation and no
    // `OnceLock` of its own — only the thread has to be started once.
    static EXECUTOR: Executor<'static> = Executor::new();
    static STARTED: OnceLock<bool> = OnceLock::new();

    let started = *STARTED.get_or_init(|| {
        match std::thread::Builder::new()
            .name("perry-ui-async".into())
            .spawn(|| {
                // Runs until the process exits. `pending()` never
                // resolves, so `run` keeps polling whatever has been
                // spawned and parks when there is nothing to do.
                futures_lite::future::block_on(EXECUTOR.run(futures_lite::future::pending::<()>()));
            }) {
            Ok(_) => true,
            Err(e) => {
                eprintln!(
                    "[perry] warning: could not start the perry-ui-async thread: {e} \
                     — tray icon updates and MPRIS shutdown will not be delivered"
                );
                false
            }
        }
    });
    started.then_some(&EXECUTOR)
}

/// Run `future` in the background and forget about it.
///
/// The replacement for `tokio::runtime::Runtime::spawn`. Safe to call
/// from any thread, including the GTK main thread; it never blocks.
///
/// A panicking task is caught here rather than allowed to unwind out of
/// `Executor::run`, which would kill the driver thread for the rest of
/// the process and leave every later `spawn_detached` queued behind a
/// runner that no longer exists — a tray that silently stops updating.
/// tokio isolated a panicking task to that task; so does this.
pub(crate) fn spawn_detached(future: impl Future<Output = ()> + Send + 'static) {
    let Some(executor) = executor() else {
        return;
    };
    executor
        .spawn(async move {
            if AssertUnwindSafe(future).catch_unwind().await.is_err() {
                eprintln!("[perry] warning: a perry-ui background task panicked");
            }
        })
        .detach();
}

/// Run `future` to completion on the calling thread.
///
/// The replacement for `tokio::runtime::Runtime::block_on`. I/O inside
/// the future is still serviced: `async-io` runs a reactor thread of its
/// own, and both `ksni` and zbus tick their connections on their own
/// threads, so this only has to drive the future's own state machine.
pub(crate) fn block_on<T>(future: impl Future<Output = T>) -> T {
    futures_lite::future::block_on(future)
}
