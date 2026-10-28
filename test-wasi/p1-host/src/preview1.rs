//! A minimal `wasi_snapshot_preview1` on the guest's own `Caller`.
//!
//! Only what a networking program needs: process, clock, random, stdio,
//! sockets and `poll_oneoff`. There are no pre-opened directories and no
//! filesystem. Imports not defined here are bound as traps by the caller, so a
//! program that reaches an unimplemented call fails loudly.

use crate::fdtable::{rights, Fd, Table};
use std::fmt;
use std::os::fd::{FromRawFd, OwnedFd};
use wasmtime::{AsContextMut, Caller, Extern, Linker};

pub mod errno {
    pub const SUCCESS: u16 = 0;
    pub const AGAIN: u16 = 6;
    pub const BADF: u16 = 8;
    pub const CONNRESET: u16 = 15;
    pub const FAULT: u16 = 21;
    pub const INTR: u16 = 27;
    pub const INVAL: u16 = 28;
    pub const IO: u16 = 29;
    pub const NOTCONN: u16 = 53;
    pub const NOTSOCK: u16 = 57;
    pub const PIPE: u16 = 64;
    pub const SPIPE: u16 = 70;
    pub const NOTCAPABLE: u16 = 76;
}

const FDFLAG_NONBLOCK: u32 = 4;
const EVENT_CLOCK: u8 = 0;
const EVENT_FD_READ: u8 = 1;
const EVENT_FD_WRITE: u8 = 2;
const EVENT_HANGUP: u16 = 1;
const SUB_SIZE: usize = 48;
const EVENT_SIZE: usize = 32;

/// Raised by `proc_exit` to unwind the guest with a status.
#[derive(Debug)]
pub struct ExitCode(pub i32);

impl fmt::Display for ExitCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "exit {}", self.0)
    }
}

impl std::error::Error for ExitCode {}

/// What the host observed, written out for the test driver.
#[derive(Default)]
pub struct Stats {
    /// Every `poll_oneoff` the guest made.
    pub polls: u64,
    /// Polls that returned only because a clock subscription expired.
    pub clock_wakeups: u64,
    /// Polls that returned with at least one descriptor event.
    pub fd_wakeups: u64,
    /// The most subscriptions in a single poll.
    pub max_subs: u32,
    pub prestat_calls: u64,
    /// The descriptor number of every accepted connection, in order.
    pub accepts: Vec<u32>,
}

pub struct Host {
    pub args: Vec<String>,
    pub env: Vec<String>,
    pub table: Table,
    pub stats: Stats,
    random: Option<std::fs::File>,
}

impl Host {
    pub fn new(args: Vec<String>, env: Vec<String>, table: Table) -> Self {
        Host {
            args,
            env,
            table,
            stats: Stats::default(),
            random: None,
        }
    }
}

type Res<T = ()> = Result<T, u16>;

fn os_errno() -> u16 {
    match std::io::Error::last_os_error().raw_os_error().unwrap_or(0) {
        libc::EAGAIN => errno::AGAIN,
        libc::EBADF => errno::BADF,
        libc::ECONNRESET | libc::ECONNABORTED => errno::CONNRESET,
        libc::EINTR => errno::INTR,
        libc::EINVAL => errno::INVAL,
        libc::ENOTCONN => errno::NOTCONN,
        libc::ENOTSOCK => errno::NOTSOCK,
        libc::EPIPE => errno::PIPE,
        _ => errno::IO,
    }
}

/// The guest memory and the host state, borrowed together from a call.
fn split<'a>(caller: &'a mut Caller<'_, Host>) -> Res<(&'a mut [u8], &'a mut Host)> {
    let Some(Extern::Memory(memory)) = caller.get_export("memory") else {
        return Err(errno::FAULT);
    };
    Ok(memory.data_and_store_mut(caller.as_context_mut()))
}

/// The guest range `ptr..ptr + len`, or `FAULT` when it overflows or leaves
/// the memory. Every guest pointer plus length goes through here, so no guest
/// value reaches an unchecked addition.
fn span(mem: &[u8], ptr: u32, len: usize) -> Res<std::ops::Range<usize>> {
    let start = ptr as usize;
    let end = start.checked_add(len).ok_or(errno::FAULT)?;
    if end > mem.len() {
        return Err(errno::FAULT);
    }
    Ok(start..end)
}

