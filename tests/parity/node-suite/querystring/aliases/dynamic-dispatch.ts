// #7720 follow-up: every INDIRECT form of a querystring method used to return
// `undefined`. `nm_dispatch_querystring` advertised the whole set
// (escape/unescape/stringify/encode/parse/decode) but the stdlib bridge it
// calls implemented only `unescapeBuffer`, so the statically-dispatched
// `qs.escape("a b")` was correct while a captured, type-erased or spread call
// of the same method silently produced `undefined`.
import qs from "node:querystring";

const dyn: any = qs;
const captured = qs.escape;
const args: [string] = ["a b"];

console.log("static:", qs.escape("a b"));
console.log("captured:", (captured as any)("a b"));
console.log("dynamic:", dyn.escape("a b"));
console.log("spread:", qs.escape(...args));

console.log("unescape dynamic:", dyn.unescape("a%20b"));
console.log("stringify dynamic:", dyn.stringify({ a: 1, b: "x y" }));
console.log("encode alias:", dyn.encode({ a: 1 }));
console.log("parse dynamic:", JSON.stringify(dyn.parse("a=1&b=2")));
console.log("decode alias:", JSON.stringify(dyn.decode("a=1")));
console.log("unknown method:", String(dyn.definitelyNotAMethod));
