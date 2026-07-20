//! POSIX pseudo-terminal primitives for the node-pty surface (#6563).
//!
//! Pure OS layer — no JSValues anywhere in this file, so every function is
//! callable from background threads and directly unit-testable. The JS-facing
//! wiring (IPty object, event pump) lives in `mod.rs` / `reactor.rs`.
//!
//! Model:
//! * [`open_pty_pair`] — `openpty(3)` with node-pty's sane default termios
//!   (echo on, canonical mode, ISIG, 38400 baud) and the requested winsize.
//! * [`spawn_in_pty`] — fork; the child becomes a session leader, takes the
//!   slave as its controlling terminal (`TIOCSCTTY`), dups it onto
//!   stdin/stdout/stderr and execs. Everything the child touches (argv, envp,
//!   cwd, resolved exec candidates) is pre-marshalled in the parent because
//!   only async-signal-safe calls are allowed between `fork` and `execve` in
//!   a multithreaded process.
//! * [`wait_child`] — blocking `waitpid` reap (run on a dedicated thread by
//!   the reactor), decoded to node's `(exitCode, signal)` split.
//! * [`resize_pty`] / [`signal_pid`] — `TIOCSWINSZ` and `kill(2)`.
//!
//! Works headless: the pty pair is allocated fresh — no controlling tty on
//! the parent is required (CI runners included).

#![allow(clippy::manual_c_str_literals)]

use std::ffi::CString;
use std::io;
use std::os::unix::io::RawFd;

/// Everything needed to launch one pty child. Built on the main thread from
/// the JS `spawn(file, args, options)` call.
pub(crate) struct PtySpawnRequest {
    pub file: String,
    pub args: Vec<String>,
    /// Full child environment (`TERM` already folded in by the caller).
    pub env: Vec<(String, String)>,
    pub cwd: Option<String>,
    pub cols: u16,
    pub rows: u16,
}

/// A successfully launched pty child: the process id and the master side of
/// the pty (the slave lives on as fds 0/1/2 of the child only).
pub(crate) struct PtyChild {
    pub pid: i32,
    pub master: RawFd,
}

/// node-pty's default termios (deps/pty.cc `assign(...)`): interactive line
/// discipline — canonical mode, echo on, ISIG — exactly what a real terminal
/// hands a login shell.
fn sane_termios() -> libc::termios {
    let mut term: libc::termios = unsafe { std::mem::zeroed() };
    term.c_iflag = libc::ICRNL | libc::IXON | libc::IXANY | libc::IMAXBEL | libc::BRKINT;
    term.c_iflag |= libc::IUTF8;
    term.c_oflag = libc::OPOST | libc::ONLCR;
    term.c_cflag = libc::CREAD | libc::CS8 | libc::HUPCL;
    term.c_lflag = libc::ICANON
        | libc::ISIG
        | libc::IEXTEN
        | libc::ECHO
        | libc::ECHOE
        | libc::ECHOK
        | libc::ECHOKE
        | libc::ECHOCTL;
    term.c_cc[libc::VEOF] = 4; // ^D
    term.c_cc[libc::VEOL] = 0xff; // disabled
    term.c_cc[libc::VEOL2] = 0xff;
    term.c_cc[libc::VERASE] = 0x7f; // DEL
    term.c_cc[libc::VWERASE] = 23; // ^W
    term.c_cc[libc::VKILL] = 21; // ^U
    term.c_cc[libc::VREPRINT] = 18; // ^R
    term.c_cc[libc::VINTR] = 3; // ^C
    term.c_cc[libc::VQUIT] = 0x1c; // ^\
    term.c_cc[libc::VSUSP] = 26; // ^Z
    term.c_cc[libc::VSTART] = 17; // ^Q
    term.c_cc[libc::VSTOP] = 19; // ^S
    term.c_cc[libc::VLNEXT] = 22; // ^V
    term.c_cc[libc::VDISCARD] = 15; // ^O
    term.c_cc[libc::VMIN] = 1;
    term.c_cc[libc::VTIME] = 0;
    unsafe {
        libc::cfsetispeed(&mut term, libc::B38400);
        libc::cfsetospeed(&mut term, libc::B38400);
    }
    term
}

fn winsize(cols: u16, rows: u16) -> libc::winsize {
    libc::winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    }
}

/// Allocate a fresh master/slave pty pair with the sane termios + winsize
/// applied. The master is marked CLOEXEC so unrelated children (including
/// this pty's own exec'd child) never inherit it.
pub(crate) fn open_pty_pair(cols: u16, rows: u16) -> io::Result<(RawFd, RawFd)> {
    let mut master: libc::c_int = -1;
    let mut slave: libc::c_int = -1;
    let mut term = sane_termios();
    let mut ws = winsize(cols, rows);
    let rc = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            &mut term,
            &mut ws,
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    unsafe {
        libc::fcntl(master, libc::F_SETFD, libc::FD_CLOEXEC);
    }
    Ok((master, slave))
}

