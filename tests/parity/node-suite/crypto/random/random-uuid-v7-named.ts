// `randomUUIDv7` is a named export of `crypto` and `node:crypto` (Node 26), and the
// imported function returns a fresh RFC 9562 version-7 UUID.
import { randomUUIDv7 } from "crypto";
import { randomUUIDv7 as v7 } from "node:crypto";
const v = randomUUIDv7();
console.log(typeof v, /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(v));
console.log(/^[0-9a-f]{8}-[0-9a-f]{4}-7/.test(v7()), v7() !== v);
