import { Worker } from 'node:worker_threads';
const timer = setTimeout(() => { console.log('live Worker stalled'); process.exit(3); }, 10000);
const worker = new Worker(new URL('./_helpers/net_payload_live_worker.ts', import.meta.url));
worker.on('message', (message: string) => {
  console.log(message);
  if (message === 'worker-live') worker.postMessage('continue');
  // The Worker closed every socket it owned; end it explicitly (Perry's
  // Worker does not yet exit on its own once a parentPort listener has run).
  if (message === 'worker-closed') { clearTimeout(timer); worker.terminate(); }
});
