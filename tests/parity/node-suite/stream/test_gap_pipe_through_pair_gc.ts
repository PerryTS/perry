// #11621: the argument must survive options evaluation and both getter calls.
declare const gc: undefined | (() => void);
function collect() { if (typeof gc === "function") gc(); }
const transform = new TransformStream<Uint8Array, Uint8Array>();
const order: string[] = [];
let pair: any = {
  get readable() { order.push("readable"); collect(); return transform.readable; },
  get writable() { order.push("writable"); collect(); return transform.writable; },
};
function options() { order.push("options"); pair = null; collect(); return {}; }
const input = new ReadableStream<Uint8Array>({ start(c) { c.enqueue(new Uint8Array([1, 2])); c.close(); } });
let size = 0;
for await (const chunk of input.pipeThrough(pair, options()) as any) size += chunk.length;
console.log("size", size);
console.log("order", order.join(","));
