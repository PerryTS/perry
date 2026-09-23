//! One connection: an owned `turnloop::Loop`, one socket on it, and optional
//! TLS — presented to the caller as blocking `write_all` / `read` with a
//! deadline.
//!
//! # Why an owned loop is correct *here* and nowhere else in Perry
//!
//! P5 and P6 both refused `turnloop_http::asynchronous` because
//! `LocalExecutor::with_config` builds its **own** `Driver`, and a second loop
//! in a thread that already owns one is the mixed-transport deadlock P1 had to
//! paper over (PerryTS/turnloop#45). That argument is about *sharing a thread
//! with a JS event loop*. This transport is used from two places that have no
//! such loop:
//!
//! * the `perry` CLI, which is a compiler driver and has no JS agent at all;
//! * the `perry-ext-*` HTTP bindings, whose requests already run inside
//!   `perry_ffi::spawn_blocking` — a pool thread that is not an agent, never
//!   parks on `js_*`, and previously blocked on a tokio runtime in exactly the
//!   same place.
//!
//! So the loop here is private to one thread that is doing nothing else, and
//! it is turned to completion before the call returns. Linking this into a
//! loop-owning thread would reintroduce the bug those reports describe; the
//! crate docs say so, and nothing in the runtime depends on it.
//!
//! # Deadlines
//!
//! Every blocking call takes an absolute `turnloop::Instant` deadline and
//! turns the loop with `Timeout::Until`. There is no separate timer handle:
//! `turn` already bounds itself by the host deadline, so a stalled peer costs
//! one wakeup at the deadline and then an `ErrorKind::TimedOut`.

use std::io::{Error as IoError, ErrorKind};
use std::net::{SocketAddr, ToSocketAddrs};
use std::time::Duration;

use perry_tls_session::TlsClientSession;
use turnloop::{
    Completions, Config, Handle, Instant, Loop, OpResult, ReadBuf, TcpOpts, Timeout, Token,
    WriteBuf,
};

/// One token per operation class. The loop is private to one connection, so a
/// constant per class is enough to tell completions apart.
const TOK_CONNECT: Token = Token(1);
const TOK_READ: Token = Token(2);
const TOK_WRITE: Token = Token(3);
const TOK_CLOSE: Token = Token(4);

/// A bound on one `write_all`/`read` call's turns. Each turn either moves
/// bytes, reaches the deadline, or terminates the operation, so this only
/// catches a backend that completes nothing — it is a spin guard, not a policy.
const MAX_TURNS: usize = 1 << 20;

pub type Result<T> = std::result::Result<T, IoError>;

fn timed_out(what: &str) -> IoError {
    IoError::new(ErrorKind::TimedOut, format!("{what} timed out"))
}

fn tls_error(text: impl Into<String>) -> IoError {
    IoError::other(text.into())
}

/// Resolve a host:port with the platform resolver.
///
/// This is a genuinely blocking `getaddrinfo` on the calling thread, not a
/// `turnloop` blocking-pool submission. The caller is a thread that is doing
/// nothing else, so posting the lookup to a pool and then turning the loop to
/// wait for it would add a thread and a hop to buy nothing. It is also what
/// makes this transport usable before a loop exists.
pub fn resolve(host: &str, port: u16) -> Result<Vec<SocketAddr>> {
    let addrs: Vec<SocketAddr> = (host, port).to_socket_addrs()?.collect();
    if addrs.is_empty() {
        return Err(IoError::new(
            ErrorKind::NotFound,
            format!("no address for {host}:{port}"),
        ));
    }
    Ok(addrs)
}

/// A connected socket on a loop this struct owns, with optional TLS on top.
pub struct Connection {
    driver: Loop,
    handle: Option<Handle>,
    completions: Completions,
    tls: Option<Box<TlsClientSession>>,
    /// Plaintext that arrived but the caller has not taken yet.
    inbox: Vec<u8>,
    /// The peer closed its half — `read` returns 0 once `inbox` is drained.
    eof: bool,
}

