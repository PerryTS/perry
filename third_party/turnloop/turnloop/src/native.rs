//! Portable names and options for native IPC, processes, signals and terminals.
use crate::Handle;
use std::{ffi::OsString, path::PathBuf};

/// Local IPC address: a filesystem socket path on Unix or named-pipe name on Windows.
/// Unix callers own removal of the socket path after closing the listener.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PipeName(pub PathBuf);

/// Standard stream to duplicate into a loop. Closing it does not close the host fd.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Stdio {
    /// Standard input.
    Stdin,
    /// Standard output.
    Stdout,
    /// Standard error.
    Stderr,
}

/// Child-side standard-stream configuration.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ProcessStdio {
    /// Inherit the corresponding host stream.
    #[default]
    Inherit,
    /// Connect to the platform null device.
    Null,
    /// Create a pipe; its parent end is returned in [`Process`].
    Pipe,
    /// Duplicate this loop-owned stream into the child.
    Handle(Handle),
}

/// Child launch options. Arguments exclude `argv[0]`, which is the program name.
/// Environment entries override inherited values unless `env_clear` is true.
#[derive(Clone, Debug)]
pub struct ProcessSpec {
    /// Executable path or name resolved using PATH.
    pub program: OsString,
    /// Arguments after `argv[0]`.
    pub args: Vec<OsString>,
    /// Environment additions or overrides.
    pub env: Vec<(OsString, OsString)>,
    /// Start from an empty environment.
    pub env_clear: bool,
    /// Child working directory; None inherits the host directory.
    pub cwd: Option<PathBuf>,
    /// Child stdin, stdout and stderr, in that order.
    pub stdio: [ProcessStdio; 3],
    /// Additional child descriptors beyond stdin/stdout/stderr, at fixed child
    /// descriptor numbers. This is the tail of Node's `stdio` array: entry
    /// `stdio[3]` of `['pipe','pipe','pipe','ipc']` is [`ChildFd`] number 3.
    /// Empty by default; [`Driver::spawn`](crate::Driver::spawn) refuses a
    /// non-empty list, because the parent ends have nowhere to go. Use
    /// [`Driver::spawn_extra`](crate::Driver::spawn_extra).
    pub extra: Vec<ChildFd>,
    /// Unix user ID; unsupported platforms reject this option.
    pub uid: Option<u32>,
    /// Unix group ID; unsupported platforms reject this option.
    pub gid: Option<u32>,
    /// Create an isolated process group (Windows Job Object) for tree termination.
    pub new_process_group: bool,
    /// Hide child windows on Windows (Node's `windowsHide`); ignored elsewhere.
    /// Defaults to false. With inherited stdio the console stays attached, even
    /// when true; with only pipes/null streams Windows uses CREATE_NO_WINDOW.
    pub windows_hide: bool,
    /// Prepare the child to outlive the parent process (Node's `detached`).
    /// Windows creates a detached process group and excludes it from the parent's
    /// lifetime job; Unix creates a new session and process group. Defaults false.
    /// This does not unref the child or change turnloop's explicit ownership:
    /// closing the child or dropping its owning loop still terminates a live child.
    pub detached: bool,
    /// Make the child's stdin its controlling terminal (Unix `TIOCSCTTY`), which
    /// is what a pty child needs so that job control, `/dev/tty` and terminal
    /// signals work inside it. Requires `detached`, because only a session leader
    /// with no controlling terminal may claim one; without it the spawn is
    /// `InvalidInput`. Windows has no equivalent and reports `Unsupported`.
    /// Defaults to false.
    pub controlling_terminal: bool,
}

/// Highest child descriptor number an extra [`ChildFd`] may use.
///
/// Node's `stdio` array indexes the child's own descriptors, so the numbers in
/// practice are small; the bound keeps the Windows inherited-descriptor block,
/// which is dense from 0 to the highest number in use, a fixed small size.
pub const MAX_CHILD_FD: u32 = 255;

