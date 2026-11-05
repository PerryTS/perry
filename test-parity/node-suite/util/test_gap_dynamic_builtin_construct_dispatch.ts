// Dynamically-constructed builtin instances must behave like their literal
// counterparts. `new X()` with a LITERAL name is codegen-recognized; obtaining
// the constructor as a VALUE — `require("util").TextEncoder`, `const P =
// Promise`, a property read off a namespace — routes through the runtime's
// dynamic construct + dynamic method dispatch, where three gaps lived:
//
//  1. TextEncoder instances are a shared small-handle sentinel with no dynamic
//     method dispatch — `enc.encode(...)` / `enc.encodeInto(...)` returned
//     `undefined`. react-server-dom's flight byte-writer does exactly
//     `var T = new (require("util").TextEncoder)` and then
//     `T.encodeInto(row, scratch).read`, so the flight flush threw and every
//     Next.js App Router dynamic route hung (#5989).
//  2. TextDecoder handles had no dynamic `decode` dispatch.
//  3. `new P(executor)` where P holds the global Promise constructor value fell
//     through to the call path and threw "Constructor Promise requires 'new'".
//
// Validated byte-for-byte against `node --experimental-strip-types`.

import { TextEncoder as UtilTextEncoder, TextDecoder as UtilTextDecoder } from "util";

// (1) TextEncoder obtained as a value — the flight byte-writer shape.
const T: any = UtilTextEncoder;
const enc: any = new T();
const scratch = new Uint8Array(8);
const r: any = enc.encodeInto("hi", scratch);
console.log(r.read, r.written, scratch[0], scratch[1]);
const encoded: any = enc.encode("xyz");
console.log(encoded.length, encoded[0], encoded[2]);

// multi-byte + partial destination through the dynamic path
const small = new Uint8Array(3);
const rp: any = enc.encodeInto("héllo", small);
console.log(rp.read <= 3, rp.written <= 3);

// (2) TextDecoder obtained as a value.
const D: any = UtilTextDecoder;
const dec: any = new D();
console.log(dec.decode(new Uint8Array([104, 101, 106])));
console.log(JSON.stringify(dec.decode()));

// (3) Promise constructor obtained as a value (polyfill alias shape).
const P: any = Promise;
const p = new P((resolve: any) => resolve(42));
p.then((v: any) => console.log("resolved", v));

// executor receives working resolve/reject; rejection path too
const q = new P((_res: any, rej: any) => rej(new Error("nope")));
q.catch((e: any) => console.log("rejected", e.message));

// (4) Map/Set/WeakMap constructor values (regression guard for the arms that
// already worked — construction via value + native method dispatch).
const M: any = Map;
const m = new M([["k", 7]]);
m.set("j", 8);
console.log(m.get("k"), m.get("j"), m.size);
const S: any = Set;
const s = new S([1, 2]);
s.add(3);
console.log(s.has(2), s.has(3), s.size);
