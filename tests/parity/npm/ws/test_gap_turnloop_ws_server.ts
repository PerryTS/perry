// turnloop — a `ws` WebSocketServer({ port: 0 }), driven by a raw net.Socket.
//
// The client side is hand-framed on purpose: it is the only way to assert what
// the SERVER actually put on the wire. Covered — the 101 response head; the
// `connection` event; an echoed text message; an echoed BINARY message (hex-
// printed, so a lossy UTF-8 round trip diffs instead of passing); the server
// answering a client ping with a pong; `ws.close(1000, 'bye')` and the code
// plus reason as they appear in the close frame; and `wss.clients.size` before
// and after.
//
// Client frames are MASKED. An unmasked client frame is a protocol error and
// a conforming server must fail the connection, so masking is not a detail
// here — it is what keeps the test about the server's behaviour.
//
// The 101 head is normalized before printing: every header name lowercased and
// the header lines sorted. Header casing and order are a known Perry-vs-Node
// difference and are not what this file is about; the status line, the header
// SET and the accept value are. A FIXED Sec-WebSocket-Key is sent so the
// accept digest is a constant rather than host state.
//
// Nothing host-specific is printed, so the output is byte-comparable against
// `node --experimental-strip-types`.
import net from 'node:net';
import { Buffer } from 'node:buffer';
import { WebSocketServer } from 'ws';

// RFC 6455's example key; its accept digest is the constant below.
const CLIENT_KEY = 'dGhlIHNhbXBsZSBub25jZQ==';

function headEnd(b: Buffer): number {
  for (let i = 0; i + 3 < b.length; i++) {
    if (b[i] === 13 && b[i + 1] === 10 && b[i + 2] === 13 && b[i + 3] === 10) return i;
  }
  return -1;
}

function scrubHead(raw: string): string {
  const other: string[] = [];
  const headers: string[] = [];
  for (const line of raw.split('\r\n')) {
    if (line === '') continue;
    const c = line.indexOf(':');
    const name = c > 0 ? line.slice(0, c) : '';
    if (c > 0 && /^[A-Za-z][A-Za-z0-9-]*$/.test(name)) {
      headers.push(name.toLowerCase() + ': ' + line.slice(c + 1).trim());
    } else {
      other.push(line);
    }
  }
  headers.sort();
  return other.concat(headers).join('\n');
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

function describeFrame(f: any, hex: boolean): string {
  if (f === null) return '(timeout)';
  const body = hex
    ? 'hex=' + f.payload.toString('hex')
    : 'text=' + text(f.payload);
  return 'opcode=' + f.opcode + ' fin=' + f.fin + ' len=' + f.payload.length + ' ' + body;
}

async function main() {
  const serverEvents = makeQueue();
  const clientFrames = makeQueue();
  let serverWs: any = null;

  const wss: any = new WebSocketServer({ port: 0, host: '127.0.0.1' });
  wss.on('connection', (sock: any) => {
    serverWs = sock;
    serverEvents.push({ tag: 'connection' });
    sock.on('message', (data: any, isBinary: any) => {
      serverEvents.push({ tag: 'message', data, isBinary });
      sock.send(asBuffer(data), { binary: isBinary === true });
    });
    sock.on('ping', (data: any) => serverEvents.push({ tag: 'ping', data }));
    sock.on('close', (code: any, reason: any) =>
      serverEvents.push({ tag: 'close', code, reason }));
    // Surfaced rather than swallowed: under Node this never fires, so the
    // oracle output is unchanged, but a runtime that fails a send should say
    // so instead of going quiet.
    sock.on('error', (err: any) =>
      serverEvents.push({ tag: 'error', message: String(err && err.message) }));
  });

  await new Promise<void>((r) => wss.on('listening', () => r()));
  const ad: any = wss.address();
  const port = typeof ad === 'object' && ad !== null ? ad.port : 0;
  console.log('listening:', port > 0);

  // Hand-rolled client: handshake, then frames.
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
      const head = Buffer.from(buf.subarray(0, idx)).toString('utf8');
      buf = Buffer.from(buf.subarray(idx + 4));
      gotHead = true;
      headQueue.push(head);
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

  const head = await headQueue.take();
  console.log('--- 101 head ---');
  console.log(head === null ? '(timeout)' : scrubHead(head));

  const conn = await serverEvents.take();
  console.log('connection event:', conn === null ? '(timeout)' : conn.tag);
  console.log('clients before:', wss.clients.size);

  // 1. text echo.
  sock.write(encodeFrame(1, Buffer.from('ping-text', 'utf8'), true, true));
  const sm1 = await serverEvents.take();
  if (sm1 === null || sm1.tag !== 'message') {
    console.log('server text message: unexpected', sm1 === null ? '(timeout)' : sm1.tag);
  } else {
    const b = asBuffer(sm1.data);
    console.log('server text message: isBinary=' + sm1.isBinary + ' len=' + b.length + ' text=' + text(b));
  }
  console.log('text echo frame:', describeFrame(await clientFrames.take(), false));

  // 2. binary echo. The payload is not valid UTF-8; hex makes a lossy round
  //    trip a diff rather than a pass.
  const raw = Buffer.from([0x00, 0x9f, 0x92, 0x96, 0xff]);
  sock.write(encodeFrame(2, raw, true, true));
  const sm2 = await serverEvents.take();
  if (sm2 === null || sm2.tag !== 'message') {
    console.log('server binary message: unexpected', sm2 === null ? '(timeout)' : sm2.tag);
  } else {
    const b = asBuffer(sm2.data);
    console.log('server binary message: isBinary=' + sm2.isBinary + ' len=' + b.length + ' hex=' + b.toString('hex'));
  }
  console.log('binary echo frame:', describeFrame(await clientFrames.take(), true));

  // 3. a client ping -> the server's automatic pong.
  sock.write(encodeFrame(9, Buffer.from('cping', 'utf8'), true, true));
  const pev = await serverEvents.take();
  if (pev === null || pev.tag !== 'ping') {
    console.log('server ping event: unexpected', pev === null ? '(timeout)' : pev.tag);
  } else {
    const b = asBuffer(pev.data);
    console.log('server ping event: len=' + b.length + ' text=' + text(b));
  }
  console.log('server pong frame:', describeFrame(await clientFrames.take(), false));

  // 4. the server closes with a code and a reason.
  serverWs.close(1000, 'bye');
  const cf = await clientFrames.take();
  if (cf === null) {
    console.log('close frame: (timeout)');
  } else if (cf.opcode !== 8) {
    console.log('close frame: unexpected opcode=' + cf.opcode);
  } else {
    const p = cf.payload;
    const code = p.length >= 2 ? p.readUInt16BE(0) : 0;
    const why = p.length > 2 ? Buffer.from(p.subarray(2)) : Buffer.alloc(0);
    console.log('close frame: opcode=8 fin=' + cf.fin + ' len=' + p.length +
      ' code=' + code + ' reason=' + text(why));
    // Echo the close back so the server finishes its close handshake without
    // waiting on its 30s close timeout.
    sock.write(encodeFrame(8, p, true, true));
  }

  const sclose = await serverEvents.take();
  if (sclose === null || sclose.tag !== 'close') {
    console.log('server close event: unexpected', sclose === null ? '(timeout)' : sclose.tag);
  } else {
    console.log('server close event: code=' + sclose.code + ' reason=' + text(asBuffer(sclose.reason)));
  }
  console.log('clients after:', wss.clients.size);

  try { sock.destroy(); } catch {}
  await new Promise<void>((r) => wss.close(() => r()));
  console.log('closed');
}

main();
