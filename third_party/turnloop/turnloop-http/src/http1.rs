//! Incremental RFC 9112 framing. The host retains unconsumed input and owns writes.
//! `Body` borrows input; head/trailer allocations are the returned representation.
use crate::{Error, Result};
use std::io::Write;

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub headers: usize,
    pub head_bytes: usize,
    pub line_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            headers: 128,
            head_bytes: 32768,
            line_bytes: 8192,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub name: String,
    pub value: Vec<u8>,
}
impl Header {
    pub fn new(name: &str, value: impl AsRef<[u8]>) -> Self {
        Self {
            name: name.to_ascii_lowercase(),
            value: value.as_ref().to_vec(),
        }
    }
    /// Preserve an HTTP/1 field's original spelling. Lookups remain case-insensitive.
    /// HTTP/2 callers must continue to supply lowercase names.
    pub fn new_preserving_case(name: &str, value: impl AsRef<[u8]>) -> Self {
        Self {
            name: name.to_owned(),
            value: value.as_ref().to_vec(),
        }
    }
}
#[derive(Debug, Clone)]
pub struct Head {
    pub method: String,
    pub target: String,
    pub status: u16,
    pub version: u8,
    pub headers: Vec<Header>,
    pub keep_alive: bool,
}
impl Head {
    pub fn get(&self, name: &str) -> Option<&[u8]> {
        self.headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .map(|h| h.value.as_slice())
    }
    /// Every value of a header that may repeat, in wire order. [`get`](Self::get)
    /// returns only the first line; a list-valued field such as
    /// `Content-Encoding` needs all of them, for example
    /// `StreamingDecoder::from_codings(head.values("content-encoding"), limit)`.
    pub fn values<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a [u8]> + 'a {
        self.headers
            .iter()
            .filter(move |h| h.name.eq_ignore_ascii_case(name))
            .map(|h| h.value.as_slice())
    }
    pub fn token(&self, name: &str, token: &str) -> bool {
        self.headers
            .iter()
            .filter(|h| h.name.eq_ignore_ascii_case(name))
            .any(|h| tokens(&h.value).any(|t| t.eq_ignore_ascii_case(token.as_bytes())))
    }
}
/// Which side of the connection a [`Decoder`] reads.
///
/// The two modes raise the same [`Event`]s, [`Event::Upgrade`] included: a
/// server decoding an upgrade or CONNECT request sees it just as a client
/// decoding the `101` or `2xx` does. What differs is who decides. In
/// [`Mode::Response`] the peer has already switched protocols, so the decoder
/// is finished with the connection. In [`Mode::Request`] the peer has only
/// asked, and the host answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Server side: decode requests.
    Request,
    /// Client side: decode responses. Call [`Decoder::response_to`] first.
    Response,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Head,
    Fixed(u64),
    ChunkSize,
    Chunk(u64),
    ChunkEnd,
    Trailers,
    Eof,
    End,
    Done,
    Upgrade,
    Failed,
}
#[derive(Debug)]
pub enum Event<'a> {
    Head(Head),
    Informational(Head),
    Body(&'a [u8]),
    Trailers(Vec<Header>),
    /// The message is complete. Reads no input: see [`Step`].
    End,
    /// The message is complete, and it leaves HTTP/1 or asks to. Raised
    /// *instead of* [`Event::End`], with `consumed == 0`, as the last event of
    /// the message. Every byte after it belongs to the next protocol, so the
    /// host takes its retained input along.
    ///
    /// Both [`Mode`]s raise it:
    ///
    /// * [`Mode::Response`]: a `101 Switching Protocols`, or a `2xx` answer to a
    ///   request passed to [`Decoder::response_to`] as `CONNECT`. The switch has
    ///   happened, so the decoder is never [`reusable`](Decoder::reusable)
    ///   afterwards.
    /// * [`Mode::Request`]: an HTTP/1.1 request with an `Upgrade` header and
    ///   the `upgrade` token in `Connection` (RFC 9110 section 7.8), or any
    ///   `CONNECT` request. It follows the request body, if there is one. The
    ///   switch has **not** happened: answering `101` (or `2xx` for CONNECT) is
    ///   the host's decision. A host that accepts stops feeding this decoder.
    ///   A host that declines sends an ordinary response and may carry on: the
    ///   decoder is [`reusable`](Decoder::reusable) on the same terms as after
    ///   [`Event::End`], and [`Decoder::reset`] resumes HTTP/1.
    ///
    /// An `Upgrade` header on an HTTP/1.0 request is ignored, as RFC 9110
    /// section 7.8 requires, and that request ends with [`Event::End`].
    Upgrade,
}
/// One decode step: how much of `input` was consumed, and the event it
/// produced, if any. See the [step contract](crate#the-step-contract).
///
/// **`consumed` and `event` are independent.** All four shapes occur:
///
/// | `consumed` | `event` | meaning |
/// |---|---|---|
/// | `0` | `None` | **stop.** More input is needed - or the message is over and the decoder is done; the step does not say which. |
/// | `> 0` | `None` | **keep going.** A chunk-size line, a chunk CRLF or an empty trailer block: progress with nothing for the host. |
/// | `> 0` | `Some` | an event. |
/// | `0` | `Some` | an event that reads no input: [`Event::End`] and [`Event::Upgrade`] both arrive this way. |
///
/// So the loop condition is `consumed > 0 || event.is_some()`, and a host that
/// stops on either zero alone is wrong in one of the two directions:
///
/// ```
/// # use turnloop_http::http1::{Decoder, Event, Mode};
/// # let mut decoder = Decoder::new(Mode::Response, Default::default());
/// # decoder.response_to("GET");
/// # let mut input = b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\nhi".to_vec();
/// let mut ended = false;
/// loop {
///     let step = decoder.receive(&input)?;
///     let consumed = step.consumed;
///     let progressed = consumed > 0 || step.event.is_some();
///     // Body borrows `input`, so handle the event before draining.
///     ended |= matches!(step.event, Some(Event::End));
///     input.drain(..consumed);
///     if !progressed {
///         break; // read more bytes from the transport, then continue
///     }
/// }
/// // `End` came from a step after the last byte was consumed.
/// assert!(ended && input.is_empty() && decoder.reusable());
/// # Ok::<(), turnloop_http::Error>(())
/// ```
///
/// **The zero-consumption event is the trap.** The step that consumes the last
/// byte of a message does not carry `End`; the *next* call does, and it reads
/// nothing. A host that loops while it has input - or stops when a step
/// consumed nothing - never makes that call, and the finished message sits in
/// the decoder until the peer's idle timeout closes the connection.
/// [`Decoder::wants_step`] names the pending call for a host that keys its
/// loop on input rather than on steps.
#[derive(Debug)]
pub struct Step<'a> {
    pub consumed: usize,
    pub event: Option<Event<'a>>,
}
#[derive(Debug)]
pub struct Decoder {
    mode: Mode,
    limits: Limits,
    state: State,
    head_request: bool,
    connect_request: bool,
    keep_alive: bool,
    /// The request being read asks to leave HTTP/1, so it ends in `Upgrade`.
    upgrade_request: bool,
}
fn invalid(message: &'static str) -> Error {
    Error::new("HPE_INVALID_HEADER_TOKEN", message)
}
fn tokens(v: &[u8]) -> impl Iterator<Item = &[u8]> {
    v.split(|b| *b == b',').map(trim)
}
fn trim(v: &[u8]) -> &[u8] {
    v.trim_ascii()
}
fn boundary(input: &[u8]) -> Option<usize> {
    input
        .windows(4)
        .position(|b| b == b"\r\n\r\n")
        .map(|n| n + 4)
}
fn lines_valid(input: &[u8], max: usize) -> Result<()> {
    let mut start = 0;
    for (i, b) in input.iter().enumerate() {
        if *b == b'\n' {
            if i == 0 || input[i - 1] != b'\r' {
                return Err(invalid("bare LF"));
            }
            if i - start > max {
                return Err(Error::new("HPE_HEADER_OVERFLOW", "line limit"));
            }
            start = i + 1;
        } else if *b == b'\r' && i + 1 < input.len() && input[i + 1] != b'\n' {
            return Err(invalid("bare CR"));
        }
    }
    if input.len() - start > max {
        return Err(Error::new("HPE_HEADER_OVERFLOW", "line limit"));
    }
    Ok(())
}
fn headers(raw: &[httparse::Header<'_>], limits: Limits) -> Result<Vec<Header>> {
    if raw.len() > limits.headers {
        return Err(Error::new("HPE_HEADER_OVERFLOW", "header count limit"));
    }
    Ok(raw
        .iter()
        .map(|h| Header::new_preserving_case(h.name, h.value))
        .collect())
}
fn lengths(head: &Head) -> Result<(Option<u64>, bool)> {
    let mut cl = None;
    let mut te = false;
    for h in &head.headers {
        if h.name.eq_ignore_ascii_case("content-length") {
            // Deliberately reject even identical duplicates: no downstream ambiguity.
            if cl.is_some() || h.value.is_empty() || !h.value.iter().all(u8::is_ascii_digit) {
                return Err(Error::new(
                    "HPE_UNEXPECTED_CONTENT_LENGTH",
                    "invalid or duplicate content-length",
                ));
            }
            cl = Some(
                h.value
                    .iter()
                    .try_fold(0u64, |n, b| {
                        n.checked_mul(10)?.checked_add((b - b'0') as u64)
                    })
                    .ok_or(invalid("content-length overflow"))?,
            );
        }
        if h.name.eq_ignore_ascii_case("transfer-encoding") {
            if te || !h.value.eq_ignore_ascii_case(b"chunked") {
                return Err(Error::new(
                    "HPE_INVALID_TRANSFER_ENCODING",
                    "unsupported or duplicate transfer coding",
                ));
            }
            te = true;
        }
    }
    if cl.is_some() && te {
        return Err(Error::new(
            "HPE_UNEXPECTED_CONTENT_LENGTH",
            "transfer-encoding with content-length",
        ));
    }
    if te && head.version == 0 {
        return Err(invalid("transfer-encoding on HTTP/1.0"));
    }
    Ok((cl, te))
}
impl Decoder {
    pub fn new(mode: Mode, limits: Limits) -> Self {
        Self {
            mode,
            limits,
            state: State::Head,
            head_request: false,
            connect_request: false,
            keep_alive: false,
            upgrade_request: false,
        }
    }
    /// Must precede response parsing. Pipelining is intentionally not provided.
    pub fn response_to(&mut self, method: &str) {
        self.head_request = method == "HEAD";
        self.connect_request = method == "CONNECT";
    }
    pub fn reusable(&self) -> bool {
        self.state == State::Done && self.keep_alive
    }
    /// True when the decoder holds an event that reads no input, so the host
    /// must call [`receive`](Self::receive) again even with nothing new to feed
    /// it - an empty slice will do. That event is [`Event::End`] or
    /// [`Event::Upgrade`]. It becomes pending when the last body byte, the
    /// final chunk or the head of a bodiless message is consumed, and after
    /// [`eof`](Self::eof) ends a close-delimited body.
    ///
    /// This is the signal a host that drives on "bytes to feed" is missing.
    /// Such a host stops once its buffer is empty and never sees `End`: the
    /// message sits finished inside the decoder, the connection is not
    /// [`reusable`](Self::reusable), and only the peer's idle timeout ends the
    /// exchange. Loop while `!input.is_empty() || decoder.wants_step()`, or use
    /// the [`Step`] contract's condition, which covers the same case.
    pub fn wants_step(&self) -> bool {
        matches!(self.state, State::End | State::Upgrade)
    }
    /// True between a message's [`Event::Head`] and the point its end is
    /// reached: the body, chunk framing or trailers are still being read. False
    /// before the head (including after an [`Event::Informational`], which is
    /// not the message), once `End` or `Upgrade` is owed or delivered, and after
    /// a failure. Bytes of a head the host has buffered but not yet completed
    /// are the host's to know; the decoder has not accepted them.
    ///
    /// A close-delimited body is mid-message too, though EOF is its legal end:
    /// ask [`eof_is_clean`](Self::eof_is_clean) to tell the two apart.
    pub fn is_mid_message(&self) -> bool {
        matches!(
            self.state,
            State::Fixed(_)
                | State::ChunkSize
                | State::Chunk(_)
                | State::ChunkEnd
                | State::Trailers
                | State::Eof
        )
    }
    /// Whether the transport ending now would be a clean end rather than a
    /// truncation: exactly the cases in which [`eof`](Self::eof) returns `Ok`,
    /// answered without changing any state. True inside a close-delimited body
    /// and once the message is complete; false inside a framed body, before a
    /// head arrives and after a failure. So a host whose read returned zero can
    /// choose what to do - retry, fail with its own diagnosis, log - before it
    /// commits to `eof`.
    ///
    /// Before a head is always "not clean" here, because a client that sees EOF
    /// there got no response. A server whose connection closes while idle
    /// between requests (with no buffered head bytes) treats that as an
    /// ordinary close on its own.
    pub fn eof_is_clean(&self) -> bool {
        matches!(self.state, State::Eof | State::End | State::Done)
    }
    pub fn reset(&mut self) -> Result<()> {
        if !self.reusable() {
            return Err(invalid("connection is not reusable"));
        }
        self.state = State::Head;
        Ok(())
    }
    pub fn receive<'a>(&mut self, input: &'a [u8]) -> Result<Step<'a>> {
        let result = self.receive_inner(input);
        if result.is_err() {
            self.state = State::Failed;
        }
        result
    }
    fn receive_inner<'a>(&mut self, input: &'a [u8]) -> Result<Step<'a>> {
        let mut step = Step {
            consumed: 0,
            event: None,
        };
        match self.state {
            State::Head => {
                let end = boundary(input);
                let n = end.unwrap_or(input.len());
                if n > self.limits.head_bytes {
                    return Err(Error::new("HPE_HEADER_OVERFLOW", "head size limit"));
                }
                lines_valid(&input[..n], self.limits.line_bytes)?;
                let Some(end) = end else { return Ok(step) };
                let mut slots = [httparse::EMPTY_HEADER; 256];
                let mut head = if self.mode == Mode::Response {
                    let mut parsed = httparse::Response::new(&mut slots);
                    parsed
                        .parse(&input[..end])
                        .map_err(|_| invalid("invalid response head"))?;
                    Head {
                        method: String::new(),
                        target: String::new(),
                        status: parsed.code.ok_or(invalid("missing status"))?,
                        version: parsed.version.unwrap_or(1),
                        headers: headers(parsed.headers, self.limits)?,
                        keep_alive: false,
                    }
                } else {
                    let mut parsed = httparse::Request::new(&mut slots);
                    parsed
                        .parse(&input[..end])
                        .map_err(|_| invalid("invalid request head"))?;
                    Head {
                        method: parsed.method.ok_or(invalid("missing method"))?.into(),
                        target: parsed.path.ok_or(invalid("missing target"))?.into(),
                        status: 0,
                        version: parsed.version.unwrap_or(1),
                        headers: headers(parsed.headers, self.limits)?,
                        keep_alive: false,
                    }
                };
                if self.mode == Mode::Request
                    && head.version == 1
                    && (head
                        .headers
                        .iter()
                        .filter(|h| h.name.eq_ignore_ascii_case("host"))
                        .count()
                        != 1
                        || head.get("host").is_none_or(|v| v.is_empty()))
                {
                    return Err(invalid("HTTP/1.1 requires exactly one Host"));
                }
                // RFC 9112 6.3: successful CONNECT starts a tunnel regardless of
                // message framing fields in its response.
                let (cl, te) = if self.mode == Mode::Response
                    && self.connect_request
                    && (200..300).contains(&head.status)
                {
                    (None, false)
                } else {
                    lengths(&head)?
                };
                head.keep_alive = !head.token("connection", "close")
                    && (head.version == 1 || head.token("connection", "keep-alive"));
                self.keep_alive = head.keep_alive;
                // RFC 9110 7.8: an Upgrade in an HTTP/1.0 request is ignored.
                self.upgrade_request = self.mode == Mode::Request
                    && (head.method == "CONNECT"
                        || (head.version == 1
                            && head.get("upgrade").is_some()
                            && head.token("connection", "upgrade")));
                step.consumed = end;
                if self.mode == Mode::Response
                    && (100..200).contains(&head.status)
                    && head.status != 101
                {
                    if cl.is_some() || te {
                        return Err(invalid("informational response has body framing"));
                    }
                    step.event = Some(Event::Informational(head));
                    return Ok(step);
                }
                self.state = if self.mode == Mode::Response
                    && (head.status == 101
                        || (self.connect_request && (200..300).contains(&head.status)))
                {
                    self.keep_alive = false;
                    State::Upgrade
                } else if self.mode == Mode::Response
                    && (self.head_request || head.status == 204 || head.status == 304)
                {
                    State::End
                } else if te {
                    State::ChunkSize
                } else if let Some(n) = cl {
                    if n == 0 { State::End } else { State::Fixed(n) }
                } else if self.mode == Mode::Request {
                    State::End
                } else {
                    self.keep_alive = false;
                    State::Eof
                };
                if self.mode == Mode::Response && head.status == 204 && (cl.is_some() || te) {
                    return Err(invalid("204 has body framing"));
                }
                step.event = Some(Event::Head(head));
            }
            State::Fixed(left) | State::Chunk(left) => {
                let n = input.len().min(usize::try_from(left).unwrap_or(usize::MAX));
                if n > 0 {
                    step.consumed = n;
                    step.event = Some(Event::Body(&input[..n]));
                    self.state = match self.state {
                        State::Fixed(_) => {
                            if left == n as u64 {
                                State::End
                            } else {
                                State::Fixed(left - n as u64)
                            }
                        }
                        _ => {
                            if left == n as u64 {
                                State::ChunkEnd
                            } else {
                                State::Chunk(left - n as u64)
                            }
                        }
                    };
                }
            }
            State::ChunkSize => {
                let end = input.windows(2).position(|w| w == b"\r\n");
                let n = end.unwrap_or(input.len());
                if n > self.limits.line_bytes {
                    return Err(invalid("chunk line limit"));
                }
                let Some(n) = end else { return Ok(step) };
                let digits = input[..n].split(|b| *b == b';').next().unwrap();
                if digits.is_empty() || !digits.iter().all(u8::is_ascii_hexdigit) {
                    return Err(invalid("invalid chunk size"));
                }
                if input[..n].iter().any(|b| *b < 32 || *b == 127) {
                    return Err(invalid("invalid chunk extension"));
                }
                let size = digits
                    .iter()
                    .try_fold(0u64, |v, b| {
                        v.checked_mul(16)?
                            .checked_add((*b as char).to_digit(16)? as u64)
                    })
                    .ok_or(invalid("chunk overflow"))?;
                self.state = if size == 0 {
                    State::Trailers
                } else {
                    State::Chunk(size)
                };
                step.consumed = n + 2;
            }
            State::ChunkEnd => {
                if input.len() >= 2 {
                    if &input[..2] != b"\r\n" {
                        return Err(invalid("missing chunk CRLF"));
                    }
                    step.consumed = 2;
                    self.state = State::ChunkSize;
                }
            }
            State::Trailers => {
                if input.starts_with(b"\r\n") {
                    step.consumed = 2;
                    self.state = State::End;
                    return Ok(step);
                }
                let end = boundary(input);
                let n = end.unwrap_or(input.len());
                if n > self.limits.head_bytes {
                    return Err(invalid("trailer size limit"));
                }
                lines_valid(&input[..n], self.limits.line_bytes)?;
                let Some(n) = end else { return Ok(step) };
                let mut slots = [httparse::EMPTY_HEADER; 256];
                let httparse::Status::Complete((_, raw)) =
                    httparse::parse_headers(&input[..n], &mut slots)
                        .map_err(|_| invalid("invalid trailers"))?
                else {
                    return Err(invalid("incomplete trailers"));
                };
                let trailers = headers(raw, self.limits)?;
                if trailers.iter().any(|h| forbidden_trailer(&h.name)) {
                    return Err(invalid("forbidden trailer"));
                }
                step.consumed = n;
                step.event = Some(Event::Trailers(trailers));
                self.state = State::End;
            }
            State::Eof => {
                if !input.is_empty() {
                    step.consumed = input.len();
                    step.event = Some(Event::Body(input));
                }
            }
            State::End => {
                self.state = State::Done;
                step.event = Some(if self.upgrade_request {
                    Event::Upgrade
                } else {
                    Event::End
                });
            }
            State::Upgrade => {
                self.state = State::Done;
                step.event = Some(Event::Upgrade);
            }
            State::Done => {}
            State::Failed => return Err(invalid("failed connection")),
        }
        Ok(step)
    }
    /// Declare the end of input. Ends a close-delimited body (leaving
    /// [`Event::End`] owed, see [`wants_step`](Self::wants_step)); anywhere
    /// [`eof_is_clean`](Self::eof_is_clean) is false it fails the decoder.
    pub fn eof(&mut self) -> Result<()> {
        if !self.eof_is_clean() {
            self.state = State::Failed;
            return Err(Error::new("UND_ERR_SOCKET", "unexpected EOF"));
        }
        if self.state == State::Eof {
            self.state = State::End;
        }
        Ok(())
    }
}
/// How the body after an encoded head is framed.
#[derive(Debug, Clone, Copy)]
pub enum BodyLength {
    /// No body. A `Content-Length` in the head, if any, must be `0`.
    Empty,
    /// Exactly this many bytes, framed by `Content-Length`.
    Known(u64),
    /// `Transfer-Encoding: chunked`; the only framing that carries trailers.
    Chunked,
    /// Responses only: no framing, the body ends when the connection closes
    /// (RFC 9112 section 6.3, rule 8). The head must carry neither
    /// `Content-Length` nor `Transfer-Encoding`, and after
    /// [`finish`](Encoder::finish) the host closes the connection - that close
    /// *is* the end of the body. Adding `connection: close` is the host's
    /// choice; the peer reads to EOF either way.
    CloseDelimited,
    /// Responses only: the head's framing headers are sent exactly as given,
    /// and no body follows them. This is a response to HEAD, which advertises
    /// the `Content-Length` (or chunked coding) it *would* have sent, and a
    /// `304`, which may do the same. A `204` or `1xx` may not advertise a length
    /// at all (RFC 9110 sections 8.6 and 15.2), so either header is refused
    /// there. The encoder cannot see the request method, so a HEAD response is
    /// the host's word. No framing header is added, and [`body`](Encoder::body)
    /// accepts only empty slices.
    Omitted,
}
/// Caller-owned wire buffer, reused across commands. No body buffering.
pub struct Encoder {
    left: BodyLength,
    finished: bool,
}
impl Encoder {
    /// Encode a head. A response's status line uses the canonical reason
    /// phrase; [`start_with_reason`](Self::start_with_reason) sets another one.
    pub fn start(head: &Head, body: BodyLength, out: &mut Vec<u8>) -> Result<Self> {
        Self::encode(head, None, body, out)
    }
    /// Encode a response head with a custom reason phrase, as Node's
    /// `res.writeHead(404, "Nope")` puts `HTTP/1.1 404 Nope` on the wire. The
    /// phrase may be empty, and may hold spaces, tabs, visible ASCII and
    /// obs-text (RFC 9112 section 4); a request head or a control character is
    /// refused before anything is written.
    pub fn start_with_reason(
        head: &Head,
        reason: &str,
        body: BodyLength,
        out: &mut Vec<u8>,
    ) -> Result<Self> {
        if head.status == 0 {
            return Err(invalid("reason phrase on a request"));
        }
        if reason.bytes().any(|b| (b < 32 && b != b'\t') || b == 127) {
            return Err(invalid("invalid reason phrase"));
        }
        Self::encode(head, Some(reason), body, out)
    }
    fn encode(
        head: &Head,
        reason: Option<&str>,
        body: BodyLength,
        out: &mut Vec<u8>,
    ) -> Result<Self> {
        // Validate everything before mutating the output.
        for h in &head.headers {
            validate_field(h)?;
        }
        let (cl, te) = lengths(head)?;
        let conflict = match body {
            BodyLength::Known(n) => te || cl.is_some_and(|cl| cl != n),
            BodyLength::Empty => te || cl.is_some_and(|cl| cl != 0),
            BodyLength::Chunked => cl.is_some(),
            BodyLength::CloseDelimited => te || cl.is_some(),
            BodyLength::Omitted => {
                (cl.is_some() || te) && (head.status < 200 || head.status == 204)
            }
        };
        if conflict {
            return Err(invalid("body length conflicts with headers"));
        }
        let response_only = matches!(body, BodyLength::CloseDelimited | BodyLength::Omitted);
        if response_only && head.status == 0 {
            return Err(invalid("body length is only valid for a response"));
        }
        if matches!(body, BodyLength::CloseDelimited)
            && (head.status < 200 || matches!(head.status, 204 | 304))
        {
            return Err(invalid("status forbids a body"));
        }
        if head.status == 0 {
            if !valid_token(head.method.as_bytes()) {
                return Err(invalid("invalid method"));
            }
            if head.target.is_empty() || head.target.bytes().any(|b| b <= 32 || b == 127) {
                return Err(invalid("invalid request target"));
            }
            write!(out, "{} {} HTTP/1.1\r\n", head.method, head.target).unwrap();
        } else {
            let status =
                http::StatusCode::from_u16(head.status).map_err(|_| invalid("invalid status"))?;
            let reason = reason.unwrap_or(status.canonical_reason().unwrap_or(""));
            write!(out, "HTTP/1.1 {} {reason}\r\n", head.status).unwrap();
        }
        for h in &head.headers {
            out.extend_from_slice(h.name.as_bytes());
            out.extend_from_slice(b": ");
            out.extend_from_slice(&h.value);
            out.extend_from_slice(b"\r\n");
        }
        if cl.is_none() && !te {
            match body {
                BodyLength::Known(n) => write!(out, "content-length: {n}\r\n").unwrap(),
                BodyLength::Chunked => out.extend_from_slice(b"transfer-encoding: chunked\r\n"),
                BodyLength::Empty | BodyLength::CloseDelimited | BodyLength::Omitted => {}
            }
        }
        out.extend_from_slice(b"\r\n");
        Ok(Self {
            left: body,
            finished: false,
        })
    }
    pub fn body(&mut self, bytes: &[u8], out: &mut Vec<u8>) -> Result<()> {
        if self.finished {
            return Err(invalid("body already finished"));
        }
        match self.left {
            BodyLength::Empty | BodyLength::Omitted => {
                if !bytes.is_empty() {
                    return Err(invalid("unexpected body"));
                }
            }
            BodyLength::CloseDelimited => out.extend_from_slice(bytes),
            BodyLength::Known(n) => {
                if bytes.len() as u64 > n {
                    return Err(invalid("body exceeds content-length"));
                }
                out.extend_from_slice(bytes);
                self.left = BodyLength::Known(n - bytes.len() as u64);
            }
            BodyLength::Chunked => {
                if !bytes.is_empty() {
                    write!(out, "{:x}\r\n", bytes.len()).unwrap();
                    out.extend_from_slice(bytes);
                    out.extend_from_slice(b"\r\n");
                }
            }
        }
        Ok(())
    }
    pub fn finish(&mut self, trailers: &[Header], out: &mut Vec<u8>) -> Result<()> {
        if self.finished || matches!(self.left, BodyLength::Known(1..)) {
            return Err(invalid("incomplete or already finished body"));
        }
        if !trailers.is_empty() && !matches!(self.left, BodyLength::Chunked) {
            return Err(invalid("trailers require chunked body"));
        }
        for h in trailers {
            if forbidden_trailer(&h.name) {
                return Err(invalid("forbidden trailer"));
            }
            validate_field(h)?;
        }
        if matches!(self.left, BodyLength::Chunked) {
            out.extend_from_slice(b"0\r\n");
            for h in trailers {
                out.extend_from_slice(h.name.as_bytes());
                out.extend_from_slice(b": ");
                out.extend_from_slice(&h.value);
                out.extend_from_slice(b"\r\n");
            }
            out.extend_from_slice(b"\r\n");
        }
        self.finished = true;
        Ok(())
    }
}

fn forbidden_trailer(name: &str) -> bool {
    [
        "content-length",
        "transfer-encoding",
        "host",
        "connection",
        "trailer",
        "upgrade",
    ]
    .iter()
    .any(|forbidden| name.eq_ignore_ascii_case(forbidden))
}

/// RFC token grammar, shared with HPACK field validation without temporary types.
pub(crate) fn valid_token(value: &[u8]) -> bool {
    !value.is_empty()
        && value
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(b))
}
fn validate_field(h: &Header) -> Result<()> {
    if !valid_token(h.name.as_bytes())
        || h.value
            .iter()
            .any(|b| (*b < 32 && *b != b'\t') || *b == 127)
    {
        return Err(invalid("invalid header field"));
    }
    Ok(())
}
