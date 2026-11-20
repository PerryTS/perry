import * as zlib from 'node:zlib';

const backing = Buffer.alloc(32832, 219);
for (let i = 32; i < backing.length - 32; i++) backing[i] = (i * 31 + (i >>> 7)) & 255;
const input = backing.subarray(19, backing.length - 17).subarray(13, 32781);
const expected = Buffer.from(input);
console.log('crc slice', zlib.crc32(input) === zlib.crc32(expected));
for (const [encode, decode] of [['gzip', 'gunzip'], ['deflate', 'inflate'],
  ['deflateRaw', 'inflateRaw'], ['brotliCompress', 'brotliDecompress'], ['zstdCompress', 'zstdDecompress']]) {
  const sync = (zlib as any)[encode + 'Sync'](input);
  const framed = Buffer.concat([Buffer.alloc(7, 233), sync, Buffer.alloc(11, 177)]);
  const decoded = (zlib as any)[decode + 'Sync'](framed.subarray(7, framed.length - 11));
  const async = await new Promise<Buffer>((resolve, reject) => (zlib as any)[encode](input,
    (error: Error | null, value: Buffer) => error ? reject(error) : resolve(value)));
  const roundTrip = (zlib as any)[decode + 'Sync'](async);
  console.log(encode, decoded.equals(expected), roundTrip.equals(expected));
}
