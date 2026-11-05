import { Worker } from 'node:worker_threads';
import { delay } from './fetch_stream_fixture.ts';
async function main() {
  const worker = new Worker(new URL('./fetch_stream_pipe_worker.ts', import.meta.url));
  await new Promise(resolve => worker.once('message', resolve));
  const first = new Promise<any>(resolve => worker.once('message', resolve));
  worker.postMessage({start:true});
  await delay(100); worker.postMessage({block:'A'});
  // The producer waits for real downstream progress before sending EOF.
  console.log('worker-first', (await first).first);
  const result = new Promise<any>(resolve => worker.once('message', resolve));
  worker.postMessage({block:'B'}); worker.postMessage({end:true});
  console.log('worker-live-pipe', (await result).text);
  await worker.terminate();
}
main();