/// One additional child descriptor beyond stdin, stdout and stderr.
///
/// The number is the child's own descriptor number, not the parent's: it is the
/// index into Node's `stdio` array, and is what a child reads out of a variable
/// such as `NODE_CHANNEL_FD`. turnloop never invents that variable; set it in
/// [`ProcessSpec::env`] alongside the descriptor.
///
/// On Windows the child descriptor is a C run-time descriptor, published in the
/// inherited-handle block every MSVCRT/UCRT program parses at startup, exactly
/// as libuv and Node publish theirs. A child that does not use the C run-time
/// sees the handle as inherited but has no number for it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChildFd {
    /// Child-side descriptor number, from 3 to 255. Numbers below 3 belong to
    /// [`ProcessSpec::stdio`], and a repeated number is `InvalidInput`.
    pub number: u32,
    /// What to place at that number.
    pub source: ChildFdSource,
}

/// Child-side configuration of one extra descriptor.
///
/// There is deliberately no `Inherit`: inheriting the host's own descriptor
/// number would mean clearing close-on-exec on a descriptor turnloop does not
/// own. Adopt it first ([`Detached::from_fd`](crate::Detached) on Unix) and pass
/// the resulting handle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChildFdSource {
    /// Connect to the platform null device, opened for reading and writing.
    Null,
    /// Create a one-way pipe that the child writes and the parent reads. The
    /// parent's readable end is returned.
    Pipe,
    /// Create a bidirectional stream pair. The parent's end is returned and is
    /// both readable and writable: a socket pair on Unix, a duplex named-pipe
    /// instance on Windows. This is what Node's `'pipe'` and `'ipc'` stdio
    /// entries are, and the only kind that can carry an IPC channel.
    Duplex,
    /// Duplicate this loop-owned transport into the child. The loop keeps its
    /// own end; the child receives an independent descriptor for it.
    Handle(Handle),
}
impl ProcessSpec {
    /// Launch a program with inherited environment, directory and standard streams.
    pub fn new(program: impl Into<OsString>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            env: Vec::new(),
            env_clear: false,
            cwd: None,
            stdio: [ProcessStdio::Inherit; 3],
            uid: None,
            gid: None,
            new_process_group: false,
            windows_hide: false,
            detached: false,
            controlling_terminal: false,
            extra: Vec::new(),
        }
    }
}

/// Owned child identity and optional parent pipe handles, all belonging to one loop.
#[derive(Clone, Copy, Debug)]
pub struct Process {
    /// Process handle; its exit operation carries the spawn token.
    pub handle: Handle,
    /// OS process identifier, for diagnostics rather than lifetime-safe signaling.
    pub pid: u32,
    /// Writable parent end of child stdin, if requested.
    pub stdin: Option<Handle>,
    /// Readable parent end of child stdout, if requested.
    pub stdout: Option<Handle>,
    /// Readable parent end of child stderr, if requested.
    pub stderr: Option<Handle>,
}
// Parent ends of extra descriptors are reported through the caller's slice in
// `Driver::spawn_extra`, which keeps this identity Copy and the driver free of
// a per-spawn allocation of its own.

/// Portable signal names. Unsupported mappings return an error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Signal {
    /// Interrupt (console Ctrl-C on Windows).
    Int,
    /// Terminate request; no Windows console event equivalent.
    Term,
    /// Unconditional process termination; cannot be subscribed.
    Kill,
    /// Hangup (best-effort console close on Windows).
    Hup,
    /// Child status change. Cooperates with the process dispatcher.
    Chld,
    /// Terminal window size change.
    WinCh,
    /// First user-defined Unix signal.
    Usr1,
    /// Second user-defined Unix signal.
    Usr2,
    /// Console break, where supported.
    Break,
}

/// Child termination status, preserving exit codes and signal termination.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExitStatus {
    /// Normal exit code, absent for signal termination.
    pub code: Option<i32>,
    /// Native terminating signal number, absent for normal termination.
    pub signal: Option<i32>,
}

/// Terminal input/output mode, restored when the owning handle closes or drops.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TtyMode {
    /// Restore the mode captured when the terminal was opened.
    Normal,
    /// Raw input, retaining terminal signal processing.
    Raw,
    /// Fully raw input/output, including disabling terminal-generated signals.
    Io,
}

/// Terminal dimensions in character cells.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowSize {
    /// Character columns.
    pub columns: u16,
    /// Character rows.
    pub rows: u16,
}