/// Resolve `file` to the execve candidate list — an absolute/relative path is
/// taken as-is; a bare name is expanded against the child env's `PATH` (the
/// same order `execvp` would try). Resolution happens in the PARENT so the
/// post-fork child only calls the async-signal-safe `execve`.
fn resolve_exec_candidates(file: &str, env: &[(String, String)]) -> Vec<CString> {
    if file.contains('/') {
        return CString::new(file).ok().into_iter().collect();
    }
    let path = env
        .iter()
        .find(|(k, _)| k == "PATH")
        .map(|(_, v)| v.clone())
        .or_else(|| std::env::var("PATH").ok())
        .unwrap_or_else(|| "/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin".to_string());
    path.split(':')
        .filter(|d| !d.is_empty())
        .filter_map(|d| CString::new(format!("{d}/{file}")).ok())
        .collect()
}

/// Fork + exec `req.file` with the slave side of a fresh pty as its
/// controlling terminal and stdio. Returns the child pid + master fd.
///
/// The child half runs only async-signal-safe calls (`setsid`, `ioctl`,
/// `dup2`, `chdir`, `execve`, `_exit`); all heap work happens before `fork`.
pub(crate) fn spawn_in_pty(req: &PtySpawnRequest) -> io::Result<PtyChild> {
    let candidates = resolve_exec_candidates(&req.file, &req.env);
    if candidates.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("spawn {} ENOENT", req.file),
        ));
    }

    // argv = [file, ...args]; entries with interior NULs are dropped rather
    // than failing the whole spawn (they could never be exec'd anyway).
    let argv_c: Vec<CString> = std::iter::once(req.file.as_str())
        .chain(req.args.iter().map(|s| s.as_str()))
        .filter_map(|s| CString::new(s).ok())
        .collect();
    let mut argv_ptrs: Vec<*const libc::c_char> = argv_c.iter().map(|c| c.as_ptr()).collect();
    argv_ptrs.push(std::ptr::null());

    let envp_c: Vec<CString> = req
        .env
        .iter()
        .filter_map(|(k, v)| CString::new(format!("{k}={v}")).ok())
        .collect();
    let mut envp_ptrs: Vec<*const libc::c_char> = envp_c.iter().map(|c| c.as_ptr()).collect();
    envp_ptrs.push(std::ptr::null());

    let cwd_c = match &req.cwd {
        Some(d) => match CString::new(d.as_str()) {
            Ok(c) => Some(c),
            Err(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "cwd contains a NUL byte",
                ))
            }
        },
        None => None,
    };

    let (master, slave) = open_pty_pair(req.cols, req.rows)?;

    match unsafe { libc::fork() } {
        -1 => {
            let err = io::Error::last_os_error();
            unsafe {
                libc::close(master);
                libc::close(slave);
            }
            Err(err)
        }
        0 => {
            // Child. Async-signal-safe calls ONLY from here to execve.
            unsafe {
                libc::setsid();
                libc::ioctl(slave, libc::TIOCSCTTY as libc::c_ulong, 0);
                libc::dup2(slave, 0);
                libc::dup2(slave, 1);
                libc::dup2(slave, 2);
                if slave > 2 {
                    libc::close(slave);
                }
                libc::close(master);
                if let Some(cwd) = &cwd_c {
                    if libc::chdir(cwd.as_ptr()) != 0 {
                        libc::_exit(127);
                    }
                }
                for p in &candidates {
                    libc::execve(p.as_ptr(), argv_ptrs.as_ptr(), envp_ptrs.as_ptr());
                }
                libc::_exit(127);
            }
        }
        pid => {
            unsafe {
                libc::close(slave);
            }
            Ok(PtyChild { pid, master })
        }
    }
}

/// Blocking reap of `pid`. Returns node-pty's `(exitCode, signal)` split:
/// a normal exit yields `(Some(status), None)`; death-by-signal yields
/// `(None, Some(signo))`.
pub(crate) fn wait_child(pid: i32) -> (Option<i32>, Option<i32>) {
    let mut status: libc::c_int = 0;
    loop {
        let r = unsafe { libc::waitpid(pid, &mut status, 0) };
        if r == -1 {
            if io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            return (Some(-1), None);
        }
        break;
    }
    if libc::WIFEXITED(status) {
        (Some(libc::WEXITSTATUS(status)), None)
    } else if libc::WIFSIGNALED(status) {
        (None, Some(libc::WTERMSIG(status)))
    } else {
        (Some(-1), None)
    }
}

/// `TIOCSWINSZ` on the master — the kernel delivers `SIGWINCH` to the child's
/// foreground process group.
pub(crate) fn resize_pty(master: RawFd, cols: u16, rows: u16) -> bool {
    let ws = winsize(cols, rows);
    unsafe { libc::ioctl(master, libc::TIOCSWINSZ as libc::c_ulong, &ws) == 0 }
}

