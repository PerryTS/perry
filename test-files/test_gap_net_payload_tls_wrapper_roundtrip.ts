import net from 'node:net';
import tls from 'node:tls';
import fs from 'node:fs';
const fixture = '../test-parity/node-suite/tls/fixtures/';
const cert = fs.readFileSync(new URL(fixture + 'localhost-cert.pem', import.meta.url));
const key = fs.readFileSync(new URL(fixture + 'localhost-key.pem', import.meta.url));
function send(socket: any) { socket.write('wrapped'); }
const timer = setTimeout(() => { console.log('TLS wrapper stalled'); process.exit(3); }, 8000);
const server = tls.createServer({ cert, key }, (peer) => {
  peer.on('data', (data: Buffer) => peer.write(data));
});
server.listen(0, '127.0.0.1', () => {
  const parent = net.connect(server.address().port, 'localhost');
  parent.once('connect', () => {
    const socket = tls.connect({ socket: parent, servername: 'localhost', rejectUnauthorized: false });
    console.log('wrapper', socket !== parent);
    socket.once('secureConnect', () => {
      console.log('write', socket.write('wrapped'));
      send(socket);
    });
    let received = '';
    socket.on('data', (data: Buffer) => {
      received += data.toString();
      if (received.length === 14) { console.log('TLS', received); socket.end(); }
    });
    socket.once('close', () => server.close(() => { clearTimeout(timer); console.log('closed'); }));
  });
});
