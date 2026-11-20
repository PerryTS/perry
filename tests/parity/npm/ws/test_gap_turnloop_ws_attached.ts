// turnloop — the ATTACHED-server shape: one `http.createServer()` carrying
// both ordinary HTTP and, through `new WebSocketServer({ server })`, the
// WebSocket upgrade.
//
// This is the headline case, so it is kept readable: a plain GET is served
// and asserted BEFORE any upgrade (the same listener must still answer normal
// requests), then a `ws` client connects to the same port, a text message
// travels each way, and the connection is closed cleanly. `wss.clients.size`
// is asserted before and after.
//
// The HTTP response is scrubbed for byte comparison the same way
// test_gap_turnloop_http_server.ts scrubs it: `Date` is wall-clock so it is
// dropped, and each contiguous run of header lines is sorted because header
// ORDER is a pre-existing Perry-vs-Node difference and is not what this file
// is about. The status line, the header set, the framing and the body are.
//
// Nothing host-specific is printed, so the output is byte-comparable against
// `node --experimental-strip-types`.
import http from 'node:http';
import net from 'node:net';
import { Buffer } from 'node:buffer';
import { WebSocket, WebSocketServer } from 'ws';

function scrub(raw: string): string {
  const out: string[] = [];
  let headers: string[] = [];
  const flush = () => {
    if (headers.length > 0) {
      headers.sort();
      out.push(...headers);
      headers = [];
    }
  };
  for (const line of raw.split('\r\n')) {
    if (/^[A-Za-z][A-Za-z0-9-]*:\s/.test(line)) {
      if (!/^date:/i.test(line)) headers.push(line);
      continue;
    }
    flush();
    out.push(line);
  }
  flush();
  return out.join('\n');
}

// Resolve on a quiet period rather than on a parsed message boundary: the
// connection is keep-alive, so there is no EOF to wait for. What is printed
// is the accumulated bytes, so the wait affects only when the probe stops.
function exchange(port: number, request: string): Promise<string> {
  return new Promise((resolve) => {
    const sock: any = net.connect(port, '127.0.0.1');
    let buf = '';
    let quiet: any = null;
    const finish = () => {
      if (quiet !== null) clearTimeout(quiet);
      try { sock.destroy(); } catch {}
      resolve(buf);
    };
    const bump = () => {
      if (quiet !== null) clearTimeout(quiet);
      quiet = setTimeout(finish, 250);
    };
    sock.on('error', () => finish());
    sock.on('connect', () => { sock.write(request); bump(); });
    sock.on('data', (d: any) => { buf += d.toString('binary'); bump(); });
    sock.on('end', () => finish());
  });
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
  const clientEvents = makeQueue();

  const server = http.createServer((req: any, res: any) => {
    res.setHeader('Content-Type', 'text/plain');
    res.end('http:' + (req.url === undefined ? '/' : req.url));
  });

  const wss: any = new WebSocketServer({ server });
  wss.on('connection', (sock: any) => {
    serverEvents.push({ tag: 'connection' });
    sock.on('message', (data: any, isBinary: any) => {
      serverEvents.push({ tag: 'message', data, isBinary });
      sock.send('hello from server');
    });
    sock.on('close', (code: any, reason: any) =>
      serverEvents.push({ tag: 'close', code, reason }));
    // Surfaced rather than swallowed: under Node this never fires, so the
    // oracle output is unchanged, but a runtime that fails a send should say
    // so instead of going quiet.
    sock.on('error', (err: any) =>
      serverEvents.push({ tag: 'error', message: String(err && err.message) }));
  });

  await new Promise<void>((r) => server.listen(0, '127.0.0.1', () => r()));
  const ad: any = server.address();
  const port = typeof ad === 'object' && ad !== null ? ad.port : 0;
  console.log('listening:', port > 0);

  // 1. An ordinary GET on the very same server, BEFORE any upgrade.
  const got = await exchange(port, 'GET /page HTTP/1.1\r\nHost: x\r\n\r\n');
  console.log('--- http get ---');
  console.log(scrub(got));
  console.log('clients before upgrade:', wss.clients.size);

  // 2. A WebSocket connection to the same port.
  const ws: any = new WebSocket('ws://127.0.0.1:' + port + '/socket');
  ws.on('open', () => clientEvents.push({ tag: 'open' }));
  ws.on('message', (data: any, isBinary: any) =>
    clientEvents.push({ tag: 'message', data, isBinary }));
  ws.on('close', (code: any, reason: any) =>
    clientEvents.push({ tag: 'close', code, reason }));
  ws.on('error', (err: any) =>
    clientEvents.push({ tag: 'error', message: String(err && err.message) }));

  const opened = await clientEvents.take();
  console.log('client event:', opened === null ? '(timeout)' : opened.tag);
  const conn = await serverEvents.take();
  console.log('server event:', conn === null ? '(timeout)' : conn.tag);
  console.log('clients after upgrade:', wss.clients.size);

  // 3. A text message each way.
  ws.send('hello from client');
  const sm = await serverEvents.take();
  if (sm === null || sm.tag !== 'message') {
    console.log('server message: unexpected', sm === null ? '(timeout)' : sm.tag);
  } else {
    const b = asBuffer(sm.data);
    console.log('server message: isBinary=' + sm.isBinary + ' len=' + b.length + ' text=' + text(b));
  }
  const cm = await clientEvents.take();
  if (cm === null || cm.tag !== 'message') {
    console.log('client message: unexpected', cm === null ? '(timeout)' : cm.tag);
  } else {
    const b = asBuffer(cm.data);
    console.log('client message: isBinary=' + cm.isBinary + ' len=' + b.length + ' text=' + text(b));
  }

  // 4. A clean close, initiated by the client.
  ws.close(1000, 'done');
  const sclose = await serverEvents.take();
  if (sclose === null || sclose.tag !== 'close') {
    console.log('server close: unexpected', sclose === null ? '(timeout)' : sclose.tag);
  } else {
    console.log('server close: code=' + sclose.code + ' reason=' + text(asBuffer(sclose.reason)));
  }
  const cclose = await clientEvents.take();
  if (cclose === null || cclose.tag !== 'close') {
    console.log('client close: unexpected', cclose === null ? '(timeout)' : cclose.tag);
  } else {
    console.log('client close: code=' + cclose.code + ' reason=' + text(asBuffer(cclose.reason)));
  }
  console.log('clients after close:', wss.clients.size);

  await new Promise<void>((r) => wss.close(() => r()));
  await new Promise<void>((r) => server.close(() => r()));
  console.log('closed');
}

main();
