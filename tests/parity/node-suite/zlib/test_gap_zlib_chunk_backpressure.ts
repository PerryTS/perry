import { gzipSync, deflateSync, deflateRawSync, createGunzip, createInflate, createInflateRaw, createUnzip, createGzip } from 'node:zlib';
import { Writable } from 'node:stream';
const payload = Buffer.alloc(250000, 65);
const gz = gzipSync(payload);
async function events(label: string, stream: any, bytes: any, size: number) {
  let total = 0, maximum = 0, chunks = 0;
  await new Promise<void>((resolve, reject) => {
    stream.on('data', (chunk: any) => { total += chunk.length; maximum = Math.max(maximum, chunk.length); chunks++; });
    stream.on('error', reject);
    stream.on('end', resolve);
    stream.end(bytes);
  });
  console.log(label, total === payload.length, maximum <= size, chunks > 1);
}
await events('default', createGunzip(), gz, 16384);
await events('tiny', createGunzip({chunkSize: 1024}), gz, 1024);
await events('inflate', createInflate({chunkSize: 4096}), deflateSync(payload), 4096);
await events('raw', createInflateRaw({chunkSize: 4096}), deflateRawSync(payload), 4096);
await events('unzip', createUnzip({chunkSize: 4096}), gz, 4096);

// With no consumer, the transform cannot finish a write that expands beyond
// its readable HWM. A late consumer must still get every byte in order.
const late = createGunzip({chunkSize: 1024, readableHighWaterMark: 1024});
let finished = false, writeDone = false;
late.on('finish', () => { finished = true; });
late.write(gz, () => { writeDone = true; });
late.end();
await new Promise(resolve => setTimeout(resolve, 30));
console.log('blocked', finished, writeDone);
let lateTotal = 0;
for await (const chunk of late) lateTotal += chunk.length;
console.log('late', lateTotal === payload.length, finished, writeDone);
let afterEnd = 0;
for await (const chunk of late) afterEnd++;
console.log('after end', afterEnd === 0);

const paused = createGunzip({chunkSize: 1024, readableHighWaterMark: 1024});
let seen = 0, resumed = false, pausedCorrectly = false;
await new Promise<void>((resolve, reject) => {
  paused.on('error', reject);
  paused.on('data', () => {
    seen++;
    if (seen === 1) {
      paused.pause();
      setTimeout(() => { pausedCorrectly = seen === 1; resumed = true; paused.resume(); }, 20);
    }
  });
  paused.on('end', resolve);
  paused.end(gz);
});
console.log('pause', pausedCorrectly, resumed, seen > 1);

// A slow writable makes pipe return false and later emit drain. The readable
// must wait for that drain and retain event ordering through the final end.
let piped = 0;
const sink = new Writable({highWaterMark: 1, write(chunk, _encoding, cb) { piped += chunk.length; setTimeout(cb, 1); }});
await new Promise<void>((resolve, reject) => {
  const source = createGunzip({chunkSize: 4096, readableHighWaterMark: 4096});
  source.on('error', reject);
  sink.on('finish', resolve);
  source.pipe(sink);
  source.end(gz);
});
console.log('pipe', piped === payload.length);

const early = createGunzip({chunkSize: 1024});
let earlyEnd = false, earlyClose = false;
early.on('end', () => { earlyEnd = true; });
early.on('close', () => { earlyClose = true; });
early.end(gz);
for await (const chunk of early) { break; }
await new Promise(resolve => setTimeout(resolve, 10));
console.log('break', earlyEnd, earlyClose);

const invalid = createGunzip({chunkSize: 1024});
invalid.end(Buffer.from('invalid'));
try { for await (const chunk of invalid) {} console.log('error', false); }
catch { console.log('error', true); }

// Compression also delivers bounded chunks for a large, poorly-compressible
// single write; compare reconstructed data rather than codec byte identity.
const input = Buffer.alloc(250000);
for (let i = 0; i < input.length; i++) input[i] = (i * 31 + (i >> 8)) & 255;
let maxCompressed = 0;
await new Promise<void>((resolve, reject) => {
  const compressor = createGzip({chunkSize: 64});
  compressor.on('data', (c: any) => { maxCompressed = Math.max(maxCompressed, c.length); });
  compressor.on('error', reject); compressor.on('end', resolve); compressor.end(input);
});
console.log('compress chunks', maxCompressed <= 64);

const destroyed = createGunzip({chunkSize: 1024});
const waiting = (async () => { for await (const chunk of destroyed) {} })();
setTimeout(() => destroyed.destroy(), 10);
try { await waiting; console.log('destroy pending', false); } catch (e: any) { console.log('destroy pending', e.code === 'ERR_STREAM_PREMATURE_CLOSE'); }

const cause = new Error('deliberate');
const explicit = createGunzip();
const explicitWaiting = (async () => { for await (const chunk of explicit) {} })();
setTimeout(() => explicit.destroy(cause), 10);
try { await explicitWaiting; console.log('destroy reason', false); } catch (e) { console.log('destroy reason', e === cause); }
try { for await (const chunk of explicit) {} console.log('late error', false); } catch (e) { console.log('late error', e === cause); }

const discard = createGunzip({chunkSize: 1024});
await new Promise<void>((resolve) => { discard.on('end', resolve); discard.end(gz); discard.resume(); });
console.log('resume without data', true);

const ordered = createGunzip({chunkSize: 1024, readableHighWaterMark: 1024});
const order: string[] = [];
await new Promise<void>((resolve, reject) => {
  ordered.on('error', reject);
  ordered.on('finish', () => order.push('finish'));
  ordered.on('end', () => order.push('end'));
  ordered.on('close', resolve);
  ordered.end(gz, () => order.push('end callback'));
  ordered.resume();
});
console.log('ordering', order.join(','));

const preserved = createGunzip({chunkSize: 1024, readableHighWaterMark: 1024});
preserved.end(gz);
const firstIterator = preserved.iterator({destroyOnReturn: false});
const firstChunk = await firstIterator.next();
await firstIterator.return();
let remaining = 0;
for await (const chunk of preserved) remaining += chunk.length;
console.log('return preserved', !firstChunk.done, firstChunk.value.length + remaining === payload.length);

const returned = createGunzip();
let abortCode = '';
returned.on('error', (e: any) => { abortCode = e.code; });
const returnIterator = returned[Symbol.asyncIterator]();
const outstanding = returnIterator.next();
const returning = returnIterator.return();
returned.end(gz);
console.log('return pending', (await outstanding).done);
await returning;
await new Promise(resolve => setTimeout(resolve, 10));
console.log('return destroyed', returned.destroyed, abortCode === 'ABORT_ERR');
