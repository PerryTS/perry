// process.stdout as a stream value: write, on, a pipe into it, and its
// prototype chain reaching stream.Writable.
import { Readable, Writable } from "node:stream";

process.stdout.write("direct write\n");
process.stdout.on("error", () => {});
console.log("stdout is Writable:", process.stdout instanceof Writable);
console.log("has pipe target methods:", typeof process.stdout.write, typeof process.stdout.on);
const r = Readable.from(["piped 1\n", "piped 2\n"]);
r.pipe(process.stdout);
r.on("end", () => console.log("pipe ended"));
