// #2514 — util.toUSVString: coerce to string, replace lone surrogates with U+FFFD.
import { toUSVString } from "node:util";

console.log(typeof toUSVString);
console.log(JSON.stringify(toUSVString("abc")));
console.log(JSON.stringify(toUSVString("a\uD800b"))); // lone high surrogate -> 1×U+FFFD
console.log(JSON.stringify(toUSVString("a\uDC00b"))); // lone low surrogate  -> 1×U+FFFD
console.log(toUSVString("a\uD800b").length); // 3 (one replacement unit)
console.log(toUSVString("a\uD800b").charCodeAt(1)); // 65533
console.log(JSON.stringify(toUSVString("a😀b"))); // valid pair preserved
console.log(JSON.stringify(toUSVString("café")));
console.log(JSON.stringify(toUSVString(123 as unknown as string))); // -> "123"
console.log(JSON.stringify(toUSVString(undefined as unknown as string))); // -> "undefined"
console.log(JSON.stringify(toUSVString(null as unknown as string))); // -> "null"
