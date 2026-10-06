import * as z from 'node:zlib';

const input = Buffer.from('one-shot/native-payload '.repeat(100));
for (const [encode, decode] of [['gzip', 'gunzip'], ['deflate', 'inflate'],
  ['deflateRaw', 'inflateRaw'], ['brotliCompress', 'brotliDecompress'], ['zstdCompress', 'zstdDecompress']]) {
  const encoded = (z as any)[encode + 'Sync'](input);
  const decoded = (z as any)[decode + 'Sync'](encoded);
  const asyncEncoded = await new Promise<Buffer>((resolve, reject) => (z as any)[encode](input,
    (error: any, value: Buffer) => error ? reject(error) : resolve(value)));
  const asyncDecoded = await new Promise<Buffer>((resolve, reject) => (z as any)[decode](asyncEncoded,
    (error: any, value: Buffer) => error ? reject(error) : resolve(value)));
  console.log(encode, decoded.equals(input), asyncDecoded.equals(input), encoded.equals(asyncEncoded),
    z.crc32(encoded), z.crc32(asyncEncoded));
}
console.log('unzip', z.unzipSync(z.gzipSync(input)).equals(input));
await new Promise<void>((resolve, reject) => z.unzip(z.deflateSync(input), (error, value) => {
  if (error) { reject(error); return; }
  console.log('unzip callback', value.equals(input));
  resolve();
}));
process.once('uncaughtException', (error: any) => console.log('uncaught callback', error.message));
z.gzip(input, (error, value) => {
  console.log('throwing callback', error === null, z.gunzipSync(value).equals(input));
  throw new Error('zlib callback witness');
});
