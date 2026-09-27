// #11434: a `worker_threads` worker that runs its own `net.createServer` and
// connects to it must not stall. The worker's accepted socket buffers bytes
// that arrive before its `'connection'` callback has run, and releases them
// when the callback returns — a push onto the WORKER's own event queue, made
// outside its loop's turn, with a wake through `js_notify_main_thread`. That
// wake was one process-wide flag which every agent's `js_wait_for_event`
// consumed: when the main thread's wait swapped it first, the worker parked
// with its own events queued and never woke (about half the runs stalled).
import { Worker } from 'node:worker_threads';

const worker = new Worker(new URL('./_helpers/gap_11434_worker_own_server_worker.ts', import.meta.url));

// A lost wake presents as a hang; the watchdog turns it into a visible failure.
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
