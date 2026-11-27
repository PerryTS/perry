import { createGzip, gunzipSync, crc32 } from 'node:zlib';
import { Readable, pipeline } from 'node:stream';

const input = Buffer.alloc(65536);
for (let i = 0; i < input.length; i++) input[i] = (i * 31 + (i >> 8)) & 255;
const chunks = [input.subarray(0, 8192), input.subarray(8192, 32768), input.subarray(32768)];
async function run(path: string) {
  const codec = createGzip({ chunkSize: 1024, writableHighWaterMark: 4096 });
  const events: string[] = [];
  const output: any[] = [];
  for (const event of ['drain', 'finish', 'end', 'close']) codec.on(event, () => events.push(event));
  if (path !== 'iterator') codec.on('data', (chunk: any) => { events.push('data'); output.push(chunk); });
  const closed = new Promise<void>((resolve, reject) => { codec.on('close', resolve); codec.on('error', reject); });
  if (path === 'write') {
    for (const chunk of chunks) codec.write(chunk);
    codec.end();
  } else if (path === 'pipe') {
    Readable.from(chunks).pipe(codec);
  } else if (path === 'pipeline') {
    pipeline(Readable.from(chunks), codec, (error: any) => { if (error) throw error; });
  } else {
    codec.end(input);
    for await (const chunk of codec) { events.push('data'); output.push(chunk); }
  }
  await closed;
  const encoded = Buffer.concat(output);
  const decoded = gunzipSync(encoded);
  // How many 'data' chunks arrive, and where, follows the compressed size,
  // which is the backend's own (not Node's zlib).
  const lifecycle = events.filter((event) => event !== 'data');
  console.log(path, output.length > 0, decoded.equals(input), crc32(decoded), lifecycle.join(','));
}
for (const path of ['write', 'pipe', 'pipeline', 'iterator']) await run(path);
