// RSTREAM: ReadableStream, its default controller and its default reader have
// Node's prototype shape: own keys (public names and well-known symbols only),
// descriptors, prototype identities, constructor names and tags; prototype
// members are real functions that work through `.call`.
function pub(o: any): any[] {
  return Reflect.ownKeys(o).filter(
    (k: any) => typeof k === "string" || k === Symbol.asyncIterator || k === Symbol.toStringTag,
  );
}
function desc(o: any, k: any): string {
  const d: any = Object.getOwnPropertyDescriptor(o, k);
  if (!d) return "none";
  return (d.get ? "get" : "") + (d.set ? "set" : "") + ("value" in d ? typeof d.value : "") + ":" +
    (d.enumerable ? "e" : "") + (d.configurable ? "c" : "") + (d.writable ? "w" : "");
}
function shape(label: string, p: any): void {
  if (p === null || p === undefined) { console.log(label, String(p)); return; }
  for (const k of pub(p)) console.log(label, String(k), desc(p, k));
  console.log(label, "ctor", p.constructor && p.constructor.name, "tag", p[Symbol.toStringTag]);
}
const G: any = globalThis;
const RS: any = ReadableStream;
const P: any = RS.prototype;
shape("RS.prototype", P);
console.log("asyncIterator===values", P[Symbol.asyncIterator] === P.values);
let ctl: any = null;
const s: any = new RS({ start(c: any) { ctl = c; c.enqueue("a"); c.enqueue("b"); c.close(); } });
console.log("proto(s)===P", Object.getPrototypeOf(s) === P);
console.log("s own", pub(s).length);
console.log("s.getReader===P.getReader", s.getReader === P.getReader);
console.log("s.enqueue", typeof s.enqueue);
// A ReadableStream-typed binding: `enqueue`/`desiredSize` belong to the controller only.
const s3 = new ReadableStream({ start(c) { c.enqueue(1); } });
// @ts-ignore
try { s3.enqueue(2); console.log("s3.enqueue() no throw"); } catch (e: any) { console.log("s3.enqueue()", e.name); }
// @ts-ignore
console.log("s3.desiredSize", s3.desiredSize);
console.log("ctl===s", ctl === s);
console.log("ctl.getReader", typeof ctl.getReader);
const CP: any = Object.getPrototypeOf(ctl);
shape("ctl.prototype", CP);
console.log("RSDC global", typeof G.ReadableStreamDefaultController,
  G.ReadableStreamDefaultController && G.ReadableStreamDefaultController.prototype === CP);
console.log("ctl tag", Object.prototype.toString.call(ctl));
console.log("ctl instanceof RS", ctl instanceof RS);
const r: any = P.getReader.call(s);
console.log("locked", s.locked);
const RP: any = Object.getPrototypeOf(r);
shape("reader.prototype", RP);
console.log("RSDR global", typeof G.ReadableStreamDefaultReader,
  G.ReadableStreamDefaultReader && G.ReadableStreamDefaultReader.prototype === RP);
console.log("reader tag", Object.prototype.toString.call(r));
console.log("s tag", Object.prototype.toString.call(s));
try { P.getReader.call({}); console.log("wrong this: no throw"); } catch (e: any) { console.log("wrong this", e.name); }
try { new G.ReadableStreamDefaultController(); console.log("RSDC new: no throw"); } catch (e: any) { console.log("RSDC new", e.name); }
(async () => {
  const read = RP.read;
  const a = await read.call(r); const b = await read.call(r); const c = await read.call(r);
  console.log("reads", a.value, a.done, b.value, c.done);
  let c2: any = null;
  const s2: any = new RS({ start(c: any) { c2 = c; } });
  CP.enqueue.call(c2, 1); CP.enqueue.call(c2, 2);
  console.log("desiredSize", Object.getOwnPropertyDescriptor(CP, "desiredSize")!.get!.call(c2));
  CP.close.call(c2);
  const out: any[] = [];
  for await (const v of s2) out.push(v);
  console.log("iter", out.join(","));
  try { await RP.read.call({}); console.log("reader wrong this: resolved"); } catch (e: any) { console.log("reader wrong this", e.name); }
  const ts = new TransformStream({ transform(chunk: any, tc: any) { tc.enqueue(chunk * 2); }, flush(tc: any) { tc.terminate(); } });
  const w = ts.writable.getWriter(); w.write(21); w.close();
  const tr = ts.readable.getReader();
  console.log("transform", (await tr.read()).value, (await tr.read()).done);
})();
