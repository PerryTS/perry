// turnloop — WebSocket FRAMING edge cases, driven entirely by a raw
// net.Socket against a `ws` WebSocketServer({ port: 0 }).
//
// Everything here is about the framing layer rather than the API surface, so
// the client is hand-rolled: a library client would never emit most of these
// shapes. Covered — a text message split across THREE frames (fin=0 op=1,
// fin=0 op=0, fin=1 op=0) that must surface as exactly ONE `message` with the
// joined payload; a ping interleaved BETWEEN two fragments (RFC 6455 allows a
// control frame inside a fragmented message) and the pong that must come back
// while the fragments are still open; a binary message in two fragments; an
// empty text message; and a close frame carrying NO status code, which `ws`
// reports as 1005.
//
// A running message COUNT is printed after each case: "one message, not
// three" is the actual assertion for the fragmented cases, and only a count
// can state it.
//
// Client frames are MASKED — an unmasked client frame is a protocol error.
// Nothing host-specific is printed, so the output is byte-comparable against
// `node --experimental-strip-types`.
import net from 'node:net';
import { Buffer } from 'node:buffer';
import { WebSocketServer } from 'ws';

const CLIENT_KEY = 'dGhlIHNhbXBsZSBub25jZQ==';

function headEnd(b: Buffer): number {
  for (let i = 0; i + 3 < b.length; i++) {
    if (b[i] === 13 && b[i + 1] === 10 && b[i + 2] === 13 && b[i + 3] === 10) return i;
  }
  return -1;
}

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

// See the note in test_gap_turnloop_ws_client.ts: the bound turns a missing
// event into a printable diff instead of a hang, and never fires under Node.
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
  const serverEvents = makeQueue();
  const clientFrames = makeQueue();
  let messages = 0;

  const wss: any = new WebSocketServer({ port: 0, host: '127.0.0.1' });
  wss.on('connection', (sock: any) => {
    serverEvents.push({ tag: 'connection' });
    sock.on('message', (data: any, isBinary: any) => {
      messages = messages + 1;
      serverEvents.push({ tag: 'message', data, isBinary });
    });
    sock.on('ping', (data: any) => serverEvents.push({ tag: 'ping', data }));
    sock.on('close', (code: any, reason: any) =>
      serverEvents.push({ tag: 'close', code, reason }));
    sock.on('error', (err: any) =>
      serverEvents.push({ tag: 'error', message: String(err && err.message) }));
  });

  await new Promise<void>((r) => wss.on('listening', () => r()));
  const ad: any = wss.address();
  const port = typeof ad === 'object' && ad !== null ? ad.port : 0;
  console.log('listening:', port > 0);

  const sock: any = net.connect(port, '127.0.0.1');
  let buf = Buffer.alloc(0);
  let gotHead = false;
  const headQueue = makeQueue();
  sock.on('error', () => {});
  sock.on('data', (d: any) => {
    buf = Buffer.concat([buf, asBuffer(d)]);
    if (!gotHead) {
      const idx = headEnd(buf);
      if (idx < 0) return;
      buf = Buffer.from(buf.subarray(idx + 4));
      gotHead = true;
      headQueue.push('head');
    }
    const r = decodeFrames(buf);
    buf = r.rest;
    for (const f of r.frames) clientFrames.push(f);
  });
  await new Promise<void>((r) => sock.on('connect', () => r()));
  sock.write(
    'GET / HTTP/1.1\r\n' +
      'Host: 127.0.0.1\r\n' +
      'Upgrade: websocket\r\n' +
      'Connection: Upgrade\r\n' +
      'Sec-WebSocket-Key: ' + CLIENT_KEY + '\r\n' +
      'Sec-WebSocket-Version: 13\r\n\r\n');
  const h = await headQueue.take();
  console.log('upgraded:', h === 'head');
  const conn = await serverEvents.take();
  console.log('connection event:', conn === null ? '(timeout)' : conn.tag);

  const show = (e: any, hex: boolean) => {
    if (e === null) return '(timeout)';
    if (e.tag !== 'message') return 'unexpected ' + e.tag;
    const b = asBuffer(e.data);
    const body = hex ? 'hex=' + b.toString('hex') : 'text=' + text(b);
    return 'isBinary=' + e.isBinary + ' len=' + b.length + ' ' + body;
  };

  // 1. Three fragments, one message.
  console.log('--- three text fragments ---');
  sock.write(encodeFrame(1, Buffer.from('one-', 'utf8'), false, true));
  sock.write(encodeFrame(0, Buffer.from('two-', 'utf8'), false, true));
  sock.write(encodeFrame(0, Buffer.from('three', 'utf8'), true, true));
  console.log('server message:', show(await serverEvents.take(), false));
  console.log('messages so far:', messages);

  // 2. A ping BETWEEN two fragments. The pong must come back while the
  //    fragmented message is still open, and the message must still join.
  console.log('--- ping between fragments ---');
  sock.write(encodeFrame(1, Buffer.from('AA', 'utf8'), false, true));
  sock.write(encodeFrame(9, Buffer.from('mid', 'utf8'), true, true));
  sock.write(encodeFrame(0, Buffer.from('BB', 'utf8'), true, true));
  const pev = await serverEvents.take();
  if (pev === null || pev.tag !== 'ping') {
    console.log('server ping event: unexpected', pev === null ? '(timeout)' : pev.tag);
  } else {
    const b = asBuffer(pev.data);
    console.log('server ping event: len=' + b.length + ' text=' + text(b));
  }
  const pong = await clientFrames.take();
  if (pong === null) {
    console.log('pong frame: (timeout)');
  } else {
    console.log('pong frame: opcode=' + pong.opcode + ' fin=' + pong.fin +
      ' len=' + pong.payload.length + ' text=' + text(pong.payload));
  }
  console.log('server message:', show(await serverEvents.take(), false));
  console.log('messages so far:', messages);

  // 3. A binary message in two fragments; the payload is not valid UTF-8, so
  //    hex makes a lossy join a diff rather than a pass.
  console.log('--- two binary fragments ---');
  sock.write(encodeFrame(2, Buffer.from([0x00, 0x9f]), false, true));
  sock.write(encodeFrame(0, Buffer.from([0x92, 0x96, 0xff]), true, true));
  console.log('server message:', show(await serverEvents.take(), true));
  console.log('messages so far:', messages);

  // 4. An empty text message: a zero-length payload is still a message.
  console.log('--- empty text ---');
  sock.write(encodeFrame(1, Buffer.alloc(0), true, true));
  console.log('server message:', show(await serverEvents.take(), false));
  console.log('messages so far:', messages);

  // 5. A close frame with NO status code. `ws` reports 1005 (and echoes an
  //    empty close frame, not a synthesized 1005 on the wire).
  console.log('--- close with no code ---');
  sock.write(encodeFrame(8, Buffer.alloc(0), true, true));
  const cf = await clientFrames.take();
  if (cf === null) {
    console.log('close frame from server: (timeout)');
  } else {
    console.log('close frame from server: opcode=' + cf.opcode + ' fin=' + cf.fin +
      ' len=' + cf.payload.length + ' hex=' + cf.payload.toString('hex'));
  }
  const sclose = await serverEvents.take();
  if (sclose === null || sclose.tag !== 'close') {
    console.log('server close event: unexpected', sclose === null ? '(timeout)' : sclose.tag);
  } else {
    console.log('server close event: code=' + sclose.code + ' reason=' + text(asBuffer(sclose.reason)));
  }

  console.log('messages total:', messages);
  try { sock.destroy(); } catch {}
  await new Promise<void>((r) => wss.close(() => r()));
  console.log('closed');
}

main();
