// turnloop P9 — network I/O inside a `node:worker_threads` Worker.
//
// Before P9 a Worker was not allowed a `turnloop::Loop`: the submit guard was
// `agent_loop::net_available()`, which asked "am I the PRIMARY agent?". Every
// network surface therefore fell back to tokio on that thread — `fetch` to
// reqwest, `net` to a tokio `TcpStream`, the four database drivers to their
// legacy clients — and that live fallback is why tokio could not be deleted.
//
// Nothing host-specific is printed: both servers bind port 0 and the ports
// reach the Worker through `workerData`, so the output is the same on every
// host and is comparable byte-for-byte against `node --experimental-strip-types`.
//
// Three cases, and the middle one is the point. A Worker that PARKS before it
// fetches is the shape P8 measured as a hang (exit 124 at a 25 s cap) on both
// `main` and the integration branch — a promise that never settles, which a
// green suite cannot report because the process simply stops. The 8 s watchdog
// below turns that back into a visible failure.
import http from 'node:http';
import net from 'node:net';
import { Worker } from 'node:worker_threads';

const httpServer = http.createServer((req, res) => {
  res.writeHead(200, { 'content-type': 'text/plain' });
  res.end(`hello${req.url}`);
});

const echoServer = net.createServer((sock) => {
  sock.on('data', (chunk: Buffer) => sock.write(chunk));
  sock.on('error', () => {});
});

function listen(server: { listen: (p: number, h: string, cb: () => void) => void; address: () => unknown }): Promise<number> {
  return new Promise<number>((resolve) => {
    server.listen(0, '127.0.0.1', () => {
      const addr = server.address() as { port: number };
      resolve(addr.port);
    });
  });
}

const httpPort = await listen(httpServer);
const echoPort = await listen(echoServer);

const workerUrl = new URL('./_helpers/turnloop_p9_worker_net.ts', import.meta.url);
const worker = new Worker(workerUrl, { workerData: { httpPort, echoPort } });

// A hang is the failure this test exists to catch, so it must not be allowed to
// present as "the run never finished". `unref()` keeps the watchdog from
// holding the loop open on the happy path.
const watchdog = setTimeout(() => {
  console.log('WORKER NEVER ANSWERED');
  process.exit(3);
}, 8000);
if (typeof (watchdog as { unref?: () => void }).unref === 'function') {
  (watchdog as { unref: () => void }).unref();
}

const results: string[] = await new Promise((resolve) => {
  worker.on('message', (value: string[]) => resolve(value));
  worker.on('error', (e: Error) => resolve([`worker-error ${e.message}`]));
});
clearTimeout(watchdog);

for (const line of results) console.log(line);

await worker.terminate();
httpServer.close();
echoServer.close();
console.log('done');
