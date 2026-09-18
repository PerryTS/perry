// Minimal HTTP/2 wire codec for the `node:http2` conformance fixtures.
//
// WHY THIS EXISTS: Perry's `node:http2` control surface (`session.settings()`,
// `.ping()`, `.goaway()`) is a LOOPBACK SIMULATION — it scans the process's
// own handle table for a peer `Http2SessionHandle` and pushes a synthetic
// event, without ever encoding a frame (perry-ext-http
// `server/http2_server/controls.rs`). Any fixture with Perry on BOTH ends is
// therefore satisfied by the simulation and proves nothing.
//
// Every fixture that imports this module puts a RAW TCP SOCKET on one end and
// hand-encodes/decodes HTTP/2 frames, so the bytes on the wire are the subject
// of the test. The loopback path cannot satisfy them: there is no second
// session handle in the process to find.
//
// Deliberately byte-level: frame headers are assembled and parsed with plain
// index arithmetic rather than `Buffer.readUIntBE`, so a Buffer-method gap
// cannot make an http2 fixture fail for an unrelated reason.
//
// HPACK: only the encoder is here, and only "literal header field without
// indexing, new name, no Huffman" (RFC 7541 6.2.2) — a 0x00 prefix byte, then
// length-prefixed name and value. That is enough to OPEN real streams. No
// decoder: every assertion in this set reads control frames (SETTINGS, PING,
// GOAWAY, RST_STREAM, WINDOW_UPDATE, DATA), none of which carry a header
// block, so no Huffman table is needed.
import { Buffer } from "node:buffer";
import net from "node:net";

export const PREFACE_TEXT = "PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";
export const PREFACE = Buffer.from(PREFACE_TEXT, "latin1");

export const FRAME_DATA = 0;
export const FRAME_HEADERS = 1;
export const FRAME_RST_STREAM = 3;
export const FRAME_SETTINGS = 4;
export const FRAME_PING = 6;
export const FRAME_GOAWAY = 7;
export const FRAME_WINDOW_UPDATE = 8;

export const FLAG_ACK = 0x1;
export const FLAG_END_STREAM = 0x1;
export const FLAG_END_HEADERS = 0x4;

export const SETTING_HEADER_TABLE_SIZE = 0x1;
export const SETTING_ENABLE_PUSH = 0x2;
export const SETTING_MAX_CONCURRENT_STREAMS = 0x3;
export const SETTING_INITIAL_WINDOW_SIZE = 0x4;
export const SETTING_MAX_FRAME_SIZE = 0x5;
export const SETTING_MAX_HEADER_LIST_SIZE = 0x6;

const FRAME_NAMES = [
  "DATA",
  "HEADERS",
  "PRIORITY",
  "RST_STREAM",
  "SETTINGS",
  "PUSH_PROMISE",
  "PING",
  "GOAWAY",
  "WINDOW_UPDATE",
  "CONTINUATION",
];

export interface H2Frame {
  type: number;
  flags: number;
  streamId: number;
  payload: Buffer;
  length: number;
}

function u32(buf: Buffer, off: number): number {
  return (
    buf[off] * 16777216 + buf[off + 1] * 65536 + buf[off + 2] * 256 + buf[off + 3]
  );
}

function putU32(buf: Buffer, off: number, v: number): void {
  buf[off] = (v / 16777216) & 0xff;
  buf[off + 1] = (v / 65536) & 0xff;
  buf[off + 2] = (v / 256) & 0xff;
  buf[off + 3] = v & 0xff;
}

/** Assemble one HTTP/2 frame: 9-byte header + payload. */
export function frame(
  type: number,
  flags: number,
  streamId: number,
  payload?: Buffer,
): Buffer {
  const body = payload === undefined ? Buffer.alloc(0) : payload;
  const head = Buffer.alloc(9);
  head[0] = (body.length / 65536) & 0xff;
  head[1] = (body.length / 256) & 0xff;
  head[2] = body.length & 0xff;
  head[3] = type;
  head[4] = flags;
  putU32(head, 5, streamId);
  return Buffer.concat([head, body]);
}

