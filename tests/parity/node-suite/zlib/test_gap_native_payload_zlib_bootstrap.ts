// Import only zlib: its inherited methods must not depend on a stream import.
import * as z from 'node:zlib';
const earlyPrototype = z.Gunzip.prototype;
console.log('prototype', typeof earlyPrototype.on, typeof earlyPrototype.end);
class Decoder extends z.Gunzip {}
const earlySubclass = new Decoder();
async function consume(stream: any, input: Buffer, expected: Buffer) {
  console.log('methods', typeof stream.on, typeof stream.end, typeof stream.pipe);
  const chunks: Buffer[] = [];
  await new Promise<void>((resolve, reject) => {
    stream.on('data', chunk => chunks.push(chunk));
    stream.on('error', reject);
    stream.on('end', resolve);
    stream.end(input);
  });
  console.log('bytes', Buffer.concat(chunks).equals(expected));
}
const input = Buffer.from('inherited methods without namespace side effects');
await consume(earlySubclass, z.gzipSync(input), input);
await consume(z.createGunzip(), z.gzipSync(input), input);
await consume(z.createInflate(), z.deflateSync(input), input);
await consume(z.createBrotliDecompress(), z.brotliCompressSync(input), input);
await consume(z.createZstdDecompress(), z.zstdCompressSync(input), input);