impl Connection {
    /// Open a TCP connection, trying each resolved address in turn.
    ///
    /// A fresh loop per connection is deliberate: the CLI makes a handful of
    /// requests per invocation, and a loop that outlives its socket would have
    /// to be reference-counted across commands for no measurable gain.
    /// The budget is a `Duration`, not an `Instant`, because
    /// `turnloop::Instant` is the *backend* clock and there is no way to read
    /// it before a loop exists. Each attempt converts the budget against its
    /// own loop's clock; `deadline_in` produces every later deadline.
    pub fn connect(addrs: &[SocketAddr], budget: Duration) -> Result<Self> {
        // ONE budget across every address, not one each. A host with four A
        // and four AAAA records would otherwise be allowed eight times the
        // caller's whole-request window before the first byte is sent, which
        // is what an earlier draft did.
        let started = std::time::Instant::now();
        let mut last = IoError::new(ErrorKind::NotFound, "no address");
        for addr in addrs {
            let Some(remaining) = budget.checked_sub(started.elapsed()) else {
                return Err(timed_out("connect"));
            };
            match Self::connect_one(*addr, remaining) {
                Ok(conn) => return Ok(conn),
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    fn connect_one(addr: SocketAddr, budget: Duration) -> Result<Self> {
        // A connection's loop only ever holds one socket, so the default
        // table sizes are far larger than needed. Trimming them keeps a CLI
        // invocation's allocation small; `pooled_buffers` still covers a full
        // TCP window of outstanding reads.
        let config = Config {
            max_handles: 8,
            max_operations: 32,
            pooled_buffers: 8,
            ..Config::default()
        };
        let mut driver = Loop::new(config).map_err(turnloop_io_error)?;
        let deadline = driver.now() + budget;
        let opts = TcpOpts {
            nodelay: true,
            ..TcpOpts::default()
        };
        let handle = driver
            .tcp_connect(addr, &opts, TOK_CONNECT)
            .map_err(turnloop_io_error)?;

        let mut conn = Self {
            driver,
            handle: Some(handle),
            completions: Completions::default(),
            tls: None,
            inbox: Vec::new(),
            eof: false,
        };

        let mut connected = false;
        for _ in 0..MAX_TURNS {
            if connected {
                break;
            }
            let info = conn
                .driver
                .turn(Timeout::Until(deadline), &mut conn.completions)
                .map_err(turnloop_io_error)?;
            for completion in conn.completions.drain() {
                if completion.token != TOK_CONNECT {
                    continue;
                }
                match completion.result {
                    OpResult::Connected => connected = true,
                    OpResult::Err(e) => return Err(turnloop_io_error(e)),
                    other => {
                        return Err(IoError::other(format!(
                            "unexpected connect completion: {other:?}"
                        )));
                    }
                }
            }
            if !connected && conn.driver.now() >= deadline {
                let _ = info;
                return Err(timed_out("connect"));
            }
        }
        if !connected {
            return Err(IoError::other("connect made no progress"));
        }
        Ok(conn)
    }

    /// `turnloop::Instant` is the backend clock, so a deadline must be built
    /// from it rather than from `std::time::Instant`.
    pub fn deadline_in(&self, after: Duration) -> Instant {
        self.driver.now() + after
    }

    pub fn now(&self) -> Instant {
        self.driver.now()
    }

    /// Start TLS on this connection and run the handshake to completion.
    pub fn start_tls(
        &mut self,
        config: &turnloop_tls::ClientConfig,
        server_name: &str,
        deadline: Instant,
    ) -> Result<Option<Vec<u8>>> {
        let name = perry_tls_session::server_name(server_name).map_err(tls_error)?;
        let session = TlsClientSession::new(config, name).map_err(tls_error)?;
        self.tls = Some(Box::new(session));

        for _ in 0..MAX_TURNS {
            let (handshaking, failure) = {
                let tls = self.tls.as_mut().expect("tls set above");
                tls.pump();
                (
                    tls.is_handshaking(),
                    tls.failure().map(|(code, text)| format!("{code}: {text}")),
                )
            };
            if let Some(text) = failure {
                return Err(tls_error(text));
            }
            self.flush_tls_output(deadline)?;
            if !handshaking {
                let alpn = self.tls.as_ref().and_then(|t| t.alpn_protocol());
                return Ok(alpn);
            }
            // The handshake needs more from the peer. A handshake record
            // yields no *plaintext*, so the EOF test must be on the socket
            // rather than on how much reached `inbox` — reading `inbox` here
            // is what made every https:// connection report the peer had
            // closed after one correct flight.
            self.fill_from_socket(deadline)?;
            if self.eof {
                return Err(IoError::new(
                    ErrorKind::UnexpectedEof,
                    "peer closed during TLS handshake",
                ));
            }
        }
        Err(IoError::other("TLS handshake made no progress"))
    }

    /// Write every byte, encrypting first when TLS is active.
    pub fn write_all(&mut self, bytes: &[u8], deadline: Instant) -> Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        if self.tls.is_some() {
            {
                let tls = self.tls.as_mut().expect("checked");
                tls.write(bytes);
                tls.pump();
                if let Some((code, text)) = tls.failure() {
                    return Err(tls_error(format!("{code}: {text}")));
                }
            }
            self.flush_tls_output(deadline)
        } else {
            self.write_raw(bytes.to_vec(), deadline)
        }
    }

    /// Read at least one byte of application data, or 0 at end of stream.
    ///
    /// A socket read that decrypts to no plaintext — every TLS handshake
    /// record, and a lone `close_notify` — is not end of stream, so the loop
    /// asks again rather than reporting zero.
    pub fn read(&mut self, out: &mut Vec<u8>, deadline: Instant) -> Result<usize> {
        loop {
            if !self.inbox.is_empty() {
                let n = self.inbox.len();
                out.append(&mut self.inbox);
                return Ok(n);
            }
            if self.eof {
                return Ok(0);
            }
            self.fill_from_socket(deadline)?;
        }
    }

    /// Send `close_notify` (when TLS is active) and close the socket. Errors
    /// are deliberately swallowed: a connection being torn down has nothing
    /// left to report, and the caller already has its response.
    pub fn shutdown(&mut self) {
        let deadline = self.driver.now() + Duration::from_millis(250);
        if self.tls.is_some() {
            {
                let tls = self.tls.as_mut().expect("checked");
                tls.close_notify();
                tls.pump();
            }
            let _ = self.flush_tls_output(deadline);
        }
        if let Some(handle) = self.handle.take() {
            if self.driver.close(handle, TOK_CLOSE).is_ok() {
                let _ = self.driver.turn(Timeout::Now, &mut self.completions);
                self.completions.clear();
            }
        }
    }

    /// Move whatever the TLS session has encrypted onto the socket.
    ///
    /// Bounded like every other loop in this file. It already terminates
    /// because `write_raw` carries the deadline, but a session that kept
    /// producing output onto an accepting socket would otherwise spin with no
    /// diagnostic, and "no diagnostic" is the part worth fixing.
    fn flush_tls_output(&mut self, deadline: Instant) -> Result<()> {
        for _ in 0..MAX_TURNS {
            let out = match self.tls.as_mut() {
                Some(tls) => tls.take_output(),
                None => return Ok(()),
            };
            if out.is_empty() {
                return Ok(());
            }
            self.write_raw(out, deadline)?;
        }
        Err(IoError::other("TLS output never drained"))
    }

    /// One socket read, decrypted when TLS is active, appended to `inbox`.
    ///
    /// Returns the number of bytes that came off the **socket**, not the
    /// number that reached `inbox`: a TLS handshake record is many socket
    /// bytes and no plaintext, and conflating the two reads as end of stream.
    /// End of stream is `self.eof`.
    fn fill_from_socket(&mut self, deadline: Instant) -> Result<usize> {
        let handle = self
            .handle
            .ok_or_else(|| IoError::new(ErrorKind::NotConnected, "connection closed"))?;
        self.driver
            .read(handle, ReadBuf::Pooled, TOK_READ)
            .map_err(turnloop_io_error)?;

        let mut ciphertext: Option<Vec<u8>> = None;
        let mut done = false;
        for _ in 0..MAX_TURNS {
            if done {
                break;
            }
            self.driver
                .turn(Timeout::Until(deadline), &mut self.completions)
                .map_err(turnloop_io_error)?;
            for completion in self.completions.drain() {
                if completion.token != TOK_READ {
                    continue;
                }
                match completion.result {
                    OpResult::Read { n, lease } => {
                        if let Some(lease) = lease {
                            ciphertext = Some(lease.as_slice()[..n].to_vec());
                            lease.release();
                        } else {
                            ciphertext = Some(Vec::new());
                        }
                        done = true;
                    }
                    OpResult::Eof => {
                        self.eof = true;
                        done = true;
                    }
                    OpResult::Err(e) => return Err(turnloop_io_error(e)),
                    OpResult::Cancelled | OpResult::Closed | OpResult::Stopped => {
                        self.eof = true;
                        done = true;
                    }
                    other => {
                        return Err(IoError::other(format!(
                            "unexpected read completion: {other:?}"
                        )));
                    }
                }
            }
            if !done && self.driver.now() >= deadline {
                return Err(timed_out("read"));
            }
        }
        if !done {
            return Err(IoError::other("read made no progress"));
        }

        let bytes = ciphertext.unwrap_or_default();
        let read = bytes.len();
        if self.tls.is_none() {
            self.inbox.extend_from_slice(&bytes);
            return Ok(read);
        }

        let failure = {
            let tls = self.tls.as_mut().expect("checked");
            if !bytes.is_empty() {
                tls.receive(&bytes);
            }
            tls.pump();
            let plaintext = tls.take_plaintext();
            self.inbox.extend_from_slice(&plaintext);
            tls.failure().map(|(code, text)| format!("{code}: {text}"))
        };
        if let Some(text) = failure {
            // Plaintext decrypted before the failure is still valid; report
            // the failure only when there is nothing left to hand back.
            if self.inbox.is_empty() {
                return Err(tls_error(text));
            }
        }
        // A handshake record consumed here produces ciphertext to send back.
        self.flush_tls_output(deadline)?;
        Ok(read)
    }

    /// Write every byte, resubmitting whatever a completion did not take.
    ///
    /// turnloop's `write` reports the byte count it transferred, so a short
    /// write is normal on a full socket buffer and must be resent rather than
    /// treated as an error. The operation owns its buffer for its lifetime, so
    /// each attempt hands over a fresh `Vec` of the remaining bytes.
    fn write_raw(&mut self, bytes: Vec<u8>, deadline: Instant) -> Result<()> {
        let handle = self
            .handle
            .ok_or_else(|| IoError::new(ErrorKind::NotConnected, "connection closed"))?;
        let mut offset = 0usize;
        while offset < bytes.len() {
            let chunk = bytes[offset..].to_vec();
            self.driver
                .write(handle, WriteBuf::Owned(chunk), TOK_WRITE)
                .map_err(turnloop_io_error)?;

            let mut wrote: Option<usize> = None;
            for _ in 0..MAX_TURNS {
                if wrote.is_some() {
                    break;
                }
                self.driver
                    .turn(Timeout::Until(deadline), &mut self.completions)
                    .map_err(turnloop_io_error)?;
                for completion in self.completions.drain() {
                    if completion.token != TOK_WRITE {
                        continue;
                    }
                    match completion.result {
                        OpResult::Wrote(n) => wrote = Some(n),
                        OpResult::Err(e) => return Err(turnloop_io_error(e)),
                        other => {
                            return Err(IoError::other(format!(
                                "unexpected write completion: {other:?}"
                            )));
                        }
                    }
                }
                if wrote.is_none() && self.driver.now() >= deadline {
                    return Err(timed_out("write"));
                }
            }
            let Some(n) = wrote else {
                return Err(IoError::other("write made no progress"));
            };
            if n == 0 {
                return Err(IoError::new(
                    ErrorKind::WriteZero,
                    "socket accepted no bytes",
                ));
            }
            offset += n;
        }
        Ok(())
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        if self.handle.is_some() {
            self.shutdown();
        }
    }
}

/// turnloop reports its own portable `Error`; every caller here speaks
/// `std::io::Error`. The OS code is kept when the backend had one, so an
/// `ECONNREFUSED` still prints as one rather than as "connection refused
/// (kind)".
fn turnloop_io_error(error: turnloop::Error) -> IoError {
    if let Some(code) = error.os {
        return IoError::from_raw_os_error(code);
    }
    let kind = match error.kind {
        turnloop::ErrorKind::Cancelled => ErrorKind::Interrupted,
        turnloop::ErrorKind::Unsupported => ErrorKind::Unsupported,
        turnloop::ErrorKind::InvalidInput => ErrorKind::InvalidInput,
        turnloop::ErrorKind::NotFound => ErrorKind::NotFound,
        turnloop::ErrorKind::WouldBlock => ErrorKind::WouldBlock,
        turnloop::ErrorKind::TimedOut => ErrorKind::TimedOut,
        turnloop::ErrorKind::ConnectionRefused => ErrorKind::ConnectionRefused,
        turnloop::ErrorKind::ConnectionReset => ErrorKind::ConnectionReset,
        turnloop::ErrorKind::BrokenPipe => ErrorKind::BrokenPipe,
        turnloop::ErrorKind::PermissionDenied => ErrorKind::PermissionDenied,
        turnloop::ErrorKind::AlreadyExists => ErrorKind::AlreadyExists,
        turnloop::ErrorKind::NotADirectory => ErrorKind::NotADirectory,
        turnloop::ErrorKind::IsADirectory => ErrorKind::IsADirectory,
        turnloop::ErrorKind::DirectoryNotEmpty => ErrorKind::DirectoryNotEmpty,
        turnloop::ErrorKind::ResourceLimit | turnloop::ErrorKind::Other => ErrorKind::Other,
    };
    IoError::new(kind, format!("turnloop: {:?}", error.kind))
}
