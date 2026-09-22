//! turnloop P2: a child's readable pipes as loop entries.
//!
//! Split out of `reactor.rs` only to keep that file under the repository's
//! 2000-line cap (`scripts/check_file_size.sh`); the reader call sites are
//! still there and this is the same subject.
//!
//! See `docs/turnloop/p2-report.md` for what this deletes: two threads per
//! child with piped stdio, plus one per extra `stdio` descriptor.

use std::io::Read;

use super::{cp_live_lock, cp_push_event, CpEvent, CpReader};

/// One readable child pipe, still owned by its platform descriptor.
///
/// The reader used to be handed in as a `Box<dyn Read + Send>` because a
/// thread was going to block on it. P2 needs the descriptor itself, so the
/// concrete ownership is carried this far and only boxed on the fallback path.
pub(super) enum CpPipe {
    #[cfg(unix)]
    Fd(std::os::fd::OwnedFd),
    #[cfg(windows)]
    Handle(std::os::windows::io::OwnedHandle),
}

impl CpPipe {
    /// Turn the descriptor back into a blocking reader, for the thread path.
    fn into_reader(self) -> CpReader {
        match self {
            #[cfg(unix)]
            CpPipe::Fd(fd) => Box::new(std::fs::File::from(fd)) as CpReader,
            #[cfg(windows)]
            CpPipe::Handle(h) => Box::new(std::fs::File::from(h)) as CpReader,
        }
    }

    fn into_transport(self) -> crate::turnloop_proc::adopt::Transport {
        match self {
            #[cfg(unix)]
            CpPipe::Fd(fd) => crate::turnloop_proc::adopt::Transport::Fd(fd),
            #[cfg(windows)]
            CpPipe::Handle(h) => crate::turnloop_proc::adopt::Transport::Handle(h),
        }
    }
}

/// `ChildStdout` / `ChildStderr` / a raw `stdio` fd, as the descriptor the
/// loop can adopt. `std` owns these conversions on both platforms, so nothing
/// here duplicates a descriptor or guesses at its ownership.
pub(super) fn cp_pipe_from_child_stdout(pipe: std::process::ChildStdout) -> CpPipe {
    #[cfg(unix)]
    {
        CpPipe::Fd(std::os::fd::OwnedFd::from(pipe))
    }
    #[cfg(windows)]
    {
        CpPipe::Handle(std::os::windows::io::OwnedHandle::from(pipe))
    }
}

pub(super) fn cp_pipe_from_child_stderr(pipe: std::process::ChildStderr) -> CpPipe {
    #[cfg(unix)]
    {
        CpPipe::Fd(std::os::fd::OwnedFd::from(pipe))
    }
    #[cfg(windows)]
    {
        CpPipe::Handle(std::os::windows::io::OwnedHandle::from(pipe))
    }
}

pub(super) fn cp_pipe_from_file(file: std::fs::File) -> CpPipe {
    #[cfg(unix)]
    {
        CpPipe::Fd(std::os::fd::OwnedFd::from(file))
    }
    #[cfg(windows)]
    {
        CpPipe::Handle(std::os::windows::io::OwnedHandle::from(file))
    }
}

/// Stream a child's readable pipe into the event queue until EOF.
///
/// turnloop P2: the pipe is adopted by this agent's loop and read multishot,
/// so the per-pipe reader thread — two per child with piped stdio, plus one
/// per extra `stdio` fd — is gone. The completions land on the thread that
/// owns the JS heap and push the *same* [`CpEvent::Data`] / [`CpEvent::Eof`]
/// the thread pushed, so `cp_reactor_pump` and every event-ordering rule it
/// implements are untouched.
///
/// The thread survives as the fallback for an agent with no loop (a
/// `worker_threads` agent before P3/P4, or a host where loop creation
/// failed) — the P1 coexistence rule. Adoption moves the descriptor, so the
/// fallback reconstructs the reader from it rather than from a copy: there
/// is exactly one owner at every instant.
pub(super) fn cp_spawn_reader(handle: u64, pipe: CpPipe, fd: usize) {
    if crate::turnloop_proc::available() {
        let transport = pipe.into_transport();
        match crate::turnloop_proc::adopt_stream(
            transport,
            crate::turnloop_proc::Owner::ChildStream { child: handle, fd },
        ) {
            Ok(id) => {
                if crate::turnloop_proc::read_start(id).is_ok() {
                    cp_record_loop_stream(handle, fd, id);
                    return;
                }
                // Adopted but unreadable: the descriptor now belongs to the
                // driver, so it must be released there, and the child simply
                // sees EOF on that stream — the same outcome the thread's
                // `Err(_)` arm produced.
                crate::turnloop_proc::close(id);
                cp_push_event(CpEvent::Eof { handle, fd });
                return;
            }
            Err(_) => {
                // `adopt_stream` consumed and closed the descriptor; there is
                // nothing left to read, so report EOF rather than pretending.
                cp_push_event(CpEvent::Eof { handle, fd });
                return;
            }
        }
    }
    cp_spawn_reader_thread(handle, pipe.into_reader(), fd);
}

