import { CompressionStream, DecompressionStream } from "node:stream/web";

async function collectBytes(readable: ReadableStream<Uint8Array>) {
  const reader = readable.getReader();
  const chunks: Uint8Array[] = [];
  let total = 0;
  while (true) {
    const chunk = await reader.read();
    if (chunk.done) {
      break;
    }
    chunks.push(chunk.value);
    total += chunk.value.byteLength;
  }
  const out = new Uint8Array(total);
  let offset = 0;
  for (const chunk of chunks) {
    out.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return out;
}

async function writeAndClose(writable: WritableStream<Uint8Array>, chunks: Uint8Array[]) {
  const writer = writable.getWriter();
  for (const chunk of chunks) {
    await writer.write(chunk);
  }
  await writer.close();
}

console.log("constructors:", CompressionStream.length, DecompressionStream.length);

for (const Ctor of [CompressionStream, DecompressionStream]) {
  for (const format of [undefined, "x-gzip"]) {
    try {
      format === undefined ? new Ctor(format as any) : new Ctor(format);
      console.log("bad format:", Ctor.name, String(format), "ok");
    } catch (err: any) {
      console.log("bad format:", Ctor.name, String(format), err.constructor.name, err.code);
    }
  }
}

for (const format of ["gzip", "deflate", "deflate-raw", "brotli"] as const) {
  const compressor = new CompressionStream(format);
  console.log(
    "codec streams:",
    format,
    compressor.readable instanceof ReadableStream,
    compressor.writable instanceof WritableStream,
    Object.prototype.toString.call(compressor),
  );

  const source = new TextEncoder().encode(`hello ${format}`);
  const compressed = collectBytes(compressor.readable);
  await writeAndClose(compressor.writable, [source.subarray(0, 3), source.subarray(3)]);

  const decompressor = new DecompressionStream(format);
  const plain = collectBytes(decompressor.readable);
  await writeAndClose(decompressor.writable, [await compressed]);

  console.log("codec roundtrip:", format, new TextDecoder().decode(await plain));
}
