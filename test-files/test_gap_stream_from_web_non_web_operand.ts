// `fromWeb` of a value that is not a web stream is a TypeError, as in node,
// and an `@@hasInstance` hook runs with C as its receiver. Next.js's
// `e instanceof Readable ? e : Readable.fromWeb(e)` once took the second arm
// for a real Readable and then spun forever inside fromWeb.
import { Readable, Writable, Duplex, PassThrough } from "node:stream";

function attempt(label: string, f: () => unknown) {
  try {
    const r = f();
    console.log(label, "returned", r instanceof Readable || r instanceof Writable);
  } catch (e: any) {
    console.log(label, "threw", e instanceof TypeError, e.code, e.message);
  }
}
attempt("1 Readable.fromWeb(PassThrough)", () => Readable.fromWeb(new PassThrough() as any));
attempt("2 Readable.fromWeb({readable})", () => Readable.fromWeb({ readable: true } as any));
attempt("3 Writable.fromWeb(PassThrough)", () => Writable.fromWeb(new PassThrough() as any));
attempt("4 Duplex.fromWeb({readable, writable})", () => Duplex.fromWeb({ readable: 1, writable: 2 } as any));

const web = new ReadableStream({ start(c) { c.enqueue(new TextEncoder().encode("hi")); c.close(); } });
const node = Readable.fromWeb(web as any);
const chunks: string[] = [];
node.on("data", (d: any) => chunks.push(Buffer.from(d).toString()));
node.on("end", () => console.log("5 web stream read", node instanceof Readable, chunks.join("")));

function F() {}
Object.defineProperty(F, Symbol.hasInstance, { value: function (this: unknown, v: unknown) { return this === F && v === 1; } });
console.log("6 own hook receiver", (1 as any) instanceof (F as any), (2 as any) instanceof (F as any));

function G(this: any) {}
Object.defineProperty(G, Symbol.hasInstance, { value: Function.prototype[Symbol.hasInstance] });
console.log("7 copied default hook", new (G as any)() instanceof (G as any), ({}) instanceof (G as any));
