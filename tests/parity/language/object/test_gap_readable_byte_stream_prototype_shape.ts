// RSTREAM A2: the byte-stream families (`type: "bytes"`): ReadableByteStreamController,
// ReadableStreamBYOBReader and ReadableStreamBYOBRequest have Node's globals, prototype
// shape (own keys: public names and well-known symbols only; descriptors; lengths),
// prototype identities, constructor names and tags; members work through `.call`.
function pub(o: any): any[] {
  return Reflect.ownKeys(o).filter((k: any) => typeof k === "string" || k === Symbol.toStringTag);
}
function desc(o: any, k: any): string {
  const d: any = Object.getOwnPropertyDescriptor(o, k);
  if (!d) return "none";
  const len = typeof d.value === "function" ? "/" + d.value.length : "";
  return (d.get ? "get" : "") + (d.set ? "set" : "") + ("value" in d ? typeof d.value : "") + len + ":" +
    (d.enumerable ? "e" : "") + (d.configurable ? "c" : "") + (d.writable ? "w" : "");
}
function shape(label: string, p: any): void {
  if (p === null || p === undefined) { console.log(label, String(p)); return; }
  for (const k of pub(p)) console.log(label, String(k), desc(p, k));
  console.log(label, "ctor", p.constructor && p.constructor.name, "tag", p[Symbol.toStringTag]);
}
function err(label: string, f: () => any): void {
  try { f(); console.log(label, "no throw"); } catch (e: any) { console.log(label, e.name, e.code); }
}
const G: any = globalThis;
const NAMES = ["ReadableByteStreamController", "ReadableStreamBYOBReader", "ReadableStreamBYOBRequest"];
// The first observation of each constructor is its own-keys listing.
for (const n of NAMES) console.log(n, "first-keys", Object.getOwnPropertyNames(G[n]).join(","));
for (const n of NAMES) {
  console.log(n, typeof G[n], n in G, Object.getOwnPropertyNames(G).includes(n), desc(G, n), G[n].length, G[n].name);
}
err("new RBSC", () => new G.ReadableByteStreamController());
err("new BYOBRequest", () => new G.ReadableStreamBYOBRequest());
err("new BYOBReader()", () => new G.ReadableStreamBYOBReader());
err("new BYOBReader({})", () => new G.ReadableStreamBYOBReader({}));
err("new BYOBReader(default stream)", () => new G.ReadableStreamBYOBReader(new ReadableStream()));
const CP: any = G.ReadableByteStreamController.prototype;
const RP: any = G.ReadableStreamBYOBReader.prototype;
const QP: any = G.ReadableStreamBYOBRequest.prototype;
shape("RBSC.prototype", CP);
shape("BYOBReader.prototype", RP);
shape("BYOBRequest.prototype", QP);
let ctl: any = null;
const s: any = new ReadableStream({ type: "bytes", start(c: any) { ctl = c; } } as any);
console.log("ctl proto===CP", Object.getPrototypeOf(ctl) === CP, ctl.constructor.name, Object.prototype.toString.call(ctl));
console.log("ctl instanceof", ctl instanceof G.ReadableByteStreamController, ctl instanceof G.ReadableStreamDefaultController);
console.log("ctl own", pub(ctl).length, "desiredSize", ctl.desiredSize, "byobRequest", ctl.byobRequest);
console.log("ctl.enqueue===CP.enqueue", ctl.enqueue === CP.enqueue);
console.log("desiredSize via get", Object.getOwnPropertyDescriptor(CP, "desiredSize")!.get!.call(ctl));
err("ctl wrong this", () => CP.close.call({}));
err("BYOBRequest view wrong this", () => Object.getOwnPropertyDescriptor(QP, "view")!.get!.call({}));
const r: any = s.getReader({ mode: "byob" });
console.log("reader proto===RP", Object.getPrototypeOf(r) === RP, r.constructor.name, Object.prototype.toString.call(r));
console.log("reader instanceof", r instanceof G.ReadableStreamBYOBReader, r instanceof G.ReadableStreamDefaultReader);
console.log("r.read===RP.read", r.read === RP.read, "locked", s.locked);
err("BYOBReader on locked", () => new G.ReadableStreamBYOBReader(s));
const s2: any = new ReadableStream({ type: "bytes" } as any);
const r2: any = new G.ReadableStreamBYOBReader(s2);
console.log("constructed", Object.getPrototypeOf(r2) === RP, s2.locked);
RP.releaseLock.call(r2);
console.log("released", s2.locked);
(async () => {
  const p = RP.read.call(r, new Uint8Array(4));
  const req: any = ctl.byobRequest;
  console.log("req proto===QP", Object.getPrototypeOf(req) === QP, req.constructor.name, Object.prototype.toString.call(req));
  console.log("req own", pub(req).length, "view", req.view.constructor.name, req.view.byteLength);
  req.view[0] = 7; req.view[1] = 9;
  QP.respond.call(req, 2);
  const v = await p;
  console.log("read", v.done, v.value.constructor.name, v.value.byteLength, v.value[0], v.value[1]);
  console.log("after respond byobRequest", ctl.byobRequest);
  const p2 = r.read(new Uint8Array(3));
  const req2: any = ctl.byobRequest;
  const nv = new Uint8Array(req2.view.buffer, req2.view.byteOffset, 1); nv[0] = 5;
  req2.respondWithNewView(nv);
  const v2 = await p2;
  console.log("read2", v2.done, v2.value.byteLength, v2.value[0]);
  ctl.enqueue(new Uint8Array([1, 2]));
  const v3 = await r.read(new Uint8Array(8));
  console.log("read3", v3.done, v3.value.byteLength, v3.value[0], v3.value[1]);
  try { await RP.read.call({}, new Uint8Array(1)); console.log("reader wrong this: resolved"); } catch (e: any) { console.log("reader wrong this", e.name, e.code); }
  CP.close.call(ctl);
  const v4 = await r.read(new Uint8Array(2));
  console.log("read4", v4.done, v4.value && v4.value.byteLength);
  console.log("closed", await Object.getOwnPropertyDescriptor(RP, "closed")!.get!.call(r));
})();
