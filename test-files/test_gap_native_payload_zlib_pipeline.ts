import { createGzip, createGunzip, crc32 } from 'node:zlib';
import { Readable, Writable, pipeline } from 'node:stream';
// The full design witness runs with 67108864; the gap fixture uses 1 MiB.
const size = Number(process.argv[2] ?? 1048576);
const mode = process.argv[3] ?? 'pipeline';
let position = 0;
let seed = 12345;
let expected = 0;
function* inputs() {
  while (position < size) {
    const chunk = Buffer.alloc(Math.min(16384, size - position));
    for (let i = 0; i < chunk.length; i++) { seed ^= seed << 13; seed ^= seed >>> 17; seed ^= seed << 5; chunk[i] = seed & 255; }
    position += chunk.length;
    expected = crc32(chunk, expected);
    yield chunk;
  }
}
const source = Readable.from(inputs(), { highWaterMark: 16384, objectMode: false });
const gzip = createGzip({ chunkSize: 16384 });
const gunzip = createGunzip({ chunkSize: 16384, readableHighWaterMark: 16384 });
let received = 0, checksum = 0, maximum = 0, drains = 0, falseWrites = 0;
const sink = new Writable({ highWaterMark: 16384, write(chunk: any, _encoding: any, cb: any) {
  received += chunk.length;
  checksum = crc32(chunk, checksum);
  maximum = Math.max(maximum, gunzip.readableLength);
  setTimeout(cb, 1);
} });
const original = sink.write;
sink.write = function (...args: any[]) { const accepted = original.apply(this, args as any); if (!accepted) falseWrites++; return accepted; } as any;
sink.on('drain', () => drains++);
await new Promise<void>((resolve, reject) => {
  if (mode === 'pipe') {
    for (const stream of [source, gzip, gunzip, sink]) stream.on('error', reject);
    sink.on('finish', resolve);
    source.pipe(gzip).pipe(gunzip).pipe(sink);
  } else {
    pipeline(source, gzip, gunzip, sink, (error: any) => error ? reject(error) : resolve());
  }
});
console.log(mode, received === size, checksum === expected, maximum <= 32768,
  Math.abs(falseWrites - Math.ceil(size / 16384)) <= 1,
  drains > 0 && Math.abs(falseWrites - drains) <= 1);
if (process.argv[4] === 'counts') console.log('counts', falseWrites, drains);
