import { setTimeout } from "node:timers/promises";

const unref = setTimeout(1, { a: 1 }, { ref: false });
await setTimeout(5);

console.log("unref value:", JSON.stringify(await unref));
console.log("value undefined:", await setTimeout(1));
