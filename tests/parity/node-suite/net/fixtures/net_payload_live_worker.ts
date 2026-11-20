import net from 'node:net';
import { parentPort } from 'node:worker_threads';
// Both listener and client stay live when the primary is notified. The
// top-level await keeps the Worker body alive across those turns.
let finish: () => void = () => {};
const finished = new Promise<void>((resolve) => { finish = resolve; });
const server = net.createServer((peer) => peer.on('data', (data) => peer.write(data)));
server.listen(0, '127.0.0.1', () => {
  const client = net.connect(server.address().port, '127.0.0.1');
  client.on('connect', () => client.write('worker-live'));
  client.on('data', (data: Buffer) => {
    parentPort?.postMessage(data.toString());
    // Keep sockets alive until the primary explicitly asks for another turn.
    if (data.toString() === 'worker-live') {
      parentPort?.once('message', () => client.write('worker-again'));
    }
    if (data.toString() === 'worker-again') {
      client.end();
      server.close(() => { parentPort?.postMessage('worker-closed'); finish(); });
    }
  });
});
await finished;
