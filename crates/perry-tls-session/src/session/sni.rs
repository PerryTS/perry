//! Read the SNI host name out of the client's first flight.
//!
//! rustls 0.23 parses SNI into `ServerConnectionData` but only the buffered
//! `ServerConnection` exposes it (`server_name()`); the unbuffered connection
//! has no accessor. Node reports it (`tlsSocket.servername`) and Perry's
//! `node:tls` server picks the certificate it reports as its own from it, so
//! the session reads it itself: TLS handshake records are plaintext until the
//! ServerHello, and the ClientHello is the first handshake message.
//!
//! This is observation only — rustls still parses, validates and acts on the
//! ClientHello. A malformed or truncated hello simply yields `None` here while
//! rustls reports the real error.

/// Largest ClientHello this will buffer before giving up (rustls's own
/// handshake-message limit is 64 KiB too).
const LIMIT: usize = 64 * 1024;
const RECORD_HANDSHAKE: u8 = 22;
const HANDSHAKE_CLIENT_HELLO: u8 = 1;
const EXTENSION_SERVER_NAME: u16 = 0;
const NAME_TYPE_HOST_NAME: u8 = 0;

pub(super) enum Capture {
    /// Still collecting: raw record bytes seen so far.
    Pending(Vec<u8>),
    Done(Option<String>),
}

impl Default for Capture {
    fn default() -> Self {
        Self::Pending(Vec::new())
    }
}

impl Capture {
    pub(super) fn name(&self) -> Option<&str> {
        match self {
            Self::Done(name) => name.as_deref(),
            Self::Pending(_) => None,
        }
    }

    pub(super) fn feed(&mut self, bytes: &[u8]) {
        let Self::Pending(seen) = self else {
            return;
        };
        seen.extend_from_slice(bytes);
        match client_hello(seen) {
            Scan::NeedMore if seen.len() <= LIMIT => {}
            Scan::NeedMore | Scan::Invalid => *self = Self::Done(None),
            Scan::Hello(body) => *self = Self::Done(server_name(&body)),
        }
    }
}

enum Scan {
    NeedMore,
    Invalid,
    Hello(Vec<u8>),
}

/// Reassemble the ClientHello body from the leading handshake records (it may
/// be fragmented across several).
fn client_hello(records: &[u8]) -> Scan {
    let mut message = Vec::new();
    let mut at = 0;
    loop {
        // Enough of the handshake header to know the message length?
        if message.len() >= 4 {
            if message[0] != HANDSHAKE_CLIENT_HELLO {
                return Scan::Invalid;
            }
            let len = u24(&message[1..4]);
            if message.len() >= 4 + len {
                return Scan::Hello(message[4..4 + len].to_vec());
            }
        }
        let Some(header) = records.get(at..at + 5) else {
            return Scan::NeedMore;
        };
        if header[0] != RECORD_HANDSHAKE {
            return Scan::Invalid;
        }
        let len = u16::from_be_bytes([header[3], header[4]]) as usize;
        let Some(fragment) = records.get(at + 5..at + 5 + len) else {
            return Scan::NeedMore;
        };
        message.extend_from_slice(fragment);
        at += 5 + len;
    }
}

fn u24(bytes: &[u8]) -> usize {
    (usize::from(bytes[0]) << 16) | (usize::from(bytes[1]) << 8) | usize::from(bytes[2])
}

struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.bytes.len() < n {
            return None;
        }
        let (head, tail) = self.bytes.split_at(n);
        self.bytes = tail;
        Some(head)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }
    fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|b| u16::from_be_bytes([b[0], b[1]]))
    }
    fn vec8(&mut self) -> Option<&'a [u8]> {
        let n = usize::from(self.u8()?);
        self.take(n)
    }
    fn vec16(&mut self) -> Option<&'a [u8]> {
        let n = usize::from(self.u16()?);
        self.take(n)
    }
}

/// The host name in a ClientHello body's `server_name` extension, normalised
/// the way rustls normalises the name it keeps (trailing dot trimmed,
/// lower-cased, and an IP literal or otherwise invalid DNS name is no name).
fn server_name(body: &[u8]) -> Option<String> {
    let mut r = Reader { bytes: body };
    r.take(2 + 32)?; // legacy_version, random
    r.vec8()?; // legacy_session_id
    r.vec16()?; // cipher_suites
    r.vec8()?; // legacy_compression_methods
    let mut extensions = Reader { bytes: r.vec16()? };
    while !extensions.bytes.is_empty() {
        let kind = extensions.u16()?;
        let data = extensions.vec16()?;
        if kind != EXTENSION_SERVER_NAME {
            continue;
        }
        let mut list = Reader {
            bytes: Reader { bytes: data }.vec16()?,
        };
        while !list.bytes.is_empty() {
            let name_type = list.u8()?;
            let name = list.vec16()?;
            if name_type != NAME_TYPE_HOST_NAME {
                continue;
            }
            let name = std::str::from_utf8(name).ok()?;
            let name = name.strip_suffix('.').unwrap_or(name);
            if name.parse::<std::net::IpAddr>().is_ok() {
                return None;
            }
            let dns = turnloop_tls::rustls::pki_types::DnsName::try_from(name).ok()?;
            return Some(dns.as_ref().to_ascii_lowercase());
        }
        return None;
    }
    None
}
