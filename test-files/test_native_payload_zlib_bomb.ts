// Full Z2: run separately from the ordinary gap suite (about five minutes).
// Node needs --expose-gc to establish the same post-fixture RSS baseline.
import { gzipSync, createGunzip, crc32 } from 'node:zlib';
import { Writable, pipeline } from 'node:stream';

const size = 100_000_000;
declare function gc(): void;
function fixture() {
  const input = Buffer.alloc(size, 65);
  return { expected: crc32(input), compressed: gzipSync(input) };
}
const { expected, compressed } = fixture();
await new Promise<void>(resolve => setImmediate(resolve));
gc();
const codec = createGunzip({ chunkSize: 16384, readableHighWaterMark: 16384 });
let received = 0, checksum = 0, maximumReadable = 0;
let baseline = process.memoryUsage().rss, peak = baseline;
const consumer = new Writable({ highWaterMark: 16384, write(chunk, _encoding, callback) {
  received += chunk.length;
  checksum = crc32(chunk, checksum);
  maximumReadable = Math.max(maximumReadable, codec.readableLength);
  peak = Math.max(peak, process.memoryUsage().rss);
  setTimeout(callback, 50);
} });
await new Promise<void>((resolve, reject) => {
  pipeline(codec, consumer, (error) => error ? reject(error) : resolve());
  codec.end(compressed);
});
console.log('bomb', received === size, checksum === expected, maximumReadable <= 32768);
console.error(JSON.stringify({ compressed: compressed.length, baseline, peak, rssDelta: peak - baseline,
  maximumReadable, readableBound: 32768 }));
