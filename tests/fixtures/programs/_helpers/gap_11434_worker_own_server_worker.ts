// The worker half of test_gap_11434_worker_own_server.ts.
import net from 'node:net';
import { parentPort } from 'node:worker_threads';

const server = net.createServer((sock) => {
  sock.on('data', (d: Buffer) => sock.end(d.toString().toUpperCase()));
});
const port = await new Promise<number>((resolve) => {
  server.listen(0, '127.0.0.1', () => resolve((server.address() as { port: number }).port));
});
const ROUNDS = 25;
let completed = 0;
for (let i = 0; i < ROUNDS; i++) {
  const reply = await new Promise<string>((resolve) => {
    const s = new net.Socket();
    s.setNoDelay(true);
    s.connect(port, '127.0.0.1');
    let got = '';
    s.once('connect', () => s.write('round' + i));
    s.on('data', (d: Buffer) => { got += d.toString(); });
    s.on('close', () => resolve(got));
  });
  if (reply === 'ROUND' + i) completed++;
}
server.close();
parentPort?.postMessage(`rounds ${completed}/${ROUNDS}`);
