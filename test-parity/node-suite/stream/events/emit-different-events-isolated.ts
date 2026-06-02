import { Readable } from "node:stream";
// Listeners for different events don't interfere.
const r = new Readable({ read() {} });
let a = 0, b = 0;
r.on("eventA", () => a++);
r.on("eventB", () => b++);
r.emit("eventA");
r.emit("eventB");
r.emit("eventA");
console.log("a:", a, "b:", b);
console.log("isolated:", a === 2 && b === 1);
