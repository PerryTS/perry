import { Worker } from 'node:worker_threads';
const timer = setTimeout(() => { console.log('live Worker stalled'); process.exit(3); }, 10000);
const worker = new Worker(new URL('./_helpers/net_payload_live_worker.ts', import.meta.url));
worker.on('message', (message: string) => {
  console.log(message);
  if (message === 'worker-live') worker.postMessage('continue');
});
worker.on('exit', (code: number) => { clearTimeout(timer); console.log('exit', code); });
