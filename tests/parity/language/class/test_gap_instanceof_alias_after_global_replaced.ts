// `x instanceof C` with a dynamic right-hand side runs C's own
// OrdinaryHasInstance: C[@@hasInstance] / C.prototype. It never reads the
// global binding named like C, so replacing `globalThis.<Name>` changes what
// `instanceof globalThis.<Name>` sees but not what an alias of the original
// constructor sees (also for instances created after the replacement).
export {};
const g: any = globalThis;
const families: [string, (C: any) => any][] = [
  ["EventTarget", (C) => new C()],
  ["Event", (C) => new C("e")],
  ["CustomEvent", (C) => new C("c", { detail: 1 })],
  ["AbortController", (C) => new C()],
  ["AbortSignal", (_C) => new g.AbortController().signal],
  ["DOMException", (C) => new C("d")],
  ["ReadableStream", (C) => new C()],
  ["WritableStream", (C) => new C()],
  ["TransformStream", (C) => new C()],
  ["Buffer", (C) => C.from("a")],
  ["Map", (C) => new C()],
  ["Set", (C) => new C()],
  ["Date", (C) => new C(0)],
  ["RegExp", (C) => new C("a")],
  ["ArrayBuffer", (C) => new C(4)],
  ["Uint8Array", (C) => new C(1)],
  ["Error", (C) => new C("e")],
  ["TypeError", (C) => new C("t")],
];
const rows: string[] = [];
for (const [name, make] of families) {
  const C = g[name];
  const before = make(C);
  g[name] = class Replacement {};
  const later = make(C);
  const row = [
    name,
    before instanceof C,
    later instanceof C,
    before instanceof g[name],
    later instanceof g[name],
    new g[name]() instanceof C,
  ].join(" ");
  g[name] = C;
  rows.push(row);
}
// A subclass of the original keeps answering against the original.
{
  const E = g.EventTarget;
  class Sub extends E {}
  g.EventTarget = class Replacement {};
  const s = new Sub();
  rows.push("subclass " + (s instanceof E) + " " + (s instanceof Sub) + " " + (s instanceof g.EventTarget));
  g.EventTarget = E;
}
for (const row of rows) console.log(row);