/// The guest address `base + offset`, or `FAULT` past the 32-bit space.
fn at(base: u32, offset: usize) -> Res<u32> {
    u32::try_from(offset)
        .ok()
        .and_then(|offset| base.checked_add(offset))
        .ok_or(errno::FAULT)
}

fn put(mem: &mut [u8], ptr: u32, bytes: &[u8]) -> Res {
    let r = span(mem, ptr, bytes.len())?;
    mem[r].copy_from_slice(bytes);
    Ok(())
}

fn put_u32(mem: &mut [u8], ptr: u32, v: u32) -> Res {
    put(mem, ptr, &v.to_le_bytes())
}

fn put_u64(mem: &mut [u8], ptr: u32, v: u64) -> Res {
    put(mem, ptr, &v.to_le_bytes())
}

fn code(result: Res) -> i32 {
    match result {
        Ok(()) => errno::SUCCESS as i32,
        Err(e) => e as i32,
    }
}

/// The OS descriptor behind guest descriptor `fd`, if it holds every right in
/// `need`; `BADF` for an unknown number, `NOTCAPABLE` for a missing right.
fn lookup(host: &Host, fd: u32, need: u64) -> Res<&Fd> {
    let entry = host.table.get(fd).ok_or(errno::BADF)?;
    if entry.rights() & need != need {
        return Err(errno::NOTCAPABLE);
    }
    Ok(entry)
}

fn iovecs(mem: &mut [u8], ptr: u32, count: u32) -> Res<Vec<libc::iovec>> {
    // The whole vector array is checked before any entry is decoded.
    let table = span(mem, ptr, (count as usize).saturating_mul(8))?;
    let base = mem.as_mut_ptr();
    let mut out = Vec::new();
    for entry in table.step_by(8) {
        let word = |i: usize| u32::from_le_bytes(mem[i..i + 4].try_into().unwrap());
        let (buf, len) = (word(entry), word(entry + 4));
        let range = span(mem, buf, len as usize)?;
        out.push(libc::iovec {
            iov_base: unsafe { base.add(range.start) }.cast(),
            iov_len: len as usize,
        });
    }
    Ok(out)
}

fn transfer(
    caller: &mut Caller<'_, Host>,
    fd: u32,
    iovs: u32,
    count: u32,
    out: u32,
    write: bool,
    stream: bool,
) -> Res {
    let (mem, host) = split(caller)?;
    let need = if write {
        rights::FD_WRITE
    } else {
        rights::FD_READ
    };
    let entry = lookup(host, fd, need)?;
    if stream && !matches!(entry, Fd::Stream(_)) {
        return Err(errno::NOTSOCK);
    }
    let os = entry.os();
    let vecs = iovecs(mem, iovs, count)?;
    let n = unsafe {
        if write {
            libc::writev(os, vecs.as_ptr(), vecs.len() as i32)
        } else {
            libc::readv(os, vecs.as_ptr(), vecs.len() as i32)
        }
    };
    if n < 0 {
        return Err(os_errno());
    }
    put_u32(mem, out, n as u32)
}

fn set_nonblock(os: i32, on: bool) -> Res {
    unsafe {
        let flags = libc::fcntl(os, libc::F_GETFL);
        if flags < 0 {
            return Err(os_errno());
        }
        let next = if on {
            flags | libc::O_NONBLOCK
        } else {
            flags & !libc::O_NONBLOCK
        };
        if next != flags && libc::fcntl(os, libc::F_SETFL, next) < 0 {
            return Err(os_errno());
        }
    }
    Ok(())
}

fn clock_ns(id: u32) -> Res<u64> {
    let clock = match id {
        0 => libc::CLOCK_REALTIME,
        1 => libc::CLOCK_MONOTONIC,
        2 => libc::CLOCK_PROCESS_CPUTIME_ID,
        3 => libc::CLOCK_THREAD_CPUTIME_ID,
        _ => return Err(errno::INVAL),
    };
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    if unsafe { libc::clock_gettime(clock, &mut ts) } != 0 {
        return Err(os_errno());
    }
    Ok(ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64)
}

