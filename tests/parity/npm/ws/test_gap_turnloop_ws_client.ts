// turnloop — the `ws` CLIENT, driven by a hand-rolled WebSocket server.
//
// The server side here is a raw `net.Server` that writes the 101 and every
// frame by hand, so the WIRE is entirely under this file's control. That is
// the point: an off-the-shelf server would hide which bytes the client was
// actually given. Covered, in order — the `open` event; a text message; a
// BINARY message whose payload is deliberately not valid UTF-8 (printed as
// hex, so a lossy string round-trip shows up as a diff rather than as a pass);
// the `isBinary` second argument of `message`; a server ping and the client's
// AUTOMATIC pong (asserted where it matters — arriving back at the server,
// with its payload intact); an unsolicited server pong surfacing as the
// client's `pong` event; and a server-initiated close carrying a 4xxx code
// plus a reason, asserted on both the client's `close` arguments and the
// close frame the client echoes.
//
// Nothing host-specific is printed — no port, no address, no timing, no stack
// — so the output is byte-comparable against `node --experimental-strip-types`.
import crypto from 'node:crypto';
import net from 'node:net';
import { Buffer } from 'node:buffer';
import { WebSocket } from 'ws';

const GUID = '258EAFA5-E914-47DA-95CA-C5AB0DC85B11';

function acceptFor(key: string): string {
  return crypto.createHash('sha1').update(key + GUID).digest('base64');
}

// Byte-scan for the end of the header block. Deliberately NOT a string
// search: the same buffer may already hold binary frame bytes behind the
// header block, and decoding those to find a marker is exactly the kind of
// lossy step this file exists to catch.
function headEnd(b: Buffer): number {
  for (let i = 0; i + 3 < b.length; i++) {
    if (b[i] === 13 && b[i + 1] === 10 && b[i + 2] === 13 && b[i + 3] === 10) return i;
  }
  return -1;
}

// One WebSocket frame. Server -> client frames are never masked.
function encodeFrame(opcode: number, payload: Buffer, fin: boolean, mask: boolean): Buffer {
  const len = payload.length;
  const head: number[] = [(fin ? 0x80 : 0x00) | (opcode & 0x0f)];
  const mbit = mask ? 0x80 : 0x00;
  if (len < 126) {
    head.push(mbit | len);
  } else {
    head.push(mbit | 126, (len >> 8) & 0xff, len & 0xff);
  }
  let body = payload;
  if (mask) {
    const k = [0x37, 0xfa, 0x21, 0x3d];
    head.push(k[0], k[1], k[2], k[3]);
    body = Buffer.alloc(len);
    for (let i = 0; i < len; i++) body[i] = payload[i] ^ k[i % 4];
  }
  return Buffer.concat([Buffer.from(head), body]);
}

// Pull every complete frame out of `buf`, returning the undecoded tail.
function decodeFrames(buf: Buffer): { frames: any[]; rest: Buffer } {
  const frames: any[] = [];
  let off = 0;
  while (off + 2 <= buf.length) {
    const b0 = buf[off];
    const b1 = buf[off + 1];
    const fin = (b0 & 0x80) !== 0;
    const opcode = b0 & 0x0f;
    const masked = (b1 & 0x80) !== 0;
    let len = b1 & 0x7f;
    let p = off + 2;
    if (len === 126) {
      if (p + 2 > buf.length) break;
      len = buf.readUInt16BE(p);
      p += 2;
    } else if (len === 127) {
      if (p + 8 > buf.length) break;
      len = buf.readUInt32BE(p) * 4294967296 + buf.readUInt32BE(p + 4);
      p += 8;
    }
    let key: Buffer | null = null;
    if (masked) {
      if (p + 4 > buf.length) break;
      key = Buffer.from(buf.subarray(p, p + 4));
      p += 4;
    }
    if (p + len > buf.length) break;
    const payload = Buffer.from(buf.subarray(p, p + len));
    if (key !== null) {
      for (let i = 0; i < len; i++) payload[i] = payload[i] ^ key[i % 4];
    }
    frames.push({ fin, opcode, payload });
    off = p + len;
  }
  return { frames, rest: Buffer.from(buf.subarray(off)) };
}

