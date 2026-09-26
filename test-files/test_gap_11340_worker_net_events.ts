// #11340: socket events for a socket opened on a `worker_threads` worker must
// be delivered on that worker. perry-ext-net kept ONE process-wide queue of
// pending socket events and woke the primary thread on every push, so the
// primary's pump raced the worker's for them: a worker's `'connect'` taken by
// the primary was dispatched against listeners on the worker's heap — dropped
// (the worker waited forever: compiled `pg` hung in `connect()` on a worker),
// run on the wrong thread (a `TypeError` after the worker exited, as mysql2
// showed), or a crash.
//
// The worker runs its own echo server and 25 sequential client sockets in
// node-postgres' shape (construct, `connect()`, then `once('connect')`), which
// lost roughly one round in five before the fix. Everything stays inside the
// worker, so the primary's only job is to keep pumping while it waits.
import { Worker } from 'node:worker_threads';

const worker = new Worker(new URL('./_helpers/gap_11340_worker_net_events_worker.ts', import.meta.url));

// A lost event presents as a hang; the watchdog turns it into a visible failure.
const watchdog = setTimeout(() => {
  console.log('WORKER STOPPED');
  process.exit(3);
}, 15000);
if (typeof (watchdog as { unref?: () => void }).unref === 'function') {
  (watchdog as { unref: () => void }).unref();
}

const result = await new Promise<string>((resolve) => {
  worker.on('message', (value: string) => resolve(String(value)));
  worker.on('error', (e: Error) => resolve(`worker-error ${e.message}`));
});
clearTimeout(watchdog);
console.log(result);
await worker.terminate();
console.log('done');