fn list_sizes(caller: &mut Caller<'_, Host>, count_ptr: u32, size_ptr: u32, env: bool) -> Res {
    let (mem, host) = split(caller)?;
    let items = if env { &host.env } else { &host.args };
    let size: usize = items.iter().map(|s| s.len() + 1).sum();
    let count = items.len() as u32;
    put_u32(mem, count_ptr, count)?;
    put_u32(mem, size_ptr, size as u32)
}

fn list_get(caller: &mut Caller<'_, Host>, ptrs: u32, buf: u32, env: bool) -> Res {
    let (mem, host) = split(caller)?;
    let items = if env { &host.env } else { &host.args };
    let mut to = buf;
    for (i, item) in items.iter().enumerate() {
        put_u32(mem, at(ptrs, 4 * i)?, to)?;
        put(mem, to, item.as_bytes())?;
        put(mem, at(to, item.len())?, &[0])?;
        to = at(to, item.len() + 1)?;
    }
    Ok(())
}

fn sock_accept(caller: &mut Caller<'_, Host>, fd: u32, flags: u32, out: u32) -> Res {
    let (mem, host) = split(caller)?;
    let entry = lookup(host, fd, rights::SOCK_ACCEPT)?;
    if !matches!(entry, Fd::Listener(_)) {
        return Err(errno::NOTSOCK);
    }
    // Blocks when the guest left its listener blocking; the guest owns that.
    let accepted = unsafe { libc::accept(entry.os(), std::ptr::null_mut(), std::ptr::null_mut()) };
    if accepted < 0 {
        return Err(os_errno());
    }
    let owned = unsafe { OwnedFd::from_raw_fd(accepted) };
    // Only the accepted connection takes `flags`; the listener is untouched.
    set_nonblock(accepted, flags & FDFLAG_NONBLOCK != 0)?;
    let number = host.table.insert(Fd::Stream(owned));
    host.stats.accepts.push(number);
    put_u32(mem, out, number)
}

fn sock_shutdown(caller: &mut Caller<'_, Host>, fd: u32, how: u32) -> Res {
    let (_, host) = split(caller)?;
    let entry = lookup(host, fd, rights::SOCK_SHUTDOWN)?;
    if !matches!(entry, Fd::Stream(_)) {
        return Err(errno::NOTSOCK);
    }
    let how = match how {
        1 => libc::SHUT_RD,
        2 => libc::SHUT_WR,
        3 => libc::SHUT_RDWR,
        _ => return Err(errno::INVAL),
    };
    if unsafe { libc::shutdown(entry.os(), how) } != 0 {
        return Err(os_errno());
    }
    Ok(())
}

fn fd_fdstat_get(caller: &mut Caller<'_, Host>, fd: u32, out: u32) -> Res {
    let (mem, host) = split(caller)?;
    let entry = host.table.get(fd).ok_or(errno::BADF)?;
    let nonblock = unsafe { libc::fcntl(entry.os(), libc::F_GETFL) } & libc::O_NONBLOCK != 0;
    let mut stat = [0u8; 24];
    stat[0] = entry.filetype();
    stat[2..4].copy_from_slice(&(if nonblock { FDFLAG_NONBLOCK as u16 } else { 0 }).to_le_bytes());
    stat[8..16].copy_from_slice(&entry.rights().to_le_bytes());
    put(mem, out, &stat)
}

fn fd_fdstat_set_flags(caller: &mut Caller<'_, Host>, fd: u32, flags: u32) -> Res {
    let (_, host) = split(caller)?;
    let entry = lookup(host, fd, rights::FD_FDSTAT_SET_FLAGS)?;
    set_nonblock(entry.os(), flags & FDFLAG_NONBLOCK != 0)
}

fn fd_filestat_get(caller: &mut Caller<'_, Host>, fd: u32, out: u32) -> Res {
    let (mem, host) = split(caller)?;
    let entry = host.table.get(fd).ok_or(errno::BADF)?;
    let mut stat = [0u8; 64];
    stat[16] = entry.filetype();
    stat[24] = 1;
    put(mem, out, &stat)
}