/// The pre-P2 transport, kept for agents with no loop.
fn cp_spawn_reader_thread<R: Read + Send + 'static>(handle: u64, mut pipe: R, fd: usize) {
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match pipe.read(&mut buf) {
                Ok(0) | Err(_) => {
                    cp_push_event(CpEvent::Eof { handle, fd });
                    break;
                }
                Ok(n) => {
                    cp_push_event(CpEvent::Data {
                        handle,
                        fd,
                        bytes: buf[..n].to_vec(),
                    });
                }
            }
        }
    });
}

/// Remember which loop entry carries a child's stream, so it can be closed at
/// EOF and at teardown.
fn cp_record_loop_stream(handle: u64, fd: usize, id: u64) {
    if let Some(lc) = cp_live_lock().as_mut().and_then(|map| map.get_mut(&handle)) {
        lc.loop_streams.push((fd, id));
    } else {
        // The child is already gone (a spawn that failed between registration
        // and here): release the entry rather than leaking the descriptor.
        crate::turnloop_proc::close(id);
    }
}

fn cp_take_loop_stream(handle: u64, fd: usize) -> Option<u64> {
    let mut guard = cp_live_lock();
    let lc = guard.as_mut()?.get_mut(&handle)?;
    let at = lc.loop_streams.iter().position(|(at, _)| *at == fd)?;
    Some(lc.loop_streams.remove(at).1)
}

/// One turnloop completion for a child's readable pipe.
///
/// A deliberate 1:1 translation of the deleted thread's loop body: bytes
/// become [`CpEvent::Data`], and EOF *or any read failure* becomes
/// [`CpEvent::Eof`] — the thread's `Ok(0) | Err(_)` arm made no distinction
/// either, and Node does not surface a read error on a child's stdout.
pub(crate) fn on_stream_completion(
    handle: u64,
    fd: usize,
    event: crate::turnloop_proc::StreamEvent,
) {
    use crate::turnloop_proc::StreamEvent;
    match event {
        StreamEvent::Data(bytes) => cp_push_event(CpEvent::Data { handle, fd, bytes }),
        StreamEvent::Eof => {
            if let Some(id) = cp_take_loop_stream(handle, fd) {
                crate::turnloop_proc::close(id);
            }
            cp_push_event(CpEvent::Eof { handle, fd });
        }
        StreamEvent::Error { terminal: true, .. } => {
            if let Some(id) = cp_take_loop_stream(handle, fd) {
                crate::turnloop_proc::close(id);
            }
            cp_push_event(CpEvent::Eof { handle, fd });
        }
        // A transient read failure does not end the stream, and neither the
        // driver nor Node treats it as EOF.
        StreamEvent::Error { .. } => {}
        StreamEvent::Closed
        | StreamEvent::Wrote { .. }
        | StreamEvent::Datagram { .. }
        | StreamEvent::Signal => {}
    }
}

/// Release every loop entry a child still owns. Called once the child has
/// fully closed, so a program that spawns in a loop cannot accumulate
/// descriptors the driver still holds.
pub(crate) fn cp_release_loop_streams(handle: u64) {
    let ids: Vec<u64> = {
        let mut guard = cp_live_lock();
        match guard.as_mut().and_then(|map| map.get_mut(&handle)) {
            Some(lc) => lc.loop_streams.drain(..).map(|(_, id)| id).collect(),
            None => Vec::new(),
        }
    };
    for id in ids {
        crate::turnloop_proc::close(id);
    }
}