// A queue whose `take()` resolves with the next item, or with null after a
// generous bound. The bound never fires under Node — localhost frames arrive
// in microseconds — so it changes only WHEN the probe stops, never WHAT it
// prints. Its job is to turn a missing event into a printable diff instead of
// a hang.
function makeQueue() {
  const items: any[] = [];
  const waiters: any[] = [];
  return {
    push(item: any) {
      if (waiters.length > 0) {
        const w = waiters.shift();
        w(item);
      } else {
        items.push(item);
      }
    },
    take(): Promise<any> {
      return new Promise((resolve) => {
        if (items.length > 0) {
          resolve(items.shift());
          return;
        }
        let done = false;
        let timer: any = null;
        const fire = (v: any) => {
          if (done) return;
          done = true;
          if (timer !== null) clearTimeout(timer);
          resolve(v);
        };
        waiters.push(fire);
        timer = setTimeout(() => {
          const i = waiters.indexOf(fire);
          if (i >= 0) waiters.splice(i, 1);
          fire(null);
        }, 4000);
      });
    },
  };
}

// `undefined` is not something Node ever passes here -- every `data` and every
// close `reason` is a Buffer. Tolerating it anyway is deliberate: a runtime
// that omits the argument should show up as a printable DIFF on this row and
// on every row after it, not as an uncaught TypeError that hides the rest of
// the file.
function asBuffer(d: any): Buffer {
  if (d === undefined || d === null) return Buffer.alloc(0);
  if (Buffer.isBuffer(d)) return d;
  if (typeof d === 'string') return Buffer.from(d, 'utf8');
  if (Array.isArray(d)) {
    const parts: Buffer[] = [];
    for (const x of d) parts.push(asBuffer(x));
    return Buffer.concat(parts);
  }
  return Buffer.from(d);
}

function text(b: Buffer): string {
  return JSON.stringify(b.toString('utf8'));
}

