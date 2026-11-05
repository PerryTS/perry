import { Buffer } from 'node:buffer';
import { fixture } from './fetch_stream_fixture.ts';
function pressure() {
  const live: any[] = [];
  let i = 0;
  while (i < 64) { live.push({ index: i, bytes: new Uint8Array(8192) }); i++; }
  let sum = 0;
  i = 0;
  while (i < live.length) { sum += live[i].index; i++; }
  if (sum !== 2016) throw new Error('pressure checksum');
}
declare function gc(): void;
function witness(label: string, ab: ArrayBuffer) {
  console.log(label, ab instanceof ArrayBuffer, ArrayBuffer.isView(ab), Buffer.isBuffer(ab), ab.byteLength);
  const first = new Uint8Array(ab), second = new Uint8Array(ab);
  if (first.length) first[0] = 91;
  console.log('shared', first.buffer === ab, second.buffer === ab, second[0], first.length);
  pressure();
  if (typeof gc === 'function') gc();
  console.log('after-gc', second[0], second.byteLength);
  const copied = ab.slice(0, 2);
  console.log('slice', copied instanceof ArrayBuffer, copied.byteLength, new Uint8Array(copied)[0]);
  try { new Uint8Array(ab, ab.byteLength, 1); }
  catch (error: any) { console.log('bounded', error.name); }
}
async function main() {
  witness('constructed', await new Response(new Uint8Array([1, 2, 3])).arrayBuffer());
  witness('empty', await new Response(null).arrayBuffer());
  const server = await fixture();
  try { witness('network', await (await fetch(server.url + '/array')).arrayBuffer()); }
  finally { await server.close(); }
}
main();
