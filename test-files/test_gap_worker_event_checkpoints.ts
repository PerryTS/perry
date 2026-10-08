// Queued worker events are distinct host callbacks. Jobs run between events,
// while all listeners of a single event finish before its checkpoint.
import { Worker } from 'node:worker_threads';
const events: string[] = [];
const gate = new Int32Array(new SharedArrayBuffer(4));
const worker = new Worker(new URL('./_helpers/worker_event_checkpoint_sender.ts', import.meta.url), {
  workerData: gate.buffer,
});
const guard = setTimeout(() => { console.log('timeout'); process.exit(2); }, 10000);
worker.on('message', (value: number | string) => {
  if (value === 'ready') {
    worker.postMessage('go');
    // The sender publishes all three messages before notifying this waiter.
    // This removes host-speed assumptions from the batching witness.
    if (Atomics.wait(gate, 0, 0, 10000) === 'timed-out') process.exit(2);
    return;
  }
  events.push('first' + value);
  process.nextTick(() => events.push('tick' + value));
  Promise.resolve().then(() => events.push('job' + value));
  if (value === 3) setTimeout(() => {
    console.log(events.join(','));
    clearTimeout(guard);
    worker.terminate();
  }, 0);
});
worker.on('message', (value: number | string) => {
  if (typeof value === 'number') events.push('second' + value);
});
