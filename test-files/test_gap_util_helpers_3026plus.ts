// #3026 / #2934 / #2917 — node:util helper parity.
//
//   #3026 TextDecoder.decode input validation + ArrayBuffer-view support:
//         decode()/decode(undefined) → "", decode(null)/decode([..]) throw
//         ERR_INVALID_ARG_TYPE, ArrayBuffer/DataView/sliced-typed-array decode.
//   #2934 util.isDeepStrictEqual is prototype-sensitive: objects with the same
//         own props but different [[Prototype]] are NOT equal.
//   #2917 util.callbackify wrapper: sync throw propagates synchronously, a
//         non-thenable return throws synchronously, a thenable's `.then` runs.
//
// Validated byte-for-byte against `node --experimental-strip-types`.

import util from "node:util";

// ---- #3026 TextDecoder.decode -------------------------------------------
const dec = new TextDecoder();
const show = (name: string, fn: () => string) => {
  try {
    console.log(name, "OK", JSON.stringify(fn()));
  } catch (e: any) {
    console.log(name, "THROW", e.name, e.code);
  }
};
show("decode()", () => dec.decode());
show("decode(undefined)", () => dec.decode(undefined));
show("decode(null)", () => dec.decode(null as any));
show("decode([65])", () => dec.decode([65] as any));
show("decode(42)", () => dec.decode(42 as any));
show("decode(string)", () => dec.decode("hi" as any));

const ab = new ArrayBuffer(2);
const abView = new Uint8Array(ab);
abView[0] = 65;
abView[1] = 66;
show("decode ArrayBuffer", () => dec.decode(ab));

const dvBuf = new ArrayBuffer(2);
const dvView = new Uint8Array(dvBuf);
dvView[0] = 67;
dvView[1] = 68;
show("decode DataView", () => dec.decode(new DataView(dvBuf)));

show("decode Uint8Array", () => dec.decode(new Uint8Array([88, 89, 90])));

// ---- #2934 isDeepStrictEqual prototypes ---------------------------------
class A {
  x = 1;
}
class B {
  x = 1;
}
const nullProto: any = Object.create(null);
nullProto.x = 1;
console.log("dse null-proto:", util.isDeepStrictEqual({ x: 1 }, nullProto));
console.log("dse diff-ctor:", util.isDeepStrictEqual(new A(), new B()));
console.log("dse same-ctor:", util.isDeepStrictEqual(new A(), new A()));
console.log("dse plain:", util.isDeepStrictEqual({ x: 1 }, { x: 1 }));

// ---- #2917 callbackify wrapper ------------------------------------------
function probe(label: string, fn: () => any) {
  try {
    util.callbackify(fn as any)((err: any, value: any) => {
      console.log(label, "callback", err ? err.name : "ok:" + value);
    });
  } catch (err: any) {
    console.log(label, "throw", err.name);
  }
}

(async () => {
  probe("sync throw", () => {
    throw new Error("boom");
  });
  probe("non promise", () => 42);
  probe("thenable", () => ({
    then(resolve: any) {
      resolve("thenable ok");
    },
  }));
  await Promise.resolve();
})();
