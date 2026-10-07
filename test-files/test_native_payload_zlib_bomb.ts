// Full Z2: run separately from the ordinary gap suite (about five minutes).
// The committed 97,222-byte Node fixture expands to 100 MB of byte 65.
// Generate it outside this process so its source cannot hide stream RSS growth.
import { createGunzip, crc32 } from 'node:zlib';
import { readFileSync } from 'node:fs';
import { Writable, pipeline } from 'node:stream';

const size = 100_000_000;
const expected = 2229916188;
const compressed = readFileSync(process.argv[2] ?? 'test-files/fixtures/zlib-bomb-100mb.gz');
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
