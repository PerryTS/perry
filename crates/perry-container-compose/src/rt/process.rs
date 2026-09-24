//! Child processes on turnloop: the `docker` / `podman` / `container` CLI
//! invocations every backend operation is made of.
//!
//! The shape is `std::process::Command`'s (and tokio's): build with
//! [`Command::new`] / [`Command::arg`] / [`Command::args`], then await
//! [`Command::output`] (stdin null, stdout and stderr captured) or
//! [`Command::status`] (all three inherited). The spawn happens on the first
//! poll, inside the enclosing [`super::block_on`]'s loop; the output future
//! completes once the child has been reaped AND both pipes reached EOF, so no
//! trailing output is lost.
//!
//! **Drop terminates.** Dropping a future before it completes closes the
//! process handle, which turnloop turns into terminating the child and then
//! reaping it. That is what makes `timeout(d, cmd.output())` an actual abort:
//! tokio's `output()` future (without `kill_on_drop`) left a hung CLI running
//! after the timeout fired.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use turnloop::{Handle, ProcessSpec, ProcessStdio, Token};

use super::{io_error, try_with_reactor, with_current, with_reactor, Event, Reactor};

/// A child-process builder.
#[derive(Clone, Debug)]
pub struct Command {
    program: OsString,
    args: Vec<OsString>,
}

impl Command {
    /// Launch `program`, resolved through `PATH` when it has no separator.
    pub fn new(program: impl AsRef<OsStr>) -> Self {
        Self {
            program: program.as_ref().to_owned(),
            args: Vec::new(),
        }
    }

    /// Append one argument.
    pub fn arg(&mut self, arg: impl AsRef<OsStr>) -> &mut Self {
        self.args.push(arg.as_ref().to_owned());
        self
    }

    /// Append arguments.
    pub fn args<I, S>(&mut self, args: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.args
            .extend(args.into_iter().map(|a| a.as_ref().to_owned()));
        self
    }

    fn spec(&self, stdio: [ProcessStdio; 3]) -> ProcessSpec {
        let mut spec = ProcessSpec::new(self.program.clone());
        spec.args = self.args.clone();
        spec.stdio = stdio;
        spec
    }

    /// Run to completion with stdin null and stdout / stderr captured.
    pub fn output(&mut self) -> OutputFuture {
        OutputFuture {
            inner: Child::new(self.spec([
                ProcessStdio::Null,
                ProcessStdio::Pipe,
                ProcessStdio::Pipe,
            ])),
        }
    }

    /// Run to completion with all three streams inherited.
    pub fn status(&mut self) -> StatusFuture {
        StatusFuture {
            inner: Child::new(self.spec([ProcessStdio::Inherit; 3])),
        }
    }
}

/// A child's exit status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExitStatus {
    code: Option<i32>,
    signal: Option<i32>,
}

impl ExitStatus {
    /// Whether the child exited normally with code 0.
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }

    /// The exit code; `None` when the child was terminated by a signal.
    pub fn code(&self) -> Option<i32> {
        self.code
    }

    /// The terminating signal number, when there was one.
    pub fn signal(&self) -> Option<i32> {
        self.signal
    }
}

impl From<turnloop::ExitStatus> for ExitStatus {
    fn from(status: turnloop::ExitStatus) -> Self {
        Self {
            code: status.code,
            signal: status.signal,
        }
    }
}

impl fmt::Display for ExitStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.code, self.signal) {
            (Some(code), _) => write!(f, "exit status: {code}"),
            (None, Some(signal)) => write!(f, "signal: {signal}"),
            (None, None) => f.write_str("unknown exit status"),
        }
    }
}

/// A finished child's status and captured output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Output {
    /// The reaped exit status.
    pub status: ExitStatus,
    /// Everything the child wrote to stdout.
    pub stdout: Vec<u8>,
    /// Everything the child wrote to stderr.
    pub stderr: Vec<u8>,
}

/// One captured pipe.
struct Pipe {
    handle: Handle,
    token: u64,
    bytes: Vec<u8>,
    done: bool,
}

/// A spawned child, owned by one reactor.
struct Running {
    reactor: u64,
    process: Handle,
    exit_token: u64,
    status: Option<ExitStatus>,
    pipes: Vec<Pipe>,
}