fn random_get(caller: &mut Caller<'_, Host>, buf: u32, len: u32) -> Res {
    use std::io::Read;
    let (mem, host) = split(caller)?;
    let range = span(mem, buf, len as usize)?;
    if host.random.is_none() {
        host.random = Some(std::fs::File::open("/dev/urandom").map_err(|_| errno::IO)?);
    }
    host.random
        .as_mut()
        .unwrap()
        .read_exact(&mut mem[range])
        .map_err(|_| errno::IO)
}

/// The `fd_readwrite.nbytes` of a ready descriptor. A readable stream or pipe
/// reports the bytes `FIONREAD` says are queued (0 at end of file); a readable
/// listener reports 1, one pending connection being all that is known. A
/// writable descriptor reports 1: the room left in the kernel's send buffer is
/// not worth a system call here, and any nonzero value tells a guest it may
/// write.
fn ready_bytes(os: i32, write: bool, listener: bool) -> u64 {
    if write || listener {
        return 1;
    }
    let mut queued: libc::c_int = 0;
    if unsafe { libc::ioctl(os, libc::FIONREAD as _, &mut queued) } != 0 {
        return 0;
    }
    queued.max(0) as u64
}

/// One subscription of `poll_oneoff`.
enum Sub {
    Clock { remaining_ns: u64 },
    Fd { fd: u32, write: bool },
}

fn poll_oneoff(
    caller: &mut Caller<'_, Host>,
    input: u32,
    output: u32,
    count: u32,
    nevents: u32,
) -> Res {
    let (mem, host) = split(caller)?;
    if count == 0 {
        return Err(errno::INVAL);
    }
    // Both arrays are checked whole, with checked arithmetic, before any
    // subscription is decoded or any event written.
    let raw = span(mem, input, (count as usize).saturating_mul(SUB_SIZE))?;
    span(mem, output, (count as usize).saturating_mul(EVENT_SIZE))?;
    span(mem, nevents, 4)?;
    let mut subs = Vec::new();
    let mut users = Vec::new();
    for sub in mem[raw].chunks_exact(SUB_SIZE) {
        let u64_at = |i: usize| u64::from_le_bytes(sub[i..i + 8].try_into().unwrap());
        let u32_at = |i: usize| u32::from_le_bytes(sub[i..i + 4].try_into().unwrap());
        users.push(u64_at(0));
        subs.push(match sub[8] {
            EVENT_CLOCK => {
                let (id, timeout, flags) = (u32_at(16), u64_at(24), u32_at(40) & 0xffff);
                let remaining_ns = if flags & 1 != 0 {
                    timeout.saturating_sub(clock_ns(id)?)
                } else {
                    timeout
                };
                Sub::Clock { remaining_ns }
            }
            tag @ (EVENT_FD_READ | EVENT_FD_WRITE) => Sub::Fd {
                fd: u32_at(16),
                write: tag == EVENT_FD_WRITE,
            },
            _ => return Err(errno::INVAL),
        });
    }
    let started = clock_ns(1)?;
    let soonest = subs
        .iter()
        .filter_map(|s| match s {
            Sub::Clock { remaining_ns } => Some(*remaining_ns),
            _ => None,
        })
        .min();
    // (subscription index, errno, event type, hangup, nbytes)
    let mut events: Vec<(usize, u16, u8, bool, u64)> = Vec::new();
    // The subscription index, the pollfd and whether the entry is a listener.
    let mut polled: Vec<(usize, libc::pollfd, bool)> = Vec::new();
    for (i, sub) in subs.iter().enumerate() {
        if let Sub::Fd { fd, write } = sub {
            let kind = if *write {
                EVENT_FD_WRITE
            } else {
                EVENT_FD_READ
            };
            match lookup(host, *fd, rights::POLL_FD_READWRITE) {
                Ok(entry) => polled.push((
                    i,
                    libc::pollfd {
                        fd: entry.os(),
                        events: if *write { libc::POLLOUT } else { libc::POLLIN },
                        revents: 0,
                    },
                    matches!(entry, Fd::Listener(_)),
                )),
                Err(e) => events.push((i, e, kind, false, 0)),
            }
        }
    }
    host.stats.polls += 1;
    host.stats.max_subs = host.stats.max_subs.max(count);
    let mut pollfds: Vec<libc::pollfd> = polled.iter().map(|(_, p, _)| *p).collect();
    loop {
        let elapsed = clock_ns(1)?.saturating_sub(started);
        let timeout_ms = if !events.is_empty() {
            0
        } else if let Some(ns) = soonest {
            ns.saturating_sub(elapsed)
                .div_ceil(1_000_000)
                .min(i32::MAX as u64) as i32
        } else {
            -1
        };
        let n = unsafe {
            libc::poll(
                pollfds.as_mut_ptr(),
                pollfds.len() as libc::nfds_t,
                timeout_ms,
            )
        };
        if n < 0 {
            if os_errno() == errno::INTR {
                continue;
            }
            return Err(os_errno());
        }
        for ((i, _, listener), p) in polled.iter().zip(&pollfds) {
            if p.revents != 0 {
                let write = p.events == libc::POLLOUT;
                let kind = if write { EVENT_FD_WRITE } else { EVENT_FD_READ };
                events.push((
                    *i,
                    errno::SUCCESS,
                    kind,
                    p.revents & libc::POLLHUP != 0,
                    ready_bytes(p.fd, write, *listener),
                ));
            }
        }
        let elapsed = clock_ns(1)?.saturating_sub(started);
        let mut expired = false;
        for (i, sub) in subs.iter().enumerate() {
            if matches!(sub, Sub::Clock { remaining_ns } if *remaining_ns <= elapsed) {
                events.push((i, errno::SUCCESS, EVENT_CLOCK, false, 0));
                expired = true;
            }
        }
        if !events.is_empty() {
            if events.iter().any(|e| e.2 != EVENT_CLOCK) || !expired {
                host.stats.fd_wakeups += 1;
            } else {
                host.stats.clock_wakeups += 1;
            }
            break;
        }
    }
    for (n, (i, error, kind, hangup, nbytes)) in events.iter().enumerate() {
        let mut event = [0u8; EVENT_SIZE];
        event[..8].copy_from_slice(&users[*i].to_le_bytes());
        event[8..10].copy_from_slice(&error.to_le_bytes());
        event[10] = *kind;
        event[16..24].copy_from_slice(&nbytes.to_le_bytes());
        if *hangup {
            event[24..26].copy_from_slice(&EVENT_HANGUP.to_le_bytes());
        }
        put(mem, at(output, EVENT_SIZE * n)?, &event)?;
    }
    put_u32(mem, nevents, events.len() as u32)
}

