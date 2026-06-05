import * as web from "node:stream/web";

async function collect(readable: any): Promise<string> {
  const reader = readable.getReader();
  const values: string[] = [];
  while (true) {
    const item = await reader.read();
    if (item.done) break;
    values.push(String(item.value));
  }
  return values.join("|");
}

const ReadableAlias = (web as any).ReadableStream;
const WritableAlias = (web as any).WritableStream;
const TransformAlias = (web as any).TransformStream;
const ByteLengthAlias = (web as any).ByteLengthQueuingStrategy;
const CountAlias = (web as any).CountQueuingStrategy;
const TextEncoderAlias = (web as any).TextEncoderStream;
const TextDecoderAlias = (web as any).TextDecoderStream;
const CompressionAlias = (web as any).CompressionStream;
const DecompressionAlias = (web as any).DecompressionStream;

const aliasReadable = new ReadableAlias({
  start(controller: any) {
    controller.enqueue("alias");
    controller.close();
  },
});
console.log("alias readable:", await collect(aliasReadable));

const memberReadable = new (web as any).ReadableStream({
  start(controller: any) {
    controller.enqueue("member");
    controller.close();
  },
});
console.log("member readable:", await collect(memberReadable));

const fromReadable = (web as any).ReadableStream.from(["from", "namespace"]);
console.log("namespace from:", await collect(fromReadable));

const writes: string[] = [];
const writable = new WritableAlias({
  write(chunk: any) {
    writes.push(String(chunk));
  },
});
const writer = writable.getWriter();
await writer.write("write");
await writer.close();
console.log("writable writes:", writes.join("|"));

const transform = new TransformAlias({
  transform(chunk: any, controller: any) {
    controller.enqueue(String(chunk).toUpperCase());
  },
});
const transformWriter = transform.writable.getWriter();
const transformReader = transform.readable.getReader();
const transformWrite = transformWriter.write("tx");
console.log("transform value:", (await transformReader.read()).value);
await transformWrite;
await transformWriter.close();

const byteStrategy = new ByteLengthAlias({ highWaterMark: 16 });
console.log("byte strategy:", byteStrategy.highWaterMark, byteStrategy.size({ byteLength: 3 }));

const countStrategy = new CountAlias({ highWaterMark: 4 });
console.log("count strategy:", countStrategy.highWaterMark, countStrategy.size("x"));

const encoder = new TextEncoderAlias();
console.log("encoder metadata:", encoder.encoding, typeof encoder.readable, typeof encoder.writable);

const decoder = new TextDecoderAlias("utf-8", { fatal: true, ignoreBOM: true });
console.log(
  "decoder metadata:",
  decoder.encoding,
  decoder.fatal,
  decoder.ignoreBOM,
  typeof decoder.readable,
  typeof decoder.writable,
);

const compression = new CompressionAlias("gzip");
console.log("compression sides:", typeof compression.readable, typeof compression.writable);

const decompression = new DecompressionAlias("gzip");
console.log("decompression sides:", typeof decompression.readable, typeof decompression.writable);
