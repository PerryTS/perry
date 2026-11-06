import { createDeflate, inflateSync } from 'node:zlib';

// An unpooled owner can transfer. The data listener detaches while the
// stream still owns its input; a byte pin must retain the native allocation.
const owner = new ArrayBuffer(1024 * 1024);
const input = Buffer.from(owner);
for (let i = 0; i < input.length; i++) input[i] = (i * 37 + (i >>> 7)) & 255;
const expected = Buffer.from(input);
// Leave room for one output window: Node itself throws ERR_OUT_OF_RANGE
// when a subsequent native write reuses a detached input window.
const codec = createDeflate({ chunkSize: 2 * 1024 * 1024 });
const chunks: Buffer[] = [];
let transferred = false;
codec.on('data', (chunk: Buffer) => {
  chunks.push(chunk);
  if (!transferred) {
    owner.transfer();
    transferred = true;
  }
});
codec.on('end', () => {
  const restored = inflateSync(Buffer.concat(chunks));
  console.log('detached=' + owner.detached + ' length=' + input.length);
  console.log('roundtrip=' + restored.equals(expected) + ' bytes=' + restored.length);
});
codec.end(input);