/** SETTINGS payload: 6-byte records of (u16 id, u32 value), in the given order. */
export function settingsPayload(pairs: Array<Array<number>>): Buffer {
  const out = Buffer.alloc(pairs.length * 6);
  for (let i = 0; i < pairs.length; i++) {
    const off = i * 6;
    out[off] = (pairs[i][0] / 256) & 0xff;
    out[off + 1] = pairs[i][0] & 0xff;
    putU32(out, off + 2, pairs[i][1]);
  }
  return out;
}

/** GOAWAY payload: lastStreamID, errorCode, opaque data. */
export function goawayPayload(
  lastStreamId: number,
  errorCode: number,
  opaque?: Buffer,
): Buffer {
  const tail = opaque === undefined ? Buffer.alloc(0) : opaque;
  const out = Buffer.alloc(8 + tail.length);
  putU32(out, 0, lastStreamId);
  putU32(out, 4, errorCode);
  tail.copy(out, 8);
  return out;
}

/** WINDOW_UPDATE payload: a single u31 increment. */
export function windowUpdatePayload(increment: number): Buffer {
  const out = Buffer.alloc(4);
  putU32(out, 0, increment);
  return out;
}

/** RST_STREAM payload: a single u32 error code. */
export function rstPayload(errorCode: number): Buffer {
  const out = Buffer.alloc(4);
  putU32(out, 0, errorCode);
  return out;
}

/** HPACK literal header field without indexing, new name, no Huffman. */
export function hpackLiteral(name: string, value: string): Buffer {
  const n = Buffer.from(name, "latin1");
  const v = Buffer.from(value, "latin1");
  return Buffer.concat([
    Buffer.from([0x00]),
    Buffer.from([n.length]),
    n,
    Buffer.from([v.length]),
    v,
  ]);
}

export function headerBlock(headers: Array<Array<string>>): Buffer {
  const parts: Buffer[] = [];
  for (let i = 0; i < headers.length; i++) {
    parts.push(hpackLiteral(headers[i][0], headers[i][1]));
  }
  return Buffer.concat(parts);
}

/** A GET request HEADERS frame (END_HEADERS|END_STREAM) for `path`. */
export function requestFrame(
  streamId: number,
  path: string,
  method?: string,
  endStream?: boolean,
): Buffer {
  const flags =
    FLAG_END_HEADERS | (endStream === false ? 0 : FLAG_END_STREAM);
  return frame(
    FRAME_HEADERS,
    flags,
    streamId,
    headerBlock([
      [":method", method === undefined ? "GET" : method],
      [":scheme", "http"],
      [":authority", "127.0.0.1"],
      [":path", path],
    ]),
  );
}

/**
 * Human-readable, byte-exact one-line rendering of a frame. This is what the
 * fixtures print, so the gap diff is a diff of the wire.
 */
export function describe(f: H2Frame): string {
  const name =
    f.type < FRAME_NAMES.length ? FRAME_NAMES[f.type] : "UNKNOWN(" + f.type + ")";
  let extra = "";
  if (f.type === FRAME_SETTINGS) {
    if (f.flags & FLAG_ACK) {
      extra = " ACK len=" + f.length;
    } else {
      const parts: string[] = [];
      for (let i = 0; i + 6 <= f.length; i += 6) {
        parts.push(
          f.payload[i] * 256 + f.payload[i + 1] + "=" + u32(f.payload, i + 2),
        );
      }
      extra = " {" + parts.join(",") + "}";
    }
  } else if (f.type === FRAME_PING) {
    extra =
      (f.flags & FLAG_ACK ? " ACK" : "") +
      " len=" +
      f.length +
      " " +
      f.payload.toString("hex");
  } else if (f.type === FRAME_GOAWAY) {
    extra =
      " last=" +
      u32(f.payload, 0) +
      " code=" +
      u32(f.payload, 4) +
      " opaque=" +
      JSON.stringify(f.payload.subarray(8).toString("latin1"));
  } else if (f.type === FRAME_RST_STREAM) {
    extra = " code=" + u32(f.payload, 0);
  } else if (f.type === FRAME_WINDOW_UPDATE) {
    extra = " inc=" + u32(f.payload, 0);
  } else if (f.type === FRAME_DATA) {
    extra = " len=" + f.length + (f.flags & FLAG_END_STREAM ? " END_STREAM" : "");
  } else if (f.type === FRAME_HEADERS) {
    extra = " len=" + f.length + " flags=0x" + f.flags.toString(16);
  }
  return name + " stream=" + f.streamId + extra;
}

