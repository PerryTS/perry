//! RFC 9113 connection/stream framing. Caller retains partial frames and acknowledges
//! writes. DATA is borrowed, flow-control credit is returned explicitly by the host.
use crate::{Error, Result, hpack, http1::Header};
use std::{collections::VecDeque, time::Instant};
pub const PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Client,
    Server,
}
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub frame_size: usize,
    pub header_block: usize,
    pub header_list: usize,
    pub continuations: usize,
    pub streams: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            frame_size: 16384,
            header_block: 65536,
            header_list: 32768,
            continuations: 16,
            streams: 100,
        }
    }
}
/// SETTINGS parameters this endpoint sends: at most one value per identifier,
/// kept and serialised in ascending identifier order (the order Node writes
/// them in). An empty `Settings` is a valid, empty SETTINGS frame.
///
/// Identifiers this crate does not know are sent as given, so a host can
/// advertise extension settings; the named constants are the ones RFC 9113
/// section 6.5.2 and RFC 8441 define.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    entries: Vec<(u16, u32)>,
}
impl Settings {
    pub const HEADER_TABLE_SIZE: u16 = 1;
    pub const ENABLE_PUSH: u16 = 2;
    pub const MAX_CONCURRENT_STREAMS: u16 = 3;
    pub const INITIAL_WINDOW_SIZE: u16 = 4;
    pub const MAX_FRAME_SIZE: u16 = 5;
    pub const MAX_HEADER_LIST_SIZE: u16 = 6;
    pub const ENABLE_CONNECT_PROTOCOL: u16 = 8;
    pub fn new() -> Self {
        Self::default()
    }
    /// The three limits [`Connection::new`] advertises.
    pub fn from_limits(limits: &Limits) -> Self {
        let mut settings = Self::new();
        settings
            .set(Self::MAX_CONCURRENT_STREAMS, limits.streams as u32)
            .set(Self::MAX_FRAME_SIZE, limits.frame_size as u32)
            .set(Self::MAX_HEADER_LIST_SIZE, limits.header_list as u32);
        settings
    }
    /// Set `id` to `value`, replacing any earlier value for it.
    pub fn set(&mut self, id: u16, value: u32) -> &mut Self {
        match self.entries.binary_search_by_key(&id, |e| e.0) {
            Ok(i) => self.entries[i].1 = value,
            Err(i) => self.entries.insert(i, (id, value)),
        }
        self
    }
    pub fn remove(&mut self, id: u16) -> Option<u32> {
        let i = self.entries.binary_search_by_key(&id, |e| e.0).ok()?;
        Some(self.entries.remove(i).1)
    }
    pub fn get(&self, id: u16) -> Option<u32> {
        let i = self.entries.binary_search_by_key(&id, |e| e.0).ok()?;
        Some(self.entries[i].1)
    }
    /// `(identifier, value)` pairs in ascending identifier order.
    pub fn iter(&self) -> impl Iterator<Item = (u16, u32)> + '_ {
        self.entries.iter().copied()
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    fn merge(&mut self, from: impl Iterator<Item = (u16, u32)>) {
        for (id, value) in from {
            self.set(id, value);
        }
    }
    fn encode(&self) -> Vec<u8> {
        let mut payload = Vec::with_capacity(self.entries.len() * 6);
        for (id, value) in self.iter() {
            payload.extend_from_slice(&id.to_be_bytes());
            payload.extend_from_slice(&value.to_be_bytes());
        }
        payload
    }
    /// Refuse what this endpoint cannot honour once the peer acts on it.
    fn validate(&self) -> Result<()> {
        for (id, value) in self.iter() {
            let valid = match id {
                // The HPACK decoder's dynamic table is fixed at 4096 octets;
                // advertising more would let the peer's encoder outgrow it.
                Self::HEADER_TABLE_SIZE => value <= 4096,
                // Server push is not implemented: PUSH_PROMISE is always a
                // connection error, so neither side may invite one.
                Self::ENABLE_PUSH => value == 0,
                Self::INITIAL_WINDOW_SIZE => value <= 0x7fffffff,
                Self::MAX_FRAME_SIZE => (16384..=0xffffff).contains(&value),
                Self::ENABLE_CONNECT_PROTOCOL => value <= 1,
                _ => true,
            };
            if !valid {
                return Err(protocol("invalid SETTINGS value"));
            }
        }
        Ok(())
    }
}
/// The parameters of one SETTINGS frame the peer sent, borrowed from the input
/// exactly as they were on the wire: in frame order, duplicates and unknown
/// identifiers included. RFC 9113 section 6.5.3 processes them in that order,
/// so for a repeated identifier the last value is the one in force.
#[derive(Debug, Clone, Copy)]
pub struct SettingsFrame<'a> {
    payload: &'a [u8],
}
impl<'a> SettingsFrame<'a> {
    pub fn iter(&self) -> impl Iterator<Item = (u16, u32)> + 'a {
        self.payload
            .as_chunks::<6>()
            .0
            .iter()
            .map(|s| (u16::from_be_bytes([s[0], s[1]]), u32be(&s[2..])))
    }
    /// The value in force for `id` after this frame, if the frame carried it.
    pub fn get(&self, id: u16) -> Option<u32> {
        self.iter().filter(|e| e.0 == id).last().map(|e| e.1)
    }
    pub fn is_empty(&self) -> bool {
        self.payload.is_empty()
    }
    pub fn to_settings(&self) -> Settings {
        let mut settings = Settings::new();
        settings.merge(self.iter());
        settings
    }
}
#[derive(Debug, Clone, Copy)]
pub struct Frame<'a> {
    pub kind: u8,
    pub flags: u8,
    pub stream: u32,
    pub payload: &'a [u8],
}
fn error(code: &'static str, message: &'static str) -> Error {
    Error::new(code, message)
}
fn protocol(message: &'static str) -> Error {
    error("PROTOCOL_ERROR", message)
}
fn frame_error() -> Error {
    error("FRAME_SIZE_ERROR", "invalid frame size")
}
fn u32be(b: &[u8]) -> u32 {
    u32::from_be_bytes(b[..4].try_into().unwrap())
}
pub fn decode_frame(input: &[u8], max: usize) -> Result<Option<Frame<'_>>> {
    if input.len() < 9 {
        return Ok(None);
    }
    let len = ((input[0] as usize) << 16) | ((input[1] as usize) << 8) | input[2] as usize;
    if len > max {
        return Err(frame_error());
    }
    if input.len() < 9 + len {
        return Ok(None);
    }
    Ok(Some(Frame {
        kind: input[3],
        flags: input[4],
        stream: u32be(&input[5..9]) & 0x7fffffff,
        payload: &input[9..9 + len],
    }))
}
pub fn encode_frame(
    kind: u8,
    flags: u8,
    stream: u32,
    payload: &[u8],
    out: &mut Vec<u8>,
) -> Result<()> {
    if payload.len() > 0xffffff || stream > 0x7fffffff {
        return Err(frame_error());
    }
    let len = payload.len() as u32;
    out.extend_from_slice(&len.to_be_bytes()[1..]);
    out.push(kind);
    out.push(flags);
    out.extend_from_slice(&stream.to_be_bytes());
    out.extend_from_slice(payload);
    Ok(())
}
/// Which of the three header blocks an [`Event::Headers`] carries. The
/// connection already enforces the distinction, so a host does not have to
/// track `received_head` itself to tell them apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadersKind {
    /// The request head, or a final (>= 200) response head.
    Head,
    /// A 1xx response head. It never ends the stream, and a final head follows.
    Informational,
    /// A trailer block. It always ends the stream.
    Trailers,
}
#[derive(Debug)]
pub enum Event<'a> {
    /// The peer's SETTINGS, already applied and acknowledged.
    Settings(SettingsFrame<'a>),
    /// The peer acknowledged the oldest SETTINGS frame this endpoint still had
    /// outstanding; its parameters are returned here and are now in force.
    /// Acknowledgements arrive in the order the frames were sent (RFC 9113
    /// section 6.5.3), and the first is always for the frame the constructor
    /// queued, so a host that timestamps construction and each
    /// [`Connection::settings`] call can pair them in FIFO order to measure a
    /// round trip. No clock is read here.
    SettingsAck(Settings),
    Headers {
        stream: u32,
        headers: Vec<Header>,
        end_stream: bool,
        kind: HeadersKind,
    },
    Data {
        stream: u32,
        bytes: &'a [u8],
        end_stream: bool,
    },
    /// The stream is over. It may name a stream the host never saw opened: a
    /// stream past the concurrent-stream limit is refused with
    /// RST_STREAM(REFUSED_STREAM) before any [`Event::Headers`] for it is
    /// produced (RFC 9113 section 5.1.2 requires a stream error there, and
    /// Node answers the same way — measured).
    Reset {
        stream: u32,
        code: u32,
    },
    /// The peer opened a stream after a graceful GOAWAY, above the last stream
    /// id that GOAWAY named, and **nothing has been sent for it**. RFC 9113
    /// section 6.8 makes such a stream simply "not processed" — the peer is
    /// expected to retry it on a new connection — and Node emits no frame at
    /// all here (measured against Node 26.5.1 with a raw peer: no RST_STREAM,
    /// no GOAWAY, the session stays alive and the request never reaches the
    /// application).
    ///
    /// The header block was decoded and discarded, because HPACK is connection
    /// state. The connection deliberately does not answer for the host: a frame
    /// it emitted could not be un-emitted, so a host that must match Node would
    /// have no way back. A host that wants to answer may call
    /// [`Connection::reset`].
    Unprocessed {
        stream: u32,
    },
    Goaway {
        last_stream: u32,
        code: u32,
        /// The opaque debug data (RFC 9113 section 6.8). `None` when the frame
        /// carried none - Node reports `undefined` there, not an empty buffer.
        /// The wire cannot express "present but empty", so this is never
        /// `Some` of an empty slice.
        debug: Option<&'a [u8]>,
    },
    Ping {
        ack: bool,
        data: [u8; 8],
    },
    WindowUpdate {
        stream: u32,
    },
}
/// One decode step: how much of `input` was consumed, and the event it
/// produced, if any. See the [step contract](crate#the-step-contract).
///
/// **`consumed` and `event` are independent.** All three shapes are normal:
///
/// | `consumed` | `event` | meaning |
/// |---|---|---|
/// | `0` | `None` | **stop.** A partial preface or a partial frame; read more input before calling again. |
/// | `> 0` | `None` | **keep going.** Progress with nothing for the host: the client preface, PRIORITY, an unknown frame type, or a frame the peer had in flight for a stream that is already gone. |
/// | `> 0` | `Some` | an event. In HTTP/2 an event always consumes; `consumed == 0` is only ever the stop case. |
///
/// So the loop condition is `consumed > 0 || event.is_some()`:
///
/// ```
/// # use turnloop_http::http2::{Connection, Limits, Role, PREFACE};
/// # let mut connection = Connection::new(Role::Server, Limits::default())?;
/// # let mut input = PREFACE.to_vec();
/// loop {
///     let step = connection.receive(&input)?;
///     let consumed = step.consumed;
///     let progressed = consumed > 0 || step.event.is_some();
///     if let Some(event) = step.event {
///         // DATA borrows `input`, so handle the event before draining.
///         let _ = event;
///     }
///     input.drain(..consumed);
///     if !progressed {
///         break; // read more bytes from the transport, then continue
///     }
/// }
/// # Ok::<(), turnloop_http::Error>(())
/// ```
///
/// Looping while `event.is_some()` instead **stalls at the client preface** —
/// the first step of every server connection, before a single frame is read.
/// Looping while input remains spins forever on a partial frame.
#[derive(Debug)]
pub struct Step<'a> {
    pub consumed: usize,
    pub event: Option<Event<'a>>,
}
/// What to do with a header block that does not belong to a live stream. Every
/// arm decodes the block first: HPACK is connection state, and a block that is
/// skipped desynchronises every block after it.
#[derive(Debug, Clone, Copy)]
enum Disposal {
    /// Past the concurrent-stream limit: answer RST_STREAM and report it.
    /// RFC 9113 section 5.1.2 requires a stream error here.
    Refuse(u32),
    /// Opened after our graceful GOAWAY: send nothing, report it, and let the
    /// host decide. See [`Event::Unprocessed`].
    Unprocessed,
    /// Frames the peer had in flight for a stream that is gone: discard in
    /// silence. The host already knows - it is the side that ended the stream.
    Discard,
}
#[derive(Debug)]
struct Stream {
    id: u32,
    send_window: i64,
    recv_window: i64,
    unreleased: u32,
    /// The peer reset this stream and its remaining connection credit was
    /// returned in bulk, so a late `release_capacity` for it is a no-op.
    credited: bool,
    local_end: bool,
    remote_end: bool,
    received_head: bool,
    sent_head: bool,
    recv_length: Option<u64>,
    received: u64,
    head_request: bool,
    recv_no_body: bool,
    send_no_body: bool,
    send_length: Option<u64>,
    sent: u64,
}
impl Stream {
    fn closed(&self) -> bool {
        self.local_end && self.remote_end
    }
}
pub struct Connection {
    role: Role,
    limits: Limits,
    streams: Vec<Stream>,
    next_id: u32,
    /// Highest remote stream id this connection has accepted **for processing**.
    /// GOAWAY names it, so a stream that was reported but not processed - one
    /// refused past the stream limit, or arriving after our own GOAWAY - must
    /// not advance it: RFC 9113 section 6.8 lets the peer retry everything above
    /// it on a new connection, and claiming one is processed loses that request.
    last_remote: u32,
    /// Highest remote stream id seen on the wire, processed or not. The
    /// monotonicity check and [`Connection::seen`] use this, so frames the peer
    /// had in flight for a stream we declined are still tolerated.
    highest_remote: u32,
    send_window: i64,
    recv_window: i64,
    initial_send: i64,
    peer_frame: usize,
    peer_streams: usize,
    preface: bool,
    settings_received: bool,
    /// SETTINGS frames sent and not yet acknowledged, oldest first. RFC 9113
    /// section 6.5.3 acknowledges them in order, so this is a queue: a second
    /// frame sent while one is outstanding is ordinary, not an error.
    settings_pending: VecDeque<Settings>,
    /// Every local parameter the peer has acknowledged, merged in order.
    local_settings: Settings,
    /// Every parameter the peer has sent, merged in order.
    remote_settings: Settings,
    /// The limits the acknowledged frames establish. `limits`, the ones
    /// enforced, is this relaxed by every outstanding frame: see
    /// `enforce_settings`.
    acked_limits: Limits,
    /// Our SETTINGS_INITIAL_WINDOW_SIZE as acknowledged, and as enforced.
    acked_recv_initial: i64,
    recv_initial: i64,
    decoder: hpack::Decoder,
    encoder: hpack::Encoder,
    block: Vec<u8>,
    scratch: Vec<u8>,
    continuation: Option<(u32, bool, usize)>,
    /// What to do with the header block being assembled, when it does not
    /// belong to a live stream. HPACK is connection state, so such a block
    /// still has to advance the decoder before it is thrown away.
    disposal: Option<Disposal>,
    output: Vec<u8>,
    output_pos: usize,
    draining: bool,
    failed: bool,
    settings_deadline: Option<Instant>,
}
impl Connection {
    /// A connection whose initial SETTINGS advertises `limits`: the
    /// concurrent-stream, frame-size and header-list limits, in that
    /// (ascending) identifier order, plus ENABLE_PUSH = 0 for a client.
    pub fn new(role: Role, limits: Limits) -> Result<Self> {
        Self::with_settings(role, limits, &Settings::from_limits(&limits))
    }
    /// A connection whose initial SETTINGS frame carries exactly `settings`,
    /// serialised in ascending identifier order. `Settings::new()` sends an
    /// empty frame, which is what Node's server sends.
    ///
    /// `limits` is what this endpoint enforces on its own. A parameter in
    /// `settings` that corresponds to a limit replaces it once the peer
    /// acknowledges the frame (and at once, if that relaxes it: the peer may
    /// act on the new value before its ack reaches us). One parameter is
    /// added: a client that does not name ENABLE_PUSH sends ENABLE_PUSH = 0,
    /// because push defaults to enabled and this crate treats PUSH_PROMISE
    /// as a connection error.
    pub fn with_settings(role: Role, limits: Limits, settings: &Settings) -> Result<Self> {
        if !(16384..=0xffffff).contains(&limits.frame_size)
            || limits.streams == 0
            || limits.streams > u32::MAX as usize
        {
            return Err(protocol("invalid limits"));
        }
        let mut settings = settings.clone();
        if role == Role::Client && settings.get(Settings::ENABLE_PUSH).is_none() {
            settings.set(Settings::ENABLE_PUSH, 0);
        }
        settings.validate()?;
        let mut result = Self {
            role,
            limits,
            streams: Vec::with_capacity(limits.streams),
            next_id: if role == Role::Client { 1 } else { 2 },
            last_remote: 0,
            highest_remote: 0,
            send_window: 65535,
            recv_window: 65535,
            initial_send: 65535,
            peer_frame: 16384,
            peer_streams: usize::MAX,
            preface: role == Role::Client,
            settings_received: false,
            settings_pending: VecDeque::new(),
            local_settings: Settings::new(),
            remote_settings: Settings::new(),
            acked_limits: limits,
            acked_recv_initial: 65535,
            recv_initial: 65535,
            decoder: hpack::Decoder::new(4096, limits.header_list),
            encoder: hpack::Encoder::new(4096),
            block: Vec::new(),
            scratch: Vec::new(),
            continuation: None,
            disposal: None,
            output: Vec::new(),
            output_pos: 0,
            draining: false,
            failed: false,
            settings_deadline: None,
        };
        if role == Role::Client {
            result.output.extend_from_slice(PREFACE);
        }
        result.send_settings(settings)?;
        Ok(result)
    }
    /// Send a SETTINGS frame carrying `settings` to a live connection -
    /// Node's `session.settings()`. It may be called while earlier frames
    /// are still unacknowledged; each is acknowledged in turn with
    /// [`Event::SettingsAck`].
    ///
    /// A parameter that loosens what this endpoint enforces - a larger
    /// MAX_FRAME_SIZE, INITIAL_WINDOW_SIZE, MAX_HEADER_LIST_SIZE or
    /// MAX_CONCURRENT_STREAMS - applies at once, because the peer may act on
    /// it before its acknowledgement arrives. One that tightens applies when
    /// the peer acknowledges the frame (RFC 9113 section 6.5.3), so frames the
    /// peer sent under the old value are never treated as errors.
    ///
    /// Values this endpoint could not honour are refused before anything is
    /// sent: ENABLE_PUSH other than 0, HEADER_TABLE_SIZE above 4096, and the
    /// out-of-range values RFC 9113 section 6.5.2 forbids.
    pub fn settings(&mut self, settings: &Settings) -> Result<()> {
        if self.failed {
            return Err(protocol("failed connection"));
        }
        settings.validate()?;
        self.send_settings(settings.clone())
    }
    fn send_settings(&mut self, settings: Settings) -> Result<()> {
        let payload = settings.encode();
        if payload.len() > self.peer_frame {
            return Err(frame_error());
        }
        self.frame(4, 0, 0, &payload)?;
        self.settings_pending.push_back(settings);
        self.enforce_settings();
        Ok(())
    }
    /// Recompute the enforced limits: the acknowledged ones, loosened by every
    /// frame still outstanding. The peer may already be acting on any of them.
    fn enforce_settings(&mut self) {
        let mut limits = self.acked_limits;
        let mut recv_initial = self.acked_recv_initial;
        for settings in &self.settings_pending {
            for (id, value) in settings.iter() {
                let value = value as usize;
                match id {
                    Settings::MAX_CONCURRENT_STREAMS => limits.streams = limits.streams.max(value),
                    Settings::INITIAL_WINDOW_SIZE => recv_initial = recv_initial.max(value as i64),
                    Settings::MAX_FRAME_SIZE => limits.frame_size = limits.frame_size.max(value),
                    Settings::MAX_HEADER_LIST_SIZE => {
                        limits.header_list = limits.header_list.max(value)
                    }
                    _ => {}
                }
            }
        }
        // RFC 9113 section 6.9.2: a change to the initial window adjusts every
        // open stream's window by the difference.
        let delta = recv_initial - self.recv_initial;
        if delta != 0 {
            for s in &mut self.streams {
                s.recv_window += delta;
            }
            self.recv_initial = recv_initial;
        }
        self.decoder.max_list_size = limits.header_list;
        self.limits = limits;
    }
    /// What this endpoint enforces now, including the effect of SETTINGS it
    /// has sent (see [`settings`](Self::settings)).
    pub fn limits(&self) -> Limits {
        self.limits
    }
    /// Every parameter this endpoint has sent and the peer has acknowledged -
    /// Node's `session.localSettings`, less the defaults nobody sent.
    pub fn local_settings(&self) -> &Settings {
        &self.local_settings
    }
    /// Every parameter the peer has sent - Node's `session.remoteSettings`,
    /// less the defaults the peer did not send.
    pub fn remote_settings(&self) -> &Settings {
        &self.remote_settings
    }
    /// SETTINGS frames sent and not yet acknowledged.
    pub fn pending_settings(&self) -> usize {
        self.settings_pending.len()
    }
    /// Host-supplied deadline for the oldest outstanding SETTINGS frame's
    /// acknowledgement; no clock is sampled. Each acknowledgement clears it, so
    /// a host with further frames outstanding sets the next one after each
    /// [`Event::SettingsAck`].
    pub fn set_settings_deadline(&mut self, deadline: Option<Instant>) {
        self.settings_deadline = deadline;
    }
    pub fn next_timeout(&self) -> Option<Instant> {
        if !self.settings_pending.is_empty() && !self.failed {
            self.settings_deadline
        } else {
            None
        }
    }
    pub fn handle_timeout(&mut self, now: Instant) -> Option<Error> {
        if self.next_timeout().is_some_and(|d| d <= now) {
            self.failed = true;
            let mut payload = [0; 8];
            payload[..4].copy_from_slice(&self.last_remote.to_be_bytes());
            payload[7] = 4;
            self.frame(7, 0, 0, &payload).expect("fixed-size GOAWAY");
            Some(error(
                "SETTINGS_TIMEOUT",
                "SETTINGS acknowledgement timeout",
            ))
        } else {
            None
        }
    }
    /// Notify transport loss, then call `poll_failed_stream` until exhausted.
    pub fn eof(&mut self) {
        self.failed = true;
    }
    /// Exactly one terminal result for each still-open stream after connection failure.
    /// Fully ended/reset streams have already produced their terminal event.
    pub fn poll_failed_stream(&mut self) -> Option<u32> {
        if !self.failed {
            return None;
        }
        let stream = self.streams.iter_mut().find(|s| !s.remote_end)?;
        stream.local_end = true;
        stream.remote_end = true;
        Some(stream.id)
    }
    pub fn output(&self) -> &[u8] {
        &self.output[self.output_pos..]
    }
    pub fn consume_output(&mut self, n: usize) -> Result<()> {
        if n > self.output().len() {
            return Err(protocol("write acknowledgement exceeds output"));
        }
        self.output_pos += n;
        if self.output_pos == self.output.len() {
            self.output.clear();
            self.output_pos = 0;
        }
        Ok(())
    }
    fn frame(&mut self, kind: u8, flags: u8, id: u32, payload: &[u8]) -> Result<()> {
        encode_frame(kind, flags, id, payload, &mut self.output)
    }
    fn index(&self, id: u32) -> Result<usize> {
        self.streams
            .iter()
            .position(|s| s.id == id)
            .ok_or(protocol("unknown or idle stream"))
    }
    fn active(&self) -> usize {
        self.streams.iter().filter(|s| !s.closed()).count()
    }
    /// A stream id this connection has already opened, refused or finished
    /// with. Its record may be gone — dropped by `reset`, never created for a
    /// refused stream, or recycled — while the peer, not yet having seen the
    /// RST_STREAM or GOAWAY, still sends frames for it. RFC 9113 section 5.1
    /// requires those to be tolerated; a frame for an id that was never used is
    /// still a connection error.
    fn seen(&self, id: u32) -> bool {
        if id == 0 {
            false
        } else if (id % 2 == 1) == (self.role == Role::Client) {
            id < self.next_id
        } else {
            id <= self.highest_remote
        }
    }
    /// `STREAM_CLOSED` for a stream this connection has finished with, and the
    /// idle/unknown protocol error for one it has never used.
    fn gone(&self, id: u32) -> Error {
        if self.seen(id) {
            error("STREAM_CLOSED", "stream already terminated")
        } else {
            protocol("unknown or idle stream")
        }
    }
    /// Give back the connection-level credit a terminated stream still holds.
    /// RFC 9113 section 5.1: DATA the peer sent before it saw our RST_STREAM
    /// counts against the connection window either way, so the credit has to go
    /// back or the window shrinks for the life of the connection. No
    /// stream-level WINDOW_UPDATE: that stream is over.
    fn return_credit(&mut self, i: usize) -> Result<()> {
        let n = std::mem::take(&mut self.streams[i].unreleased);
        if n == 0 {
            return Ok(());
        }
        self.recv_window += n as i64;
        self.frame(8, 0, 0, &n.to_be_bytes())
    }
    /// Account DATA for a stream that is gone against the connection window and
    /// return the credit at once: the payload is discarded, so no host release
    /// will ever arrive for it.
    fn discard_data(&mut self, flow: i64) -> Result<()> {
        if flow > self.recv_window {
            return Err(error("FLOW_CONTROL_ERROR", "receive window exceeded"));
        }
        if flow == 0 {
            return Ok(());
        }
        self.frame(8, 0, 0, &(flow as u32).to_be_bytes())
    }
    fn add_stream(&mut self, id: u32) -> Result<usize> {
        if self.active() >= self.limits.streams {
            return Err(error("REFUSED_STREAM", "stream limit"));
        }
        let stream = Stream {
            id,
            send_window: self.initial_send,
            recv_window: self.recv_initial,
            unreleased: 0,
            credited: false,
            local_end: false,
            remote_end: false,
            received_head: false,
            sent_head: false,
            recv_length: None,
            received: 0,
            head_request: false,
            recv_no_body: false,
            send_no_body: false,
            send_length: None,
            sent: 0,
        };
        if let Some(i) = self
            .streams
            .iter()
            .position(|s| s.closed() && s.unreleased == 0)
        {
            self.streams[i] = stream;
            Ok(i)
        } else {
            if self.streams.len() >= self.limits.streams {
                return Err(error(
                    "REFUSED_STREAM",
                    "release consumed capacity before opening streams",
                ));
            }
            self.streams.push(stream);
            Ok(self.streams.len() - 1)
        }
    }
    pub fn open(&mut self, headers: &[Header], end_stream: bool) -> Result<u32> {
        if self.role != Role::Client || self.draining || self.failed {
            return Err(protocol("cannot open stream"));
        }
        if self.active() >= self.peer_streams {
            return Err(error("REFUSED_STREAM", "peer stream limit"));
        }
        validate_headers(headers, true, false)?;
        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(2)
            .filter(|n| *n <= 0x7fffffff)
            .ok_or(protocol("stream IDs exhausted"))?;
        self.add_stream(id)?;
        self.send_headers(id, headers, end_stream)?;
        Ok(id)
    }
    pub fn send_headers(&mut self, id: u32, headers: &[Header], end_stream: bool) -> Result<()> {
        if self.failed {
            return Err(protocol("failed connection"));
        }
        let i = self.index(id).map_err(|_| self.gone(id))?;
        let s = &self.streams[i];
        if s.local_end {
            return Err(error("STREAM_CLOSED", "local stream closed"));
        }
        validate_headers(headers, self.role == Role::Client, s.sent_head)?;
        let informational = headers
            .iter()
            .find(|h| h.name == ":status")
            .is_some_and(|h| h.value.starts_with(b"1"));
        if informational && end_stream {
            return Err(protocol("informational END_STREAM"));
        }
        if s.sent_head && !end_stream {
            return Err(protocol("trailers must end stream"));
        }
        let no_body = self.role == Role::Server
            && (s.head_request
                || headers
                    .iter()
                    .any(|h| h.name == ":status" && matches!(h.value.as_slice(), b"204" | b"304")));
        let length = if s.sent_head {
            s.send_length
        } else if no_body {
            None
        } else {
            content_length(headers)?
        };
        if end_stream && length.is_some_and(|n| n != s.sent) {
            return Err(protocol("outgoing content-length mismatch"));
        }
        self.scratch.clear();
        self.encoder.encode(headers, &mut self.scratch);
        let count = self.scratch.len().max(1).div_ceil(self.peer_frame);
        for n in 0..count {
            let start = n * self.peer_frame;
            let end = (start + self.peer_frame).min(self.scratch.len());
            let flags =
                if n + 1 == count { 4 } else { 0 } | if n == 0 && end_stream { 1 } else { 0 };
            encode_frame(
                if n == 0 { 1 } else { 9 },
                flags,
                id,
                &self.scratch[start..end],
                &mut self.output,
            )?;
        }
        let s = &mut self.streams[i];
        s.sent_head |= !informational;
        s.send_no_body = no_body;
        s.send_length = length;
        s.local_end = end_stream;
        if self.role == Role::Client {
            s.head_request = headers
                .iter()
                .any(|h| h.name == ":method" && h.value == b"HEAD");
        }
        Ok(())
    }
    /// Returns accepted bytes (zero on flow-control stall). Host retains remainder.
    pub fn send_data(&mut self, id: u32, bytes: &[u8], end_stream: bool) -> Result<usize> {
        let i = self.index(id).map_err(|_| self.gone(id))?;
        let s = &self.streams[i];
        if self.failed || s.local_end || !s.sent_head {
            return Err(error("STREAM_CLOSED", "cannot send DATA"));
        }
        let n = bytes
            .len()
            .min(self.peer_frame)
            .min(self.send_window.max(0) as usize)
            .min(s.send_window.max(0) as usize);
        if n == 0 && !bytes.is_empty() {
            return Ok(0);
        }
        let end = end_stream && n == bytes.len();
        if s.send_no_body && !bytes.is_empty() {
            return Err(protocol("DATA on bodyless response"));
        }
        if s.send_length.is_some_and(|length| {
            s.sent + n as u64 > length || (end && s.sent + n as u64 != length)
        }) {
            return Err(protocol("outgoing content-length mismatch"));
        }
        self.frame(0, u8::from(end), id, &bytes[..n])?;
        self.send_window -= n as i64;
        let s = &mut self.streams[i];
        s.send_window -= n as i64;
        s.sent += n as u64;
        s.local_end = end;
        Ok(n)
    }
    /// Return `n` bytes of a stream's receive window to the peer; `n` may not
    /// exceed that stream's [`unreleased`](Self::unreleased) count.
    ///
    /// A terminated stream has already had its remaining credit returned in
    /// bulk, so a late release for one is a **no-op, not an error** — a host
    /// that buffers a body and releases when the application consumes it can
    /// always call this, whatever happened to the stream meanwhile. An id this
    /// connection has never used is still an error.
    pub fn release_capacity(&mut self, id: u32, n: u32) -> Result<()> {
        let Ok(i) = self.index(id) else {
            return if self.seen(id) {
                Ok(())
            } else {
                Err(protocol("unknown or idle stream"))
            };
        };
        if n == 0 || self.streams[i].credited {
            return Ok(());
        }
        let s = &mut self.streams[i];
        if n > s.unreleased {
            return Err(protocol("capacity exceeds received DATA"));
        }
        s.unreleased -= n;
        s.recv_window += n as i64;
        self.recv_window += n as i64;
        self.frame(8, 0, 0, &n.to_be_bytes())?;
        if !self.streams[i].remote_end {
            self.frame(8, 0, id, &n.to_be_bytes())?;
        }
        Ok(())
    }
    /// Received DATA bytes for `id` whose flow-control credit the host has not
    /// yet returned with [`release_capacity`](Self::release_capacity). `None`
    /// once the stream is gone, its credit having been returned in bulk.
    /// Padding never appears here: `receive` releases it as it is consumed.
    pub fn unreleased(&self, id: u32) -> Option<u32> {
        self.index(id).ok().map(|i| self.streams[i].unreleased)
    }
    /// Terminate a stream with RST_STREAM.
    ///
    /// The stream's outstanding connection-level credit is returned and its
    /// table slot is freed at once, so resetting a stream that still holds
    /// unreleased DATA — the ordinary "one stream errors while its siblings are
    /// live" case — costs the connection nothing. Frames the peer already had
    /// in flight for the stream are then ignored rather than failing the
    /// connection (RFC 9113 section 5.1).
    ///
    /// The frame is sent even when the stream has no record left — it may have
    /// been reset already, or refused before one existed, as after
    /// [`Event::Unprocessed`]. Encoding what the host asks for is this type's
    /// job; deciding not to is the host's. Only an id this connection has never
    /// used is an error.
    pub fn reset(&mut self, id: u32, code: u32) -> Result<()> {
        let Ok(i) = self.index(id) else {
            return if self.seen(id) {
                self.frame(3, 0, id, &code.to_be_bytes())
            } else {
                Err(protocol("unknown or idle stream"))
            };
        };
        self.return_credit(i)?;
        self.streams.swap_remove(i);
        self.frame(3, 0, id, &code.to_be_bytes())
    }
    pub fn ping(&mut self, data: [u8; 8]) -> Result<()> {
        self.frame(6, 0, 0, &data)
    }
    /// Graceful close: GOAWAY(NO_ERROR) naming the last stream this connection
    /// has processed, then drain. A stream the peer opens before it sees the
    /// frame does not fail the connection and is not answered: it arrives as
    /// [`Event::Unprocessed`] and the host decides.
    pub fn shutdown(&mut self) -> Result<()> {
        self.goaway(0, self.last_remote, &[])
    }
    /// GOAWAY with an explicit error code, last stream id and opaque debug
    /// data, the form [`shutdown`](Self::shutdown) cannot express. The
    /// connection starts draining either way.
    pub fn goaway(&mut self, code: u32, last_stream: u32, opaque: &[u8]) -> Result<()> {
        if last_stream > 0x7fffffff {
            return Err(protocol("invalid last stream"));
        }
        // The peer would answer a GOAWAY over its own SETTINGS_MAX_FRAME_SIZE
        // with FRAME_SIZE_ERROR, so refuse the opaque data here instead.
        if 8 + opaque.len() > self.peer_frame {
            return Err(frame_error());
        }
        self.draining = true;
        self.scratch.clear();
        self.scratch.extend_from_slice(&last_stream.to_be_bytes());
        self.scratch.extend_from_slice(&code.to_be_bytes());
        self.scratch.extend_from_slice(opaque);
        encode_frame(7, 0, 0, &self.scratch, &mut self.output)
    }
    pub fn is_drained(&self) -> bool {
        self.draining && self.active() == 0 && self.output().is_empty()
    }
    pub fn receive<'a>(&mut self, input: &'a [u8]) -> Result<Step<'a>> {
        if self.failed {
            return Err(protocol("failed connection"));
        }
        let result = self.receive_inner(input);
        if let Err(e) = &result {
            self.failed = true;
            let code = match e.code {
                "FLOW_CONTROL_ERROR" => 3,
                "FRAME_SIZE_ERROR" => 6,
                "COMPRESSION_ERROR" => 9,
                "ENHANCE_YOUR_CALM" => 11,
                _ => 1,
            };
            let mut payload = [0; 8];
            payload[..4].copy_from_slice(&self.last_remote.to_be_bytes());
            payload[4..].copy_from_slice(&(code as u32).to_be_bytes());
            self.frame(7, 0, 0, &payload)?;
        }
        result
    }
    fn receive_inner<'a>(&mut self, input: &'a [u8]) -> Result<Step<'a>> {
        if !self.preface {
            let n = input.len().min(PREFACE.len());
            if input[..n] != PREFACE[..n] {
                return Err(protocol("invalid client preface"));
            }
            if n < PREFACE.len() {
                return Ok(Step {
                    consumed: 0,
                    event: None,
                });
            }
            self.preface = true;
            return Ok(Step {
                consumed: PREFACE.len(),
                event: None,
            });
        }
        let Some(f) = decode_frame(input, self.limits.frame_size)? else {
            return Ok(Step {
                consumed: 0,
                event: None,
            });
        };
        if !self.settings_received && (f.kind != 4 || f.flags & 1 != 0) {
            return Err(protocol("first frame must be SETTINGS"));
        }
        if let Some((id, _, _)) = self.continuation {
            if f.kind != 9 || f.stream != id {
                return Err(protocol("interleaved header block"));
            }
        } else if f.kind == 9 {
            return Err(protocol("unexpected CONTINUATION"));
        }
        let mut step = Step {
            consumed: 9 + f.payload.len(),
            event: None,
        };
        match f.kind {
            0 => {
                if f.stream == 0 {
                    return Err(protocol("DATA on connection"));
                }
                let Ok(i) = self.index(f.stream) else {
                    // In flight when the stream was refused or reset. RFC 9113
                    // section 5.1 tolerates it, and the bytes still count
                    // against the connection window, so return the credit.
                    if !self.seen(f.stream) {
                        return Err(protocol("unknown or idle stream"));
                    }
                    self.discard_data(f.payload.len() as i64)?;
                    return Ok(step);
                };
                let s = &mut self.streams[i];
                if s.remote_end || !s.received_head {
                    return Err(protocol("DATA in invalid stream state"));
                }
                let payload = unpadded(f)?;
                let flow = f.payload.len() as i64;
                if flow > self.recv_window || flow > s.recv_window {
                    return Err(error("FLOW_CONTROL_ERROR", "receive window exceeded"));
                }
                self.recv_window -= flow;
                s.recv_window -= flow;
                s.unreleased += flow as u32;
                s.received += payload.len() as u64;
                let end = f.flags & 1 != 0;
                if s.recv_no_body && !payload.is_empty() {
                    return Err(protocol("DATA on bodyless response"));
                }
                if s.recv_length
                    .is_some_and(|n| s.received > n || (end && n != s.received))
                {
                    return Err(protocol("content-length mismatch"));
                }
                s.remote_end = end;
                // Padding is consumed by the protocol, never charged to the application.
                let padding = f.payload.len() - payload.len();
                if padding > 0 {
                    self.release_capacity(f.stream, padding as u32)?;
                }
                step.event = Some(Event::Data {
                    stream: f.stream,
                    bytes: payload,
                    end_stream: end,
                });
            }
            1 => {
                if f.stream == 0 {
                    return Err(protocol("HEADERS on connection"));
                }
                self.disposal = None;
                if self.index(f.stream).is_err() {
                    if self.role != Role::Server {
                        // A response the peer already had in flight when we
                        // reset the stream. Its record is gone, so decode the
                        // block to keep HPACK in step and ignore it (RFC 9113
                        // section 5.1). Only a stream we opened ourselves is
                        // unambiguous here; anything else is idle.
                        if !self.seen(f.stream) {
                            return Err(protocol("invalid new stream"));
                        }
                        self.disposal = Some(Disposal::Discard);
                    } else if f.stream % 2 == 0 || f.stream <= self.highest_remote {
                        return Err(protocol("invalid new stream"));
                    } else {
                        self.highest_remote = f.stream;
                        // A stream this connection cannot accept never fails
                        // the connection. The two reasons answer differently,
                        // and both were measured against Node 26.5.1 with a raw
                        // peer rather than assumed:
                        //
                        // * after a graceful GOAWAY, the stream is above the id
                        //   that GOAWAY named and is simply not processed (RFC
                        //   9113 section 6.8). Node sends no frame at all, so
                        //   neither do we - the host is told and decides.
                        // * past the concurrent-stream limit, RFC 9113 section
                        //   5.1.2 requires a stream error, and Node answers
                        //   RST_STREAM(REFUSED_STREAM).
                        self.disposal = if self.draining {
                            Some(Disposal::Unprocessed)
                        } else {
                            match self.add_stream(f.stream) {
                                Ok(_) => {
                                    // Accepted for processing, so GOAWAY may name it.
                                    self.last_remote = f.stream;
                                    None
                                }
                                Err(e) if e.code == "REFUSED_STREAM" => Some(Disposal::Refuse(7)),
                                Err(e) => return Err(e),
                            }
                        };
                    }
                }
                let mut payload = unpadded(f)?;
                if f.flags & 32 != 0 {
                    if payload.len() < 5 {
                        return Err(frame_error());
                    }
                    if u32be(payload) & 0x7fffffff == f.stream {
                        return Err(protocol("stream depends on itself"));
                    }
                    payload = &payload[5..];
                }
                self.block.clear();
                self.append_block(payload)?;
                let end = f.flags & 1 != 0;
                if f.flags & 4 != 0 {
                    step.event = self.finish_block(f.stream, end)?;
                } else {
                    self.continuation = Some((f.stream, end, 0));
                }
            }
            9 => {
                let (id, end, count) = self.continuation.unwrap();
                if count >= self.limits.continuations {
                    return Err(error("ENHANCE_YOUR_CALM", "CONTINUATION limit"));
                }
                self.append_block(f.payload)?;
                if f.flags & 4 != 0 {
                    self.continuation = None;
                    step.event = self.finish_block(id, end)?;
                } else {
                    self.continuation = Some((id, end, count + 1));
                }
            }
            2 => {
                if f.stream == 0 {
                    return Err(protocol("PRIORITY on connection"));
                }
                if f.payload.len() != 5 {
                    return Err(frame_error());
                }
                if u32be(f.payload) & 0x7fffffff == f.stream {
                    return Err(protocol("stream depends on itself"));
                }
            }
            3 => {
                if f.stream == 0 {
                    return Err(protocol("RST_STREAM on connection"));
                }
                if f.payload.len() != 4 {
                    return Err(frame_error());
                }
                let Ok(i) = self.index(f.stream) else {
                    // Crossed resets, or a reset for a stream already refused.
                    if !self.seen(f.stream) {
                        return Err(protocol("unknown or idle stream"));
                    }
                    return Ok(step);
                };
                self.return_credit(i)?;
                let s = &mut self.streams[i];
                s.credited = true;
                s.remote_end = true;
                s.local_end = true;
                step.event = Some(Event::Reset {
                    stream: f.stream,
                    code: u32be(f.payload),
                });
            }
            4 => {
                if f.stream != 0 {
                    return Err(protocol("SETTINGS on stream"));
                }
                if f.flags & 1 != 0 {
                    if !f.payload.is_empty() {
                        return Err(frame_error());
                    }
                    let Some(acked) = self.settings_pending.pop_front() else {
                        return Err(protocol("unsolicited SETTINGS ack"));
                    };
                    for (id, value) in acked.iter() {
                        match id {
                            Settings::MAX_CONCURRENT_STREAMS => {
                                self.acked_limits.streams = value as usize
                            }
                            Settings::INITIAL_WINDOW_SIZE => self.acked_recv_initial = value as i64,
                            Settings::MAX_FRAME_SIZE => {
                                self.acked_limits.frame_size = value as usize
                            }
                            Settings::MAX_HEADER_LIST_SIZE => {
                                self.acked_limits.header_list = value as usize
                            }
                            _ => {}
                        }
                    }
                    self.local_settings.merge(acked.iter());
                    self.enforce_settings();
                    // The deadline was for this frame; the host arms the next.
                    self.settings_deadline = None;
                    step.event = Some(Event::SettingsAck(acked));
                } else {
                    if f.payload.len() % 6 != 0 {
                        return Err(frame_error());
                    }
                    for setting in f.payload.as_chunks::<6>().0 {
                        let id = u16::from_be_bytes(setting[..2].try_into().unwrap());
                        let value = u32be(&setting[2..]);
                        match id {
                            1 => self.encoder.set_table_size((value as usize).min(4096)),
                            2 => {
                                if value > 1 || self.role == Role::Client {
                                    return Err(protocol("invalid ENABLE_PUSH"));
                                }
                            }
                            3 => self.peer_streams = value as usize,
                            4 => {
                                if value > 0x7fffffff {
                                    return Err(error(
                                        "FLOW_CONTROL_ERROR",
                                        "invalid initial window",
                                    ));
                                }
                                let delta = value as i64 - self.initial_send;
                                for s in &mut self.streams {
                                    if s.send_window + delta > 0x7fffffff {
                                        return Err(error("FLOW_CONTROL_ERROR", "window overflow"));
                                    }
                                    s.send_window += delta;
                                }
                                self.initial_send = value as i64;
                            }
                            5 => {
                                if !(16384..=0xffffff).contains(&value) {
                                    return Err(protocol("invalid MAX_FRAME_SIZE"));
                                }
                                self.peer_frame = value as usize;
                            }
                            _ => {}
                        }
                    }
                    self.settings_received = true;
                    let settings = SettingsFrame { payload: f.payload };
                    self.remote_settings.merge(settings.iter());
                    self.frame(4, 1, 0, &[])?;
                    step.event = Some(Event::Settings(settings));
                }
            }
            5 => return Err(protocol("server push disabled")),
            6 => {
                if f.stream != 0 {
                    return Err(protocol("PING on stream"));
                }
                if f.payload.len() != 8 {
                    return Err(frame_error());
                }
                let data = f.payload.try_into().unwrap();
                let ack = f.flags & 1 != 0;
                if !ack {
                    self.frame(6, 1, 0, f.payload)?;
                }
                step.event = Some(Event::Ping { ack, data });
            }
            7 => {
                if f.stream != 0 {
                    return Err(protocol("GOAWAY on stream"));
                }
                if f.payload.len() < 8 {
                    return Err(frame_error());
                }
                self.draining = true;
                step.event = Some(Event::Goaway {
                    last_stream: u32be(f.payload) & 0x7fffffff,
                    code: u32be(&f.payload[4..]),
                    debug: Some(&f.payload[8..]).filter(|d| !d.is_empty()),
                });
            }
            8 => {
                if f.payload.len() != 4 {
                    return Err(frame_error());
                }
                if f.stream != 0 && self.index(f.stream).is_err() {
                    // Credit for a stream that is already gone. RFC 9113
                    // section 6.9 requires this to be tolerated.
                    if !self.seen(f.stream) {
                        return Err(protocol("unknown or idle stream"));
                    }
                    return Ok(step);
                }
                let n = (u32be(f.payload) & 0x7fffffff) as i64;
                if n == 0 {
                    if f.stream != 0 {
                        self.reset(f.stream, 1)?;
                        step.event = Some(Event::Reset {
                            stream: f.stream,
                            code: 1,
                        });
                        return Ok(step);
                    }
                    return Err(protocol("zero WINDOW_UPDATE"));
                }
                let window = if f.stream == 0 {
                    &mut self.send_window
                } else {
                    let i = self.index(f.stream)?;
                    &mut self.streams[i].send_window
                };
                if *window + n > 0x7fffffff {
                    if f.stream != 0 {
                        self.reset(f.stream, 3)?;
                        step.event = Some(Event::Reset {
                            stream: f.stream,
                            code: 3,
                        });
                        return Ok(step);
                    }
                    return Err(error("FLOW_CONTROL_ERROR", "window overflow"));
                }
                *window += n;
                step.event = Some(Event::WindowUpdate { stream: f.stream });
            }
            _ => {}
        }
        Ok(step)
    }
    fn append_block(&mut self, bytes: &[u8]) -> Result<()> {
        if bytes.len() > self.limits.header_block.saturating_sub(self.block.len()) {
            return Err(error("ENHANCE_YOUR_CALM", "compressed header block limit"));
        }
        self.block.extend_from_slice(bytes);
        Ok(())
    }
    /// Complete a header block: the stream's own, or one being thrown away.
    fn finish_block(&mut self, id: u32, end: bool) -> Result<Option<Event<'static>>> {
        let Some(disposal) = self.disposal.take() else {
            return self.finish_headers(id, end).map(Some);
        };
        // Decode before discarding: HPACK state belongs to the connection, not
        // the stream, and a skipped block desynchronises every block after it.
        let mut headers = Vec::new();
        self.decoder.decode(&self.block, &mut headers)?;
        match disposal {
            Disposal::Refuse(code) => {
                self.frame(3, 0, id, &code.to_be_bytes())?;
                Ok(Some(Event::Reset { stream: id, code }))
            }
            Disposal::Unprocessed => Ok(Some(Event::Unprocessed { stream: id })),
            Disposal::Discard => Ok(None),
        }
    }
    fn finish_headers(&mut self, id: u32, end: bool) -> Result<Event<'static>> {
        let mut headers = Vec::new();
        self.decoder.decode(&self.block, &mut headers)?;
        let i = self.index(id)?;
        let s = &mut self.streams[i];
        if s.remote_end {
            return Err(error("STREAM_CLOSED", "headers after END_STREAM"));
        }
        validate_headers(&headers, self.role == Role::Server, s.received_head)?;
        let informational = headers
            .iter()
            .find(|h| h.name == ":status")
            .is_some_and(|h| h.value.starts_with(b"1"));
        if informational && end {
            return Err(protocol("informational END_STREAM"));
        }
        if s.received_head && !end {
            return Err(protocol("trailers without END_STREAM"));
        }
        let kind = if informational {
            HeadersKind::Informational
        } else if s.received_head {
            HeadersKind::Trailers
        } else {
            HeadersKind::Head
        };
        if !s.received_head && !informational {
            s.recv_no_body = self.role == Role::Client
                && (s.head_request
                    || headers.iter().any(|h| {
                        h.name == ":status" && matches!(h.value.as_slice(), b"204" | b"304")
                    }));
            s.recv_length = if s.recv_no_body {
                None
            } else {
                content_length(&headers)?
            };
            if self.role == Role::Server {
                s.head_request = headers
                    .iter()
                    .any(|h| h.name == ":method" && h.value == b"HEAD");
            }
            s.received_head = true;
        }
        if end && s.recv_length.is_some_and(|n| n != s.received) {
            return Err(protocol("content-length mismatch"));
        }
        s.remote_end = end;
        Ok(Event::Headers {
            stream: id,
            headers,
            end_stream: end,
            kind,
        })
    }
}
fn unpadded(f: Frame<'_>) -> Result<&[u8]> {
    if f.flags & 8 == 0 {
        return Ok(f.payload);
    }
    let pad = *f.payload.first().ok_or_else(frame_error)? as usize;
    if pad >= f.payload.len() {
        return Err(protocol("invalid padding"));
    }
    Ok(&f.payload[1..f.payload.len() - pad])
}
fn content_length(headers: &[Header]) -> Result<Option<u64>> {
    let mut found = None;
    for h in headers {
        if h.name == "content-length" {
            if found.is_some() || h.value.is_empty() || !h.value.iter().all(u8::is_ascii_digit) {
                return Err(protocol("invalid content-length"));
            }
            found = Some(
                h.value
                    .iter()
                    .try_fold(0u64, |n, b| {
                        n.checked_mul(10)?.checked_add((b - b'0') as u64)
                    })
                    .ok_or(protocol("content-length overflow"))?,
            );
        }
    }
    Ok(found)
}
fn validate_headers(headers: &[Header], request: bool, trailers: bool) -> Result<()> {
    let mut regular = false;
    let mut pseudo = [false; 5];
    for h in headers {
        if h.name.is_empty()
            || h.name.bytes().any(|b| b.is_ascii_uppercase())
            || h.value.iter().any(|b| matches!(b, 0 | 10 | 13))
            || h.value.first().is_some_and(|b| matches!(b, b' ' | b'\t'))
            || h.value.last().is_some_and(|b| matches!(b, b' ' | b'\t'))
        {
            return Err(protocol("invalid header field"));
        }
        if h.name.starts_with(':') {
            if regular || trailers {
                return Err(protocol("pseudo-header ordering"));
            }
            let index = match h.name.as_str() {
                ":method" if request => 0,
                ":scheme" if request => 1,
                ":path" if request => 2,
                ":authority" if request => 3,
                ":status" if !request => 4,
                _ => return Err(protocol("invalid pseudo-header")),
            };
            if pseudo[index] {
                return Err(protocol("duplicate pseudo-header"));
            }
            pseudo[index] = true;
        } else {
            regular = true;
            if !crate::http1::valid_token(h.name.as_bytes()) {
                return Err(protocol("invalid header name"));
            }
            if matches!(
                h.name.as_str(),
                "connection" | "proxy-connection" | "keep-alive" | "transfer-encoding" | "upgrade"
            ) || h.name == "te" && h.value != b"trailers"
            {
                return Err(protocol("connection-specific field"));
            }
        }
    }
    if !trailers {
        if request {
            let connect = headers
                .iter()
                .any(|h| h.name == ":method" && h.value == b"CONNECT");
            if !pseudo[0]
                || if connect {
                    !pseudo[3] || pseudo[1] || pseudo[2]
                } else {
                    !pseudo[1]
                        || !pseudo[2]
                        || headers
                            .iter()
                            .any(|h| h.name == ":path" && h.value.is_empty())
                }
            {
                return Err(protocol("missing request pseudo-header"));
            }
        } else {
            let Some(status) = headers.iter().find(|h| h.name == ":status") else {
                return Err(protocol("missing status"));
            };
            if status.value.len() != 3
                || !status.value.iter().all(u8::is_ascii_digit)
                || status.value.as_slice() < b"100".as_slice()
                || status.value == b"101"
            {
                return Err(protocol("invalid status"));
            }
        }
    }
    if trailers && headers.iter().any(|h| h.name == "content-length") {
        return Err(protocol("content-length in trailers"));
    }
    content_length(headers)?;
    Ok(())
}
