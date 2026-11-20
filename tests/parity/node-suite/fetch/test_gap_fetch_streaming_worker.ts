import { Worker } from 'node:worker_threads';
import { fixture } from './fetch_stream_fixture.ts';
async function main() {
  const server = await fixture();
  try {
    const a = new Worker(new URL('./fetch_stream_worker.ts', import.meta.url), {workerData:{url:server.url,label:'a'}});
    const b = new Worker(new URL('./fetch_stream_worker.ts', import.meta.url), {workerData:{url:server.url,label:'b'}});
    const results = Promise.all([
      new Promise<any>(resolve => a.once('message', resolve)),
      new Promise<any>(resolve => b.once('message', resolve))
    ]);
    const mainBody = await (await fetch(server.url + '/parent?label=main')).json();
    const workers = await results;
    console.log('worker-agents', workers.every(x => x.progress), workers.every(x => x.matched), mainBody.value);
    await a.terminate(); await b.terminate();
  } finally { await server.close(); }
}
main();
