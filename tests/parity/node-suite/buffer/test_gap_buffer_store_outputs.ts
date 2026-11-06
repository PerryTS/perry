import { Buffer } from 'node:buffer';
import { gzipSync, gunzipSync } from 'node:zlib';
import { createHash, randomBytes } from 'node:crypto';
declare function gc(): void;
for (const size of [64, 4096, 262144]) {
  const input = Buffer.alloc(size);
  for (let i = 0; i < size; i++) input[i] = (i * 31 + (i >>> 4)) & 255;
  const compressed = gzipSync(input, { level: 0 });
  const output = gunzipSync(compressed);
  if (typeof gc === 'function') gc();
  console.log('codec', size, output.equals(input), compressed.subarray(0, 2).toString('hex'), output.length);
  const digest = createHash('sha256').update(output).digest();
  if (typeof gc === 'function') gc();
  console.log('digest', digest.toString('hex'));
  const random = randomBytes(size);
  console.log('random', size, random.length, random.some((x: number) => x !== 0));
}