/** Incremental frame splitter. Feed it socket chunks; it calls back per frame. */
export function feedFrames(
  state: { buf: Buffer },
  chunk: Buffer,
  onFrame: (f: H2Frame) => void,
): void {
  state.buf = Buffer.concat([state.buf, chunk]);
  for (;;) {
    if (state.buf.length < 9) return;
    const len = state.buf[0] * 65536 + state.buf[1] * 256 + state.buf[2];
    if (state.buf.length < 9 + len) return;
    const f: H2Frame = {
      type: state.buf[3],
      flags: state.buf[4],
      streamId: u32(state.buf, 5) & 0x7fffffff,
      payload: Buffer.from(state.buf.subarray(9, 9 + len)),
      length: len,
    };
    state.buf = Buffer.from(state.buf.subarray(9 + len));
    onFrame(f);
  }
}

export function sleep(ms: number): Promise<void> {
  return new Promise<void>((resolve) => setTimeout(resolve, ms));
}

/**
 * Bound an event-driven await so a non-delivering implementation produces a
 * READABLE diff instead of eating the harness timeout. Node always wins these
 * races by orders of magnitude, so the fallback never appears in the oracle
 * output; an implementation that never fires the event prints the fallback and
 * the fixture fails on that line rather than hanging.
 */
export function withTimeout<T>(p: Promise<T>, ms: number, onTimeout: T): Promise<T> {
  return new Promise<T>((resolve) => {
    let done = false;
    const timer = setTimeout(() => {
      if (!done) {
        done = true;
        resolve(onTimeout);
      }
    }, ms);
    p.then((value: T) => {
      if (!done) {
        done = true;
        clearTimeout(timer);
        resolve(value);
      }
    });
  });
}

/** `withTimeout` for a void barrier; resolves either way. */
export function barrier(p: Promise<void>, ms: number): Promise<void> {
  return withTimeout<void>(p, ms, undefined as unknown as void);
}

/**
 * Await an EventEmitter event, bounded. Returns true if it fired.
 *
 * Deliberately `on(...)` and not `once(...)`: `once` is NOT in the method
 * surface of perry-ext-http's http2 session handle
 * (`server/http2_server/dispatch.rs` accepts only `on` / `addListener`), and a
 * barrier that fails because the registration method is missing would make
 * every fixture in this set fail for the wrong reason. The one-shot guard here
 * gives `once` semantics over the method both implementations have.
 * (`typeof session.once` is probed on its own in
 * `test_gap_http2_e2e_streams.ts`.)
 */
export function waitEvent(target: any, event: string, ms: number): Promise<boolean> {
  return withTimeout<boolean>(
    new Promise<boolean>((resolve) => {
      let fired = false;
      target.on(event, () => {
        if (!fired) {
          fired = true;
          resolve(true);
        }
      });
    }),
    ms,
    false,
  );
}

/**
 * A raw HTTP/2 CLIENT peer: connects to `port`, sends the connection preface
 * and an (optionally empty) SETTINGS frame, and records every frame the
 * server sends.
 */
export class RawClientPeer {
  socket: any;
  log: string[] = [];
  frames: H2Frame[] = [];
  state: { buf: Buffer } = { buf: Buffer.alloc(0) };
  dataBytes = 0;
  dataFrames = 0;
  closed = false;
  /** ACK inbound SETTINGS and PING automatically (so the peer looks alive). */
  autoAck = true;

  constructor(socket: any) {
    this.socket = socket;
    const self = this;
    socket.on("error", () => {});
    socket.on("close", () => {
      self.closed = true;
    });
    socket.on("data", (chunk: Buffer) => {
      feedFrames(self.state, chunk, (f: H2Frame) => {
        self.frames.push(f);
        if (f.type === FRAME_DATA) {
          self.dataBytes += f.length;
          self.dataFrames += 1;
        }
        self.log.push(describe(f));
        if (self.autoAck) {
          if (f.type === FRAME_SETTINGS && !(f.flags & FLAG_ACK)) {
            socket.write(frame(FRAME_SETTINGS, FLAG_ACK, 0));
          }
          if (f.type === FRAME_PING && !(f.flags & FLAG_ACK)) {
            socket.write(frame(FRAME_PING, FLAG_ACK, 0, f.payload));
          }
        }
      });
    });
  }

