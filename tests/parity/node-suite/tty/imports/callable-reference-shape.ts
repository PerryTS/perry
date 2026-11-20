import { isatty, ReadStream, WriteStream } from "node:tty";

console.log("named isatty callable:", typeof isatty === "function");
console.log("named ReadStream callable:", typeof ReadStream === "function");
console.log("named WriteStream callable:", typeof WriteStream === "function");
console.log("named constructor names:", ReadStream.name === "ReadStream" && WriteStream.name === "WriteStream");
// Calling the captured `isatty` reference is intentionally not asserted here:
// Perry currently preserves the callable shape but direct captured dispatch is
// tracked separately from namespace method dispatch.
