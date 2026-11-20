import {
  gzip, gunzip, deflate, inflate, deflateRaw, inflateRaw, unzip,
  brotliCompress, brotliDecompress, zstdCompress, zstdDecompress,
  gzipSync, deflateSync, deflateRawSync, brotliCompressSync, zstdCompressSync,
} from 'node:zlib';
import { promisify } from 'node:util';

const text = 'options reach the codec worker '.repeat(300);
const input = Buffer.from(text);
const cases: any[] = [
  ['gzip', gzip, input], ['deflate', deflate, input], ['deflateRaw', deflateRaw, input],
  ['gunzip', gunzip, gzipSync(input)], ['inflate', inflate, deflateSync(input)],
  ['inflateRaw', inflateRaw, deflateRawSync(input)], ['unzip', unzip, gzipSync(input)],
  ['brotliCompress', brotliCompress, input], ['brotliDecompress', brotliDecompress, brotliCompressSync(input)],
  ['zstdCompress', zstdCompress, input], ['zstdDecompress', zstdDecompress, zstdCompressSync(input)],
];
for (const [name, fn, data] of cases) {
  let calls = 0;
  for (const mode of ['two', 'options', 'undefined', 'null']) {
    let returned = false;
    const result: any = await new Promise((resolve, reject) => {
      const cb = (error: any, output: any) => {
        calls++;
        if (!returned) return reject(new Error('synchronous callback'));
        if (error) reject(error); else resolve(output);
      };
      if (mode === 'two') fn(data, cb);
      else if (mode === 'options') fn(data, { level: 6, chunkSize: 128 }, cb);
      else if (mode === 'undefined') fn(data, undefined, cb);
      else fn(data, null, cb);
      returned = true;
    });
    console.log(name, mode, Buffer.isBuffer(result), result.length > 0);
  }
  console.log(name, 'callbacks', calls);
}

// Direct native-table calls and value/promisify dispatch must preserve options.
const direct: any = await new Promise((resolve, reject) => gzip(input, { level: 0 }, (e: any, b: any) => e ? reject(e) : resolve(b)));
console.log('direct stored', direct.length > input.length);
const directBrotli: any = await new Promise((resolve, reject) => brotliCompress(input, { level: 6 }, (e: any, b: any) => e ? reject(e) : resolve(b)));
console.log('direct brotli', directBrotli.length > 0);
for (const [name, fn, decode] of [['gzip', gzip, gunzip], ['deflate', deflate, inflate], ['deflateRaw', deflateRaw, inflateRaw]] as any[]) {
  const compress: any = promisify(fn);
  const decompress: any = promisify(decode);
  const stored = await compress(input, { level: 0 });
  const packed = await compress(input, { level: 9 });
  console.log(name, 'levels', stored.length > input.length, packed.length < stored.length, (await decompress(packed, {})).toString() === text);
  try { await compress(input, { level: 10 }); }
  catch (e: any) { console.log(name, 'invalid level', e.code); }
}
for (const cb of [undefined, null, 42, {}]) {
  try { gzip(input, {}, cb as any); }
  catch (e: any) { console.log('invalid callback', e.code); }
}
// Node chooses the second argument when it is callable, even with a third.
await new Promise((resolve, reject) => gzip(input, (e: any, b: any) => {
  console.log('second callback', !e, Buffer.isBuffer(b)); resolve(undefined);
}, (() => reject(new Error('wrong callback'))) as any));
