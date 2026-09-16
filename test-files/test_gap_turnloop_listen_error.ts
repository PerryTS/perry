// The shape of a failed `listen()`, and when it is delivered.
//
// The port is held by a `net` server, so the HTTP bind fails against a
// different owner — `net` and `http` are different turnloop subsystems — rather
// than racing another HTTP listener.
//
// Node emits `'error'` ASYNCHRONOUSLY on the server: measured on 26.5.1 the
// order is `after-listen-call, error`, `'listening'` never fires, the
// `listen(cb)` callback never runs, and `server.listening` stays false.
//
// Two values are deliberately not printed raw. The port is ephemeral (this runs
// sharded and once per A/B arm, so a literal would collide with itself), so it
// is redacted out of the message and asserted by equality instead. And
// `err.errno` is printed as its SIGN: it is the negated OS code — -98 for
// EADDRINUSE on Linux, -48 on macOS — so the number is platform data while
// being present and negative is the contract.
import http from 'node:http';
import net from 'node:net';

const order: string[] = [];
const holder = net.createServer();

holder.listen(0, '127.0.0.1', () => {
  const port = (holder.address() as any).port;
  const server = http.createServer((_req, res) => res.end('never'));

  server.on('listening', () => order.push('listening'));
  server.on('error', (err: any) => {
    order.push('error');
    const redacted = String(err.message).split(String(port)).join('<port>');
    console.log('message :', redacted);
    console.log('code    :', err.code);
    console.log('errno<0 :', typeof err.errno === 'number' && err.errno < 0);
    console.log('syscall :', err.syscall);
    console.log('address :', err.address);
    console.log('port-matches:', err.port === port);
    console.log('name    :', err.name);
    console.log('listening after error:', server.listening);
    console.log('order   :', order.join(','));
    holder.close();
    console.log('done');
  });

  const returned = server.listen(port, '127.0.0.1', () => order.push('listen-cb'));
  console.log('listen() returned the server:', returned === server);
  console.log('listening flag right after listen():', server.listening);
  order.push('after-listen-call');
});
