import { parentPort, workerData } from 'node:worker_threads';
function pressure() {
  const live: any[] = [];
  let i = 0;
  while (i < 64) { live.push({ index: i, bytes: new Uint8Array(8192) }); i++; }
  let sum = 0;
  i = 0;
  while (i < live.length) { sum += live[i].index; i++; }
  if (sum !== 2016) throw new Error('pressure checksum');
}
const ab = workerData;
const u8 = new Uint8Array(ab);
pressure();
parentPort!.postMessage([ab instanceof ArrayBuffer, ab.byteLength, u8[0], u8[1], u8[2]].join(','));