/// Define the subset on `linker` under `wasi_snapshot_preview1`.
pub fn add(linker: &mut Linker<Host>) -> wasmtime::Result<()> {
    const M: &str = "wasi_snapshot_preview1";
    linker.func_wrap(
        M,
        "args_sizes_get",
        |mut c: Caller<'_, Host>, a: u32, b: u32| code(list_sizes(&mut c, a, b, false)),
    )?;
    linker.func_wrap(M, "args_get", |mut c: Caller<'_, Host>, a: u32, b: u32| {
        code(list_get(&mut c, a, b, false))
    })?;
    linker.func_wrap(
        M,
        "environ_sizes_get",
        |mut c: Caller<'_, Host>, a: u32, b: u32| code(list_sizes(&mut c, a, b, true)),
    )?;
    linker.func_wrap(
        M,
        "environ_get",
        |mut c: Caller<'_, Host>, a: u32, b: u32| code(list_get(&mut c, a, b, true)),
    )?;
    linker.func_wrap(
        M,
        "clock_res_get",
        |mut c: Caller<'_, Host>, id: u32, out: u32| {
            code((|| {
                let (mem, _) = split(&mut c)?;
                clock_ns(id)?;
                put_u64(mem, out, 1)
            })())
        },
    )?;
    linker.func_wrap(
        M,
        "clock_time_get",
        |mut c: Caller<'_, Host>, id: u32, _precision: u64, out: u32| {
            code((|| {
                let now = clock_ns(id)?;
                let (mem, _) = split(&mut c)?;
                put_u64(mem, out, now)
            })())
        },
    )?;
    linker.func_wrap(
        M,
        "random_get",
        |mut c: Caller<'_, Host>, buf: u32, len: u32| code(random_get(&mut c, buf, len)),
    )?;
    linker.func_wrap(M, "sched_yield", || errno::SUCCESS as i32)?;
    linker.func_wrap(M, "proc_exit", |status: u32| -> wasmtime::Result<()> {
        Err(wasmtime::Error::new(ExitCode(status as i32)))
    })?;
    linker.func_wrap(M, "fd_close", |mut c: Caller<'_, Host>, fd: u32| {
        code(split(&mut c).and_then(|(_, h)| h.table.remove(fd).map(drop).ok_or(errno::BADF)))
    })?;
    linker.func_wrap(
        M,
        "fd_fdstat_get",
        |mut c: Caller<'_, Host>, fd: u32, out: u32| code(fd_fdstat_get(&mut c, fd, out)),
    )?;
    linker.func_wrap(
        M,
        "fd_fdstat_set_flags",
        |mut c: Caller<'_, Host>, fd: u32, flags: u32| code(fd_fdstat_set_flags(&mut c, fd, flags)),
    )?;
    linker.func_wrap(
        M,
        "fd_filestat_get",
        |mut c: Caller<'_, Host>, fd: u32, out: u32| code(fd_filestat_get(&mut c, fd, out)),
    )?;
    // There are no pre-opened directories: the scan wasi-libc runs at startup
    // must stop at the first descriptor it asks about, sockets included.
    linker.func_wrap(
        M,
        "fd_prestat_get",
        |mut c: Caller<'_, Host>, _fd: u32, _out: u32| {
            code(split(&mut c).and_then(|(_, h)| {
                h.stats.prestat_calls += 1;
                Err(errno::BADF)
            }))
        },
    )?;
    linker.func_wrap(
        M,
        "fd_prestat_dir_name",
        |_fd: u32, _path: u32, _len: u32| errno::BADF as i32,
    )?;
    linker.func_wrap(
        M,
        "fd_seek",
        |_fd: u32, _offset: u64, _whence: u32, _out: u32| errno::SPIPE as i32,
    )?;
    linker.func_wrap(M, "fd_tell", |_fd: u32, _out: u32| errno::SPIPE as i32)?;
    linker.func_wrap(
        M,
        "fd_read",
        |mut c: Caller<'_, Host>, fd: u32, iovs: u32, n: u32, out: u32| {
            code(transfer(&mut c, fd, iovs, n, out, false, false))
        },
    )?;
    linker.func_wrap(
        M,
        "fd_write",
        |mut c: Caller<'_, Host>, fd: u32, iovs: u32, n: u32, out: u32| {
            code(transfer(&mut c, fd, iovs, n, out, true, false))
        },
    )?;
    linker.func_wrap(
        M,
        "sock_accept",
        |mut c: Caller<'_, Host>, fd: u32, flags: u32, out: u32| {
            code(sock_accept(&mut c, fd, flags, out))
        },
    )?;
    linker.func_wrap(
        M,
        "sock_recv",
        |mut c: Caller<'_, Host>,
         fd: u32,
         iovs: u32,
         n: u32,
         _flags: u32,
         out: u32,
         roflags: u32| {
            code(
                transfer(&mut c, fd, iovs, n, out, false, true).and_then(|()| {
                    let (mem, _) = split(&mut c)?;
                    put(mem, roflags, &[0, 0])
                }),
            )
        },
    )?;
    linker.func_wrap(
        M,
        "sock_send",
        |mut c: Caller<'_, Host>, fd: u32, iovs: u32, n: u32, _flags: u32, out: u32| {
            code(transfer(&mut c, fd, iovs, n, out, true, true))
        },
    )?;
    linker.func_wrap(
        M,
        "sock_shutdown",
        |mut c: Caller<'_, Host>, fd: u32, how: u32| code(sock_shutdown(&mut c, fd, how)),
    )?;
    linker.func_wrap(
        M,
        "poll_oneoff",
        |mut c: Caller<'_, Host>, i: u32, o: u32, n: u32, e: u32| {
            code(poll_oneoff(&mut c, i, o, n, e))
        },
    )?;
    Ok(())
}
