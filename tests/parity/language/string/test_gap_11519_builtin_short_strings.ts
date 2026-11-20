// #11519: builtins handed a SHORT (SSO, <= 5 bytes, built at runtime) string.
// Each of these read the argument through a heap-only accessor or a bare mask:
// `Array.from(s, fn)` came back empty, `Uint8Array.fromHex/fromBase64`, a
// `StringDecoder` encoding, an `EvalError` message and an async_hooks
// callback slot holding a string segfaulted, a `Temporal` time zone and an
// array-typed field holding a string at runtime misbehaved.
import { StringDecoder } from "node:string_decoder";
import { createHook } from "node:async_hooks";
const S = (s: string): string => s.charAt(0) + s.slice(1);

console.log("Array.from map:", Array.from(S("abc"), (c) => c + "!").join(), Array.from(`${123}`, Number).join());
const af = Array.from;
console.log("Array.from alias:", af(S("xyz")).join("|"), Array.from(S("ab")).length);
console.log("fromHex:", Array.from(Uint8Array.fromHex(S("4869"))).join(), Array.from(Uint8Array.fromHex(S(""))).length);
console.log("fromBase64:", Array.from(Uint8Array.fromBase64(S("SGk="))).join(), Array.from(Uint8Array.fromBase64(S("AQID"))).join());
const u = new Uint8Array(2);
console.log("setFromHex:", JSON.stringify(u.setFromHex(S("0aff"))), Array.from(u).join());

const d = new StringDecoder(S("utf8"));
console.log("StringDecoder:", JSON.stringify(d.write(Buffer.from("hé"))), JSON.stringify(d.end()), d.encoding);
console.log("StringDecoder hex:", new StringDecoder(S("hex")).write(Buffer.from([1, 255])));

for (const m of ["", "e", "12345"]) {
  const e = new EvalError(S(m));
  const u2 = new URIError(S(m));
  console.log(`Eval/URIError len ${m.length}:`, JSON.stringify(e.message), JSON.stringify(u2.message), String(e));
}

try {
  createHook({ init: S("x") } as any);
} catch (err: any) {
  console.log("createHook:", err.name, err.code);
}

console.log("Temporal tz:", typeof Temporal.Now.plainDateISO(S("UTC")).day, Temporal.Now.zonedDateTimeISO(S("UTC")).timeZoneId);

type Bag = { items: string[] };
const neg = (b: Bag) => "" + b.items[-1] + "|" + b.items[0.5] + "|" + b.items[1];
console.log("array-typed sso:", neg({ items: S("xy") as any }), neg({ items: ["p", "q"] }));
