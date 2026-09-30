// #11621: object-backed native transforms must expose endpoints to pipeThrough.
import * as zlib from "node:zlib";
async function count(stream: ReadableStream<Uint8Array>): Promise<number> {
  let n = 0;
  for await (const chunk of stream as any) n += chunk.length;
  return n;
}
function source(bytes: Uint8Array): ReadableStream<Uint8Array> {
  return new ReadableStream({ start(c) {
    c.enqueue(bytes.subarray(0, 10)); c.enqueue(bytes.subarray(10)); c.close();
  }});
}
const bytes = new Uint8Array(10000).fill(65);
console.log("gzip", await count(source(zlib.gzipSync(bytes)).pipeThrough(new DecompressionStream("gzip"))));
console.log("roundtrip", await count(source(bytes).pipeThrough(new CompressionStream("gzip")).pipeThrough(new DecompressionStream("gzip"))));
const dynamicSource: any = source(zlib.gzipSync(bytes));
console.log("dynamic", await count(dynamicSource.pipeThrough(new DecompressionStream("gzip"))));
const ds = new DecompressionStream("deflate");
const pair = { readable: ds.readable, writable: ds.writable };
console.log("pair", await count(source(zlib.deflateSync(bytes)).pipeThrough(pair)));
const text = new ReadableStream<string>({ start(c) { c.enqueue("hello"); c.close(); }});
console.log("encoder", await count(text.pipeThrough(new TextEncoderStream())));
console.log("decoder", await count(source(new Uint8Array([104, 101, 108, 108, 111])).pipeThrough(new TextDecoderStream()) as any));
const transform = new TransformStream<Uint8Array, Uint8Array>();
console.log("identity", await count(source(bytes).pipeThrough(transform)));

const order: string[] = [];
const orderedTransform = new TransformStream<Uint8Array, Uint8Array>();
const getterPair = {
  get readable() { order.push("readable"); return orderedTransform.readable; },
  get writable() { order.push("writable"); return orderedTransform.writable; },
};
function options() { order.push("options"); return {}; }
const typedSource = new ReadableStream<Uint8Array>({ start(c) { c.enqueue(bytes); c.close(); } });
console.log("getter", await count(typedSource.pipeThrough(getterPair, options())));
console.log("order", order.join(","));
