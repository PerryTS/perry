// #11261: `3 instanceof EventEmitter` printed `true`. The native brand probes
// behind `instanceof EventEmitter` (and net.Socket, AsyncLocalStorage, …)
// resolved their operand as a registry handle id, and that resolution also
// accepted a plain positive integral NUMBER. So any number equal to a live
// emitter's handle id was an "emitter". Which ids are live depends on how
// `node:events` is linked: the perry-stdlib-bundled emitter mints ids from 1,
// the perry-ext-events wrapper from 0x38000 — so this sweeps the whole handle
// band [1, 0x40000) instead of guessing one id.
//
// OrdinaryHasInstance step 3: a primitive is never `instanceof` anything
// (unless a user `Symbol.hasInstance` says otherwise — covered at the end).
import { EventEmitter } from "node:events";

const rt = <T>(v: T): T => JSON.parse(JSON.stringify(v));

// Several live emitters, so small ids AND the wrapper's band are occupied.
const emitters: EventEmitter[] = [];
for (let i = 0; i < 8; i++) emitters.push(new EventEmitter());

class Plain {}
class Child extends Plain {}
class MyEmitter extends EventEmitter {}
const mine = new MyEmitter();

// Static-RHS sweep over the handle band.
let eeHits = 0;
let firstHit = -1;
for (let n = 1; n < 0x40000; n++) {
  const v: any = rt(n);
  if (v instanceof EventEmitter) {
    eeHits++;
    if (firstHit < 0) firstHit = n;
  }
}
console.log("numbers instanceof EventEmitter:", eeHits, "first:", firstHit);

// Dynamic-RHS sweep (constructor held in a variable).
const EEdyn: any = [EventEmitter][rt(0)];
let dynHits = 0;
for (let n = 1; n < 0x40000; n++) {
  if ((rt(n) as any) instanceof EEdyn) dynHits++;
}
console.log("numbers instanceof (dynamic) EventEmitter:", dynHits);

const primitives: [string, any][] = [
  ["number", rt(3)],
  ["zero", rt(0)],
  ["negative", rt(-1)],
  ["float", rt(1.5)],
  ["NaN", NaN],
  ["string", rt("x")],
  ["long string", rt("y".repeat(40))],
  ["empty string", rt("")],
  ["true", rt(true)],
  ["false", rt(false)],
  ["null", rt(null)],
  ["undefined", undefined],
  ["bigint", BigInt(rt(3))],
  ["symbol", Symbol("s")],
];

const ctors: [string, any][] = [
  ["EventEmitter", EventEmitter],
  ["Buffer", Buffer],
  ["Uint8Array", Uint8Array],
  ["Map", Map],
  ["Error", Error],
  ["Object", Object],
  ["Plain", Plain],
  ["Child", Child],
  ["MyEmitter", MyEmitter],
];

for (const [pname, p] of primitives) {
  const statics = [
    p instanceof EventEmitter,
    p instanceof Buffer,
    p instanceof Uint8Array,
    p instanceof Map,
    p instanceof Error,
    p instanceof Object,
    p instanceof Plain,
    p instanceof Child,
    p instanceof MyEmitter,
  ];
  const dynamics = ctors.map(([, C]) => p instanceof C);
  const reflective = ctors.map(([, C]) =>
    (Function.prototype as any)[Symbol.hasInstance].call(C, p),
  );
  console.log(
    `${pname}:`,
    statics.some(Boolean) || dynamics.some(Boolean) || reflective.some(Boolean)
      ? `MATCHED static=${statics} dynamic=${dynamics} reflective=${reflective}`
      : "false everywhere",
  );
}

// Controls: real instances still match.
console.log("emitter:", emitters[0] instanceof EventEmitter, emitters[7] instanceof EEdyn);
console.log("subclass:", mine instanceof EventEmitter, mine instanceof MyEmitter);
console.log("buffer:", Buffer.from("ab") instanceof Buffer, Buffer.from("ab") instanceof Uint8Array);
console.log("u8:", new Uint8Array(2) instanceof Uint8Array);
console.log("map:", new Map() instanceof Map, "error:", new Error("e") instanceof Error);
console.log("user:", new Child() instanceof Plain, new Child() instanceof Child);
console.log("object:", {} instanceof Object, [] instanceof Object);
console.log("class instanceof Function:", Plain instanceof Function);

// A user `Symbol.hasInstance` still decides for primitives.
class Even {
  static [Symbol.hasInstance](v: unknown): boolean {
    return typeof v === "number" && v % 2 === 0;
  }
}
const EvenDyn: any = [Even][rt(0)];
console.log("hasInstance:", (rt(4) as any) instanceof Even, (rt(3) as any) instanceof Even);
console.log("hasInstance dynamic:", (rt(4) as any) instanceof EvenDyn, (rt(3) as any) instanceof EvenDyn);

// Emitters still work after all of the checks.
let fired = 0;
const e0 = new EventEmitter();
e0.on("x", (n: number) => (fired += n));
e0.emit("x", 5);
console.log("fired:", fired);