async function main() {
  const clientEvents = makeQueue();
  const serverFrames = makeQueue();
  let clientSock: any = null;

  const server = net.createServer((sock: any) => {
    clientSock = sock;
    let buf = Buffer.alloc(0);
    let upgraded = false;
    sock.on('error', () => {});
    sock.on('data', (d: any) => {
      buf = Buffer.concat([buf, asBuffer(d)]);
      if (!upgraded) {
        const idx = headEnd(buf);
        if (idx < 0) return;
        const head = Buffer.from(buf.subarray(0, idx)).toString('utf8');
        buf = Buffer.from(buf.subarray(idx + 4));
        let key = '';
        for (const line of head.split('\r\n')) {
          const c = line.indexOf(':');
          if (c > 0 && line.slice(0, c).toLowerCase() === 'sec-websocket-key') {
            key = line.slice(c + 1).trim();
          }
        }
        upgraded = true;
        sock.write(
          'HTTP/1.1 101 Switching Protocols\r\n' +
            'Upgrade: websocket\r\n' +
            'Connection: Upgrade\r\n' +
            'Sec-WebSocket-Accept: ' + acceptFor(key) + '\r\n\r\n');
      }
      const r = decodeFrames(buf);
      buf = r.rest;
      for (const f of r.frames) serverFrames.push(f);
    });
  });

  await new Promise<void>((r) => server.listen(0, '127.0.0.1', () => r()));
  const ad: any = server.address();
  const port = typeof ad === 'object' && ad !== null ? ad.port : 0;
  console.log('listening:', port > 0);

  const ws: any = new WebSocket('ws://127.0.0.1:' + port + '/');
  ws.on('open', () => clientEvents.push({ tag: 'open' }));
  ws.on('message', (data: any, isBinary: any) =>
    clientEvents.push({ tag: 'message', data, isBinary }));
  ws.on('ping', (data: any) => clientEvents.push({ tag: 'ping', data }));
  ws.on('pong', (data: any) => clientEvents.push({ tag: 'pong', data }));
  ws.on('close', (code: any, reason: any) =>
    clientEvents.push({ tag: 'close', code, reason }));
  ws.on('error', (err: any) =>
    clientEvents.push({ tag: 'error', message: String(err && err.message) }));

  // 1. open
  const opened = await clientEvents.take();
  console.log('event 1:', opened === null ? '(timeout)' : opened.tag);

  // 2. a text message.
  clientSock.write(encodeFrame(1, Buffer.from('hello-text', 'utf8'), true, false));
  const m1 = await clientEvents.take();
  if (m1 === null || m1.tag !== 'message') {
    console.log('text message: unexpected', m1 === null ? '(timeout)' : m1.tag);
  } else {
    const b = asBuffer(m1.data);
    console.log('text message: isBinary=' + m1.isBinary + ' len=' + b.length + ' text=' + text(b));
  }

  // 3. a binary message whose payload is NOT valid UTF-8. Printed as hex:
  //    a lossy conversion replaces 0x9f/0xff with U+FFFD and the hex shows it.
  const raw = Buffer.from([0x00, 0x9f, 0x92, 0x96, 0xff]);
  clientSock.write(encodeFrame(2, raw, true, false));
  const m2 = await clientEvents.take();
  if (m2 === null || m2.tag !== 'message') {
    console.log('binary message: unexpected', m2 === null ? '(timeout)' : m2.tag);
  } else {
    const b = asBuffer(m2.data);
    console.log('binary message: isBinary=' + m2.isBinary + ' len=' + b.length + ' hex=' + b.toString('hex'));
  }

  // 4. a server ping. Two consequences, awaited in a fixed order so their
  //    arrival order cannot reorder the output: the client's `ping` event and
  //    the automatic pong arriving back here with the payload unchanged.
  clientSock.write(encodeFrame(9, Buffer.from('pingpay', 'utf8'), true, false));
  const pingEvt = await clientEvents.take();
  if (pingEvt === null || pingEvt.tag !== 'ping') {
    console.log('ping event: unexpected', pingEvt === null ? '(timeout)' : pingEvt.tag);
  } else {
    const b = asBuffer(pingEvt.data);
    console.log('ping event: len=' + b.length + ' text=' + text(b));
  }
  const pongFrame = await serverFrames.take();
  if (pongFrame === null) {
    console.log('auto pong: (timeout)');
  } else {
    console.log('auto pong: opcode=' + pongFrame.opcode + ' fin=' + pongFrame.fin +
      ' len=' + pongFrame.payload.length + ' text=' + text(pongFrame.payload));
  }

  // 5. an unsolicited pong from the server -> the client's `pong` event.
  clientSock.write(encodeFrame(10, Buffer.from('srvpong', 'utf8'), true, false));
  const pongEvt = await clientEvents.take();
  if (pongEvt === null || pongEvt.tag !== 'pong') {
    console.log('pong event: unexpected', pongEvt === null ? '(timeout)' : pongEvt.tag);
  } else {
    const b = asBuffer(pongEvt.data);
    console.log('pong event: len=' + b.length + ' text=' + text(b));
  }

  // 6. server-initiated close, code 4001 plus a reason. Both the echoed close
  //    frame and the client's `close` arguments are asserted.
  const reason = Buffer.from('server-initiated', 'utf8');
  const closePayload = Buffer.alloc(2 + reason.length);
  closePayload.writeUInt16BE(4001, 0);
  reason.copy(closePayload, 2);
  clientSock.write(encodeFrame(8, closePayload, true, false));

  const echo = await serverFrames.take();
  if (echo === null) {
    console.log('close echo: (timeout)');
  } else if (echo.opcode !== 8) {
    console.log('close echo: unexpected opcode=' + echo.opcode);
  } else {
    const p = echo.payload;
    const code = p.length >= 2 ? p.readUInt16BE(0) : 0;
    const why = p.length > 2 ? Buffer.from(p.subarray(2)) : Buffer.alloc(0);
    console.log('close echo: opcode=8 len=' + p.length + ' code=' + code + ' reason=' + text(why));
  }

  const closed = await clientEvents.take();
  if (closed === null || closed.tag !== 'close') {
    console.log('close event: unexpected', closed === null ? '(timeout)' : closed.tag);
  } else {
    console.log('close event: code=' + closed.code + ' reason=' + text(asBuffer(closed.reason)));
  }

  console.log('readyState CLOSED:', ws.readyState === WebSocket.CLOSED);

  try { if (clientSock !== null) clientSock.destroy(); } catch {}
  await new Promise<void>((r) => server.close(() => r()));
  console.log('closed');
}

main();
