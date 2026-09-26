// The worker half of test_gap_11340_worker_net_events.ts.
import net from 'node:net';
import { parentPort } from 'node:worker_threads';

// The parity harness runs a plain-TCP echo server here
// (test-files/test_net_echo_server.py), the same one test_net_min.ts uses.
const PORT = 17891;
const ROUNDS = 25;
let completed = 0;
const replies = new Set<string>();
for (let i = 0; i < ROUNDS; i++) {
  const reply = await new Promise<string>((resolve) => {
    // node-postgres' shape (lib/connection.js): construct, connect, THEN subscribe.
    const s = new net.Socket();
    s.setNoDelay(true);
    s.connect(PORT, '127.0.0.1');
    const want = 'round' + i;
    let got = '';
    s.once('connect', () => s.write(want));
    s.on('data', (d: Buffer) => {
      got += d.toString();
      if (got.length >= want.length) s.end();
    });
    s.on('close', () => resolve(got));
    s.on('error', (e: Error) => resolve('error ' + e.message));
  });
  if (reply === 'round' + i) completed++;
  replies.add(reply.replace(/[0-9]+$/, ''));
}
parentPort?.postMessage(`rounds ${completed}/${ROUNDS} ${[...replies].sort().join(',')}`);
