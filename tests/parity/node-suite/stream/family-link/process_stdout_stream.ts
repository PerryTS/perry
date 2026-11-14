// process.stdout as a stream value: write, on, and a pipe into it.
// (`process.stdout instanceof Writable` is false in Perry, true in node, on
// main as well: not asserted here.)
import { Readable } from "node:stream";

process.stdout.write("direct write\n");
process.stdout.on("error", () => {});
console.log("has pipe target methods:", typeof process.stdout.write, typeof process.stdout.on);
const r = Readable.from(["piped 1\n", "piped 2\n"]);
r.pipe(process.stdout);
r.on("end", () => console.log("pipe ended"));
