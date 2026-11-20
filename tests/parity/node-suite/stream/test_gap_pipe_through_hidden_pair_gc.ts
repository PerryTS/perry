// #11621: hidden-handle getters can return the original or a distinct pair.
declare const gc: undefined | (() => void);
function collect() { if (typeof gc === "function") gc(); }
async function check(distinct: boolean) {
  const t = new TransformStream<Uint8Array, Uint8Array>();
  const inner:any = { get readable() { collect(); return t.readable; }, get writable() { collect(); return t.writable; } };
  const pair:any = distinct ? { get readable() { collect(); return t.readable; }, get writable() { collect(); return t.writable; } } : inner;
  Object.defineProperty(pair, "__perry_stream_handle__", { get() { collect(); return inner; } });
  const input = new ReadableStream<Uint8Array>({start(c){c.enqueue(new Uint8Array([1,2]));c.close();}});
  let size=0; for await (const chunk of input.pipeThrough(pair) as any) size+=chunk.length; console.log("size",distinct,size);
}
await check(false); await check(true);
