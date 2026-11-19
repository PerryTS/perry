// #10391: a WebSocketServer attached to an http server, and a client that is
// not `ws` and closes first. The server must answer the client's close frame
// with its own before it ends the connection; a server that sends a bare FIN
// instead leaves the client with an abnormal closure (1006).
//
// The client is a raw net.Socket, so nothing in it can paper over a frame the
// server never wrote. It reads until the server's FIN and prints every frame
// it saw on the way.
import http from 'node:http';
import net from 'node:net';
import { Buffer } from 'node:buffer';
import { WebSocketServer } from 'ws';

const CLIENT_KEY = 'dGhlIHNhbXBsZSBub25jZQ==';

function maskedFrame(opcode: number, payload: Buffer): Buffer {
  const key = [0x37, 0xfa, 0x21, 0x3d];
  const body = Buffer.alloc(payload.length);
  for (let i = 0; i < payload.length; i++) body[i] = payload[i] ^ key[i % 4];
  return Buffer.concat([Buffer.from([0x80 | opcode, 0x80 | payload.length, ...key]), body]);
}

// Server frames are unmasked and short here.
function frames(buf: Buffer): string[] {
  const out: string[] = [];
  let off = 0;
  while (off + 2 <= buf.length) {
    const len = buf[off + 1] & 0x7f;
    const payload = buf.subarray(off + 2, off + 2 + len);
    out.push('opcode=' + (buf[off] & 0x0f) + ' payload=' + payload.toString('hex'));
    off += 2 + len;
  }
  if (off !== buf.length) out.push('trailing bytes=' + (buf.length - off));
  return out;
}

const guard = setTimeout(() => {
  console.log('timeout');
  process.exit(1);
}, 10000);

const server = http.createServer((_req, res) => res.end('plain'));
const wss = new WebSocketServer({ server });
const serverClosed = new Promise<string>((resolve) => {
  wss.on('connection', (ws: any) => {
    ws.on('close', (code: number, reason: Buffer) => resolve(code + ' ' + String(reason)));
    ws.send('hi');
  });
});

server.listen(0, '127.0.0.1', async () => {
  const port = (server.address() as any).port;
  const sock = net.connect(port, '127.0.0.1');
  let buf = Buffer.alloc(0);
  let upgraded = false;
  const ended = new Promise<void>((resolve) => sock.on('end', () => resolve()));
  sock.on('data', (d: Buffer) => {
    buf = Buffer.concat([buf, d]);
    if (upgraded) return;
    const head = buf.indexOf('\r\n\r\n');
    if (head < 0) return;
    console.log('status:', buf.subarray(0, buf.indexOf('\r\n')).toString());
    buf = buf.subarray(head + 4);
    upgraded = true;
    const reason = Buffer.from('bye');
    const close = Buffer.alloc(2 + reason.length);
    close.writeUInt16BE(4000, 0);
    reason.copy(close, 2);
    sock.write(maskedFrame(8, close));
  });
  sock.write(
    'GET / HTTP/1.1\r\n' +
      'Host: 127.0.0.1\r\n' +
      'Upgrade: websocket\r\n' +
      'Connection: Upgrade\r\n' +
      'Sec-WebSocket-Key: ' + CLIENT_KEY + '\r\n' +
      'Sec-WebSocket-Version: 13\r\n\r\n',
  );

  await ended;
  for (const f of frames(buf)) console.log('frame before FIN:', f);
  sock.end();
  console.log('server close event:', await serverClosed);
  clearTimeout(guard);
  wss.close();
  server.close(() => console.log('server closed'));
});
