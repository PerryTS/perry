import { Readable } from 'node:stream';
import { createGunzip, gzipSync } from 'node:zlib';
import { fixture } from './fetch_stream_fixture.ts';
async function* rest(iterator: AsyncIterator<Uint8Array>, first: Uint8Array) {
  yield first;
  for (;;) { const step = await iterator.next(); if (step.done) return; yield step.value; }
}
async function main() {
  const server = await fixture();
  try {
    const before = Date.now();
    const response = await fetch(server.url + '/rawgzip');
    console.log('pipeline-headers', Date.now() - before < 180);
    const iterator = response.body![Symbol.asyncIterator]();
    const first = await iterator.next();
    const input = Readable.from(rest(iterator, first.value!));
    const output = input.pipe(createGunzip());
    let text = '', chunks = 0, firstAt = 0, lastAt = 0;
    for await (const chunk of output) { text += chunk.toString(); if (chunks++ === 0) firstAt = Date.now(); lastAt = Date.now(); }
    console.log('pipeline-stream', chunks > 1, firstAt - before < 350, lastAt - firstAt >= 120, text);
    const largeBefore=Date.now();
    const large=await fetch(server.url+'/rawgzip-large');
    const largeIterator=large.body![Symbol.asyncIterator]();
    const largeFirst=await largeIterator.next();
    const decoded=Readable.from(rest(largeIterator,largeFirst.value!)).pipe(createGunzip({chunkSize: 1048576, readableHighWaterMark: 4194304}));
    let bytes=0, sum=0, firstLarge=0;
    for await (const chunk of decoded) {
      if(bytes===0) firstLarge=Date.now();
      bytes+=chunk.length;
      for(let i=0;i<chunk.length;i++)sum+=chunk[i];
    }
    console.log('pipeline-drain',bytes===1048576,sum===133773987,firstLarge-largeBefore<600);
    const damaged=gzipSync(Buffer.from('broken'));
    damaged[damaged.length-8]^=1;
    let rejected=false;
    try {
      const output=Readable.from([damaged]).pipe(createGunzip());
      for await (const chunk of output) { }
    } catch { rejected=true; }
    console.log('pipeline-error',rejected);
  } finally { await server.close(); }
}
main();