  static async connect(port: number, settings?: Buffer): Promise<RawClientPeer> {
    const socket = net.connect(port, "127.0.0.1");
    await new Promise<void>((resolve) => socket.on("connect", () => resolve()));
    const peer = new RawClientPeer(socket);
    socket.write(PREFACE);
    socket.write(
      frame(
        FRAME_SETTINGS,
        0,
        0,
        settings === undefined ? Buffer.alloc(0) : settings,
      ),
    );
    return peer;
  }

  send(buf: Buffer): void {
    this.socket.write(buf);
  }

  /** Drain and return the recorded frame lines, then reset the log. */
  take(): string[] {
    const out = this.log.slice();
    this.log = [];
    return out;
  }

  destroy(): void {
    this.socket.destroy();
  }
}

/**
 * A raw HTTP/2 SERVER peer: accepts one connection, checks the preface, sends
 * its own SETTINGS, and (by default) ACKs the client's SETTINGS and PING
 * frames. Used to put a real wire under Node's/Perry's http2 CLIENT.
 */
export class RawServerPeer {
  server: any;
  socket: any = null;
  log: string[] = [];
  frames: H2Frame[] = [];
  autoAck: boolean;
  private settings: Buffer;
  private connected: Promise<void>;
  private resolveConnected: () => void = () => {};

  constructor(settings?: Buffer, autoAck?: boolean) {
    this.settings = settings === undefined ? Buffer.alloc(0) : settings;
    this.autoAck = autoAck === undefined ? true : autoAck;
    const self = this;
    this.connected = new Promise<void>((resolve) => {
      self.resolveConnected = resolve;
    });
    this.server = net.createServer((sock: any) => {
      if (self.socket !== null) {
        sock.destroy();
        return;
      }
      self.socket = sock;
      const state = { buf: Buffer.alloc(0) };
      let sawPreface = false;
      sock.on("error", () => {});
      sock.on("data", (chunk: Buffer) => {
        let rest = chunk;
        if (!sawPreface) {
          state.buf = Buffer.concat([state.buf, rest]);
          if (state.buf.length < PREFACE.length) return;
          self.log.push(
            "PREFACE=" +
              (state.buf.subarray(0, PREFACE.length).toString("latin1") ===
              PREFACE_TEXT
                ? "ok"
                : "BAD"),
          );
          rest = Buffer.from(state.buf.subarray(PREFACE.length));
          state.buf = Buffer.alloc(0);
          sawPreface = true;
        }
        feedFrames(state, rest, (f: H2Frame) => {
          self.frames.push(f);
          self.log.push(describe(f));
          if (self.autoAck) {
            if (f.type === FRAME_SETTINGS && !(f.flags & FLAG_ACK)) {
              sock.write(frame(FRAME_SETTINGS, FLAG_ACK, 0));
            }
            if (f.type === FRAME_PING && !(f.flags & FLAG_ACK)) {
              sock.write(frame(FRAME_PING, FLAG_ACK, 0, f.payload));
            }
          }
        });
      });
      sock.write(frame(FRAME_SETTINGS, 0, 0, self.settings));
      self.resolveConnected();
    });
  }

  listen(): Promise<number> {
    const self = this;
    return new Promise<number>((resolve) => {
      self.server.listen(0, "127.0.0.1", () => {
        resolve(self.server.address().port);
      });
    });
  }

  waitConnected(): Promise<void> {
    return this.connected;
  }

  send(buf: Buffer): void {
    this.socket.write(buf);
  }

  take(): string[] {
    const out = this.log.slice();
    this.log = [];
    return out;
  }

  close(): void {
    if (this.socket !== null) this.socket.destroy();
    this.server.close();
  }
}

/** Print a captured frame log under a heading, or "(none)". */
export function dump(label: string, lines: string[]): void {
  console.log(label + ":");
  if (lines.length === 0) {
    console.log("  (none)");
    return;
  }
  for (let i = 0; i < lines.length; i++) console.log("  " + lines[i]);
}