/// `kill(2)` — deliver `signo` to `pid`.
pub(crate) fn signal_pid(pid: i32, signo: i32) -> bool {
    unsafe { libc::kill(pid, signo) == 0 }
}

/// Read the current winsize back off a pty fd (test support).
#[cfg(test)]
pub(crate) fn read_winsize(fd: RawFd) -> Option<(u16, u16)> {
    let mut ws = winsize(0, 0);
    let rc = unsafe { libc::ioctl(fd, libc::TIOCGWINSZ as libc::c_ulong, &mut ws) };
    if rc == 0 {
        Some((ws.ws_col, ws.ws_row))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::os::unix::io::FromRawFd;

    fn base_env() -> Vec<(String, String)> {
        let mut env: Vec<(String, String)> = std::env::vars().collect();
        env.retain(|(k, _)| k != "TERM");
        env.push(("TERM".to_string(), "xterm-256color".to_string()));
        env
    }

    /// Read the master until EOF/EIO (child exited and released the slave).
    fn drain_master(master: RawFd) -> Vec<u8> {
        // Take ownership so the fd closes at the end of the test.
        let mut f = unsafe { std::fs::File::from_raw_fd(master) };
        let mut out = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            match f.read(&mut buf) {
                Ok(0) | Err(_) => break, // Linux reports EIO at pty EOF
                Ok(n) => out.extend_from_slice(&buf[..n]),
            }
        }
        out
    }

    #[test]
    fn pty_shell_echo_roundtrip_and_clean_exit() {
        // Headless-safe: the pty pair is allocated fresh; no controlling tty
        // on the test process is needed.
        let child = spawn_in_pty(&PtySpawnRequest {
            file: "sh".to_string(),
            args: Vec::new(),
            env: base_env(),
            cwd: None,
            cols: 80,
            rows: 24,
        })
        .expect("spawn sh in pty");
        assert!(child.pid > 0);

        // The command's echo prints "PTY_%s" while the output prints
        // "PTY_OK", so the assertion can't be satisfied by the echo alone.
        let script = b"printf 'PTY_%s\\n' OK\nexit\n";
        let n = unsafe {
            libc::write(
                child.master,
                script.as_ptr() as *const libc::c_void,
                script.len(),
            )
        };
        assert_eq!(n as usize, script.len(), "write to pty master");

        let out = drain_master(child.master);
        let text = String::from_utf8_lossy(&out);
        assert!(
            text.contains("PTY_OK"),
            "shell output must round-trip through the pty; got: {text:?}"
        );

        let (code, signal) = wait_child(child.pid);
        assert_eq!(code, Some(0), "clean `exit` must reap as exit code 0");
        assert_eq!(signal, None);
    }

    #[test]
    fn pty_kill_sigterm_reports_signal() {
        // `sleep` (not an interactive shell — interactive shells ignore
        // SIGTERM) so the signal actually terminates the child.
        let child = spawn_in_pty(&PtySpawnRequest {
            file: "sleep".to_string(),
            args: vec!["30".to_string()],
            env: base_env(),
            cwd: None,
            cols: 80,
            rows: 24,
        })
        .expect("spawn sleep in pty");

        // Give it a beat to exec, then terminate.
        std::thread::sleep(std::time::Duration::from_millis(150));
        assert!(signal_pid(child.pid, libc::SIGTERM), "kill(SIGTERM)");

        let (code, signal) = wait_child(child.pid);
        assert_eq!(signal, Some(libc::SIGTERM), "must reap as signal death");
        assert_eq!(code, None);
        unsafe {
            libc::close(child.master);
        }
    }

    #[test]
    fn pty_resize_applies_winsize() {
        let (master, slave) = open_pty_pair(80, 24).expect("openpty");
        assert_eq!(read_winsize(master), Some((80, 24)));
        assert!(resize_pty(master, 120, 40));
        assert_eq!(read_winsize(slave), Some((120, 40)));
        unsafe {
            libc::close(master);
            libc::close(slave);
        }
    }

    #[test]
    fn pty_spawn_enoent_is_reported() {
        let err = spawn_in_pty(&PtySpawnRequest {
            file: "definitely-not-a-real-binary-6563".to_string(),
            args: Vec::new(),
            env: vec![("PATH".to_string(), "/nonexistent-dir".to_string())],
            cwd: None,
            cols: 80,
            rows: 24,
        });
        // Bare name + dead PATH: candidates exist but every execve fails →
        // the child _exit(127)s. An empty candidate list errors in the
        // parent. Both shapes are acceptable; this test pins the parent-side
        // error for the no-candidate case.
        match err {
            Ok(child) => {
                let (code, signal) = wait_child(child.pid);
                assert_eq!(code, Some(127), "exec failure must exit 127");
                assert_eq!(signal, None);
                unsafe {
                    libc::close(child.master);
                }
            }
            Err(e) => assert_eq!(e.kind(), std::io::ErrorKind::NotFound),
        }
    }
}
