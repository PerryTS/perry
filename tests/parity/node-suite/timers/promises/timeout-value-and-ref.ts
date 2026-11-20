import { setTimeout } from "node:timers/promises";

console.log("value object:", JSON.stringify(await setTimeout(1, { a: 1 }, { ref: false })));
console.log("value undefined:", await setTimeout(1));
