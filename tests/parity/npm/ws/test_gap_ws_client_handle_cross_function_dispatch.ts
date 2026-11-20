// Client peers passed through helpers, stored fields, returns and bound values.
// Live frames/events prove dispatch; a missing method/event hits the watchdog.
import net from 'node:net';
import crypto from 'node:crypto';
import { WebSocket } from 'ws';
function identity(peer: any) { return peer; }
function listen(peer: any, event: string, callback: any) { peer.on(event, callback); }
function transmit(peer: any, text: string) { peer.send(text); }
function closePeer(peer: any) { peer.close(1000, 'helper'); }
const timer = setTimeout(() => { console.log('missing helper dispatch'); process.exit(3); }, 8000);
const server = net.createServer((socket: any) => {
  let head = '';
  let upgraded = false;
  socket.on('data', (chunk: Buffer) => {
    if (!upgraded) {
      head += chunk.toString();
      if (!head.includes('\r\n\r\n')) return;
      const key = /sec-websocket-key: *([^\r\n]+)/i.exec(head)![1];
      const accept = crypto.createHash('sha1').update(key + '258EAFA5-E914-47DA-95CA-C5AB0DC85B11').digest('base64');
      socket.write('HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: ' + accept + '\r\n\r\n');
      upgraded = true;
      return;
    }
    const opcode = chunk[0] & 15;
    if (opcode === 1) {
      const length = chunk[1] & 127;
      let text = '';
      for (let i = 0; i < length; i++) text += String.fromCharCode(chunk[6 + i] ^ chunk[2 + i % 4]);
      console.log('wire', text);
      socket.write(Buffer.concat([Buffer.from([129, 5]), Buffer.from('reply')]));
    } else if (opcode === 8) {
      socket.end(Buffer.from([136, 2, 3, 232]));
    }
  });
});
server.listen(0, '127.0.0.1', () => {
  const box: any = { peer: identity(new WebSocket('ws://127.0.0.1:' + server.address().port)) };
  listen(identity(box.peer), 'open', () => transmit(identity(box.peer), 'helper'));
  const returned = identity(box.peer);
  listen(returned, 'message', (data: Buffer) => {
    console.log('message', data.toString());
    closePeer(identity(returned));
  });
  listen(box.peer, 'close', (code: number) => {
    console.log('close', code);
    clearTimeout(timer);
    server.close(() => console.log('closed'));
  });
});
