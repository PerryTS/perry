import { Buffer } from "node:buffer";

const b = Buffer.from([9, 8, 7]);

// .values(): yields each byte.
const vi = b.values();
console.log("values.next.value:", vi.next().value);
console.log("values.next.value:", vi.next().value);
console.log("values.next.value:", vi.next().value);
const done = vi.next();
console.log("values.done:", done.done, "value:", done.value);

// .keys(): yields each index.
const ki = b.keys();
console.log("keys.next.value:", ki.next().value);
console.log("keys.next.value:", ki.next().value);

// .entries(): yields [index, value] pairs.
const ei = b.entries();
const e0 = ei.next();
console.log("entries.next.value[0]:", e0.value[0], "value[1]:", e0.value[1]);
const e1 = ei.next();
console.log("entries.next.value[0]:", e1.value[0], "value[1]:", e1.value[1]);
