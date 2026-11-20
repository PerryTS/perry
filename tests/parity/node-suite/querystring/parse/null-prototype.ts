// #1175: querystring.parse must return a null-prototype object so
// Object.getPrototypeOf(result) === null and keys like `__proto__` /
// `constructor` are stored as own data properties rather than punching
// through Object.prototype. This mirrors Node's defense against
// prototype-pollution payloads (Node v6+).
import { parse } from "node:querystring";

const r = parse("a=1&__proto__=evil&constructor=ctor") as any;
console.log("proto:", Object.getPrototypeOf(r));
console.log("__proto__:", r["__proto__"]);
console.log("constructor:", r["constructor"]);

// Plain object literals still have Object.prototype, and the `evil`
// payload must not have polluted other objects.
const o: any = {};
console.log("plain proto null?", Object.getPrototypeOf(o) === null);
console.log("pollution:", typeof o["evil"]);

// Object.assign / spread copy own keys onto a fresh `{}` so the result
// has Object.prototype again — the null-proto flag must not propagate.
const copied = Object.assign({}, r);
console.log("copied proto null?", Object.getPrototypeOf(copied) === null);
console.log("copied a:", copied.a);

const spread = { ...r };
console.log("spread proto null?", Object.getPrototypeOf(spread) === null);
console.log("spread a:", spread.a);

// JSON.stringify walks own enumerable keys — should serialize fine even
// without an Object.prototype chain.
console.log("json:", JSON.stringify(r));

// Object.keys / own-key iteration still works on a null-proto.
console.log("keys:", Object.keys(r).sort().join(","));