impl Running {
    fn start(reactor: &mut Reactor, spec: &ProcessSpec) -> io::Result<Self> {
        let exit_token = reactor.register();
        let process = match reactor.driver.spawn(spec, Token(exit_token)) {
            Ok(process) => process,
            Err(e) => {
                reactor.forget(exit_token);
                return Err(io_error(e));
            }
        };
        let mut running = Self {
            reactor: reactor.id(),
            process: process.handle,
            exit_token,
            status: None,
            pipes: Vec::new(),
        };
        for handle in [process.stdout, process.stderr].into_iter().flatten() {
            let token = reactor.register();
            running.pipes.push(Pipe {
                handle,
                token,
                bytes: Vec::new(),
                done: false,
            });
            if let Err(e) = reactor.driver.read_start(handle, Token(token)) {
                running.release(reactor);
                return Err(io_error(e));
            }
        }
        // turnloop never hands back a stdin pipe we did not ask for, but a
        // spec that did ask would leave the child waiting on it forever.
        if let Some(stdin) = process.stdin {
            reactor.close(stdin);
        }
        Ok(running)
    }

    /// Drain routed events; `Ok(true)` once exited and every pipe hit EOF.
    fn advance(&mut self, reactor: &mut Reactor, cx: &Context<'_>) -> io::Result<bool> {
        while self.status.is_none() {
            match reactor.take_event(self.exit_token, cx) {
                Some(Event::Exited(status)) => self.status = Some(status.into()),
                Some(Event::Failed(e)) => return Err(io_error(e)),
                Some(_) => {}
                None => break,
            }
        }
        for pipe in &mut self.pipes {
            while !pipe.done {
                match reactor.take_event(pipe.token, cx) {
                    Some(Event::Read(bytes)) => pipe.bytes.extend_from_slice(&bytes),
                    Some(Event::Eof) => pipe.done = true,
                    Some(Event::Failed(e)) => return Err(io_error(e)),
                    Some(_) => {}
                    None => break,
                }
            }
        }
        Ok(self.status.is_some() && self.pipes.iter().all(|p| p.done))
    }

    /// Stop routing and close every handle. Closing a live process handle
    /// terminates the child; closing a reaped one just releases it.
    fn release(&mut self, reactor: &mut Reactor) {
        reactor.forget(self.exit_token);
        reactor.close(self.process);
        for pipe in &self.pipes {
            reactor.forget(pipe.token);
            reactor.close(pipe.handle);
        }
    }
}

enum Child {
    Pending(ProcessSpec),
    Running(Running),
    Done,
}

impl Child {
    fn new(spec: ProcessSpec) -> Self {
        Self::Pending(spec)
    }

    fn poll(&mut self, cx: &Context<'_>) -> Poll<io::Result<(ExitStatus, Vec<u8>, Vec<u8>)>> {
        if let Self::Pending(spec) = self {
            match with_current(|reactor| Running::start(reactor, spec)) {
                Ok(running) => *self = Self::Running(running),
                Err(e) => {
                    *self = Self::Done;
                    return Poll::Ready(Err(e));
                }
            }
        }
        let Self::Running(running) = self else {
            panic!("perry_container_compose::rt::Command future polled after completion");
        };
        let result = with_reactor(running.reactor, |reactor| {
            let finished = running.advance(reactor, cx);
            if !matches!(finished, Ok(false)) {
                running.release(reactor);
            }
            finished
        });
        match result {
            Ok(false) => Poll::Pending,
            Ok(true) => {
                // Already released inside the reactor call above.
                let status = running.status.expect("finished implies exited");
                let mut pipes = std::mem::take(&mut running.pipes)
                    .into_iter()
                    .map(|p| p.bytes);
                let stdout = pipes.next().unwrap_or_default();
                let stderr = pipes.next().unwrap_or_default();
                *self = Self::Done;
                Poll::Ready(Ok((status, stdout, stderr)))
            }
            Err(e) => {
                *self = Self::Done;
                Poll::Ready(Err(e))
            }
        }
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        if let Self::Running(running) = self {
            let id = running.reactor;
            try_with_reactor(id, |reactor| running.release(reactor));
        }
    }
}

/// Future returned by [`Command::output`].
#[must_use = "futures do nothing unless awaited"]
pub struct OutputFuture {
    inner: Child,
}

impl Future for OutputFuture {
    type Output = io::Result<Output>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.get_mut().inner.poll(cx).map(|result| {
            result.map(|(status, stdout, stderr)| Output {
                status,
                stdout,
                stderr,
            })
        })
    }
}

/// Future returned by [`Command::status`].
#[must_use = "futures do nothing unless awaited"]
pub struct StatusFuture {
    inner: Child,
}

impl Future for StatusFuture {
    type Output = io::Result<ExitStatus>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.get_mut()
            .inner
            .poll(cx)
            .map(|result| result.map(|(status, _, _)| status))
    }
}
