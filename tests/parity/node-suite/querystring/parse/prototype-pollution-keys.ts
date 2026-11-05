// #1175: parse must store `__proto__` / `constructor` / `toString` / etc.
// as own keys on the result object (Node uses Object.create(null) for the
// same outcome). Without this, Perry's pre-fix `js_object_get_field_by_name`
// duplicate-detection walked the prototype chain and dropped `constructor`
// silently (Object.keys missed it) while leaving the inherited Function
// visible as `r.constructor`.
import { parse } from "node:querystring";

const r = parse("a=1&__proto__=evil&constructor=ctor&toString=ts&valueOf=vo") as any;

console.log("a:", r["a"]);
console.log("__proto__:", r["__proto__"]);
console.log("constructor:", r["constructor"]);
console.log("toString:", r["toString"]);
console.log("valueOf:", r["valueOf"]);
console.log("keys:", Object.keys(r).join(","));

// Prototype pollution defense: other objects must not see `evil`.
const o = {} as any;
console.log("pollution:", typeof o["evil"]);
