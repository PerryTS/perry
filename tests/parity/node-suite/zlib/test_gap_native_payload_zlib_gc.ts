import * as zlib from 'node:zlib';

const input = Buffer.alloc(1048576);
for (let i = 0; i < input.length; i++) input[i] = (i * 13 + (i >>> 8)) & 255;
for (const [encode, decode] of [['Gzip', 'Gunzip'], ['Deflate', 'Inflate'], ['DeflateRaw', 'InflateRaw'], ['BrotliCompress', 'BrotliDecompress'], ['ZstdCompress', 'ZstdDecompress']]) {
  const encoded = await new Promise<Buffer>((resolve, reject) => {
    const codec: any = (zlib as any)['create' + encode]({ chunkSize: 1024 });
    const chunks: Buffer[] = [];
    codec.on('data', (chunk: Buffer) => {
      chunks.push(chunk);
      // Force nursery turnover while the next native step is still queued.
      const garbage = Buffer.alloc(10 * 1024 * 1024, 42);
      if (garbage[1024] !== 42) throw new Error('allocation lost');
    });
    codec.on('error', reject);
    codec.on('end', () => resolve(Buffer.concat(chunks)));
    codec.end(input);
  });
  const output = await new Promise<Buffer>((resolve, reject) => {
    const codec: any = (zlib as any)['create' + decode]({ chunkSize: 16384 });
    const chunks: Buffer[] = [];
    codec.on('data', (chunk: Buffer) => {
      chunks.push(chunk);
      const garbage = Buffer.alloc(10 * 1024 * 1024, 43);
      if (garbage[1024] !== 43) throw new Error('allocation lost');
    });
    codec.on('error', reject);
    codec.on('end', () => resolve(Buffer.concat(chunks)));
    codec.end(encoded);
  });
  console.log(encode, output.equals(input), zlib.crc32(output));
}
