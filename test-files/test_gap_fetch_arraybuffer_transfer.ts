import { Worker } from 'node:worker_threads';
function pressure() {
  const live: any[] = [];
  let i = 0;
  while (i < 64) { live.push({ index: i, bytes: new Uint8Array(8192) }); i++; }
  let sum = 0;
  i = 0;
  while (i < live.length) { sum += live[i].index; i++; }
  if (sum !== 2016) throw new Error('pressure checksum');
}
const ab = await new Response(new Uint8Array([37, 91, 7])).arrayBuffer();
const first = new Uint8Array(ab), second = new Uint8Array(ab);
first[1] = 42;
pressure();
console.log('brand', ab instanceof ArrayBuffer, ab.byteLength, second[1]);
const guard = setTimeout(() => process.exit(2), 60000);
const w = new Worker(new URL('./_helpers/worker_fetch_arraybuffer.ts', import.meta.url), { workerData: ab, transferList: [ab] });
console.log('sender', ab.byteLength, first.length, second.length);
w.on('message', (m: any) => { console.log('worker', m); clearTimeout(guard); });
