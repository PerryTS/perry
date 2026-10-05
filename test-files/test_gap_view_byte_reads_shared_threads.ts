import { Worker } from 'node:worker_threads';
const sab = new SharedArrayBuffer(32);
const control = new Int32Array(sab, 0, 2);
const bytes = new Uint8Array(sab, 10, 8);
const sub = bytes.subarray(2, 6);
function scan(view: Uint8Array): number {
  let sum = 0;
  for (let i = 0; i < view.length; i++) sum += view[i]!;
  return sum;
}
const worker = new Worker(new URL('./_helpers/view_byte_reads_worker.ts', import.meta.url), {workerData: sab});
let status = Atomics.load(control, 0);
while (status === 0) {
  Atomics.wait(control, 0, 0, 10000);
  status = Atomics.load(control, 0);
}
console.log('worker writes', bytes.length, sub.length, scan(bytes), scan(sub));
// Publish an ordinary indexed write through a view to the other thread.
sub[1] = 71;
Atomics.store(control, 1, 1);
Atomics.notify(control, 1);
await new Promise<void>((resolve, reject) => {
  worker.on('message', (sum) => { console.log('worker reads', sum); resolve(); });
  worker.on('error', reject);
});
