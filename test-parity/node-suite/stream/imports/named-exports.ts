import { Readable, Writable, Duplex, Transform, PassThrough, pipeline, finished } from "node:stream";
// Named imports of the core constructors and helper functions all resolve.
console.log("Readable:", typeof Readable === "function");
console.log("Writable:", typeof Writable === "function");
console.log("Duplex:", typeof Duplex === "function");
console.log("Transform:", typeof Transform === "function");
console.log("PassThrough:", typeof PassThrough === "function");
console.log("pipeline:", typeof pipeline === "function");
console.log("finished:", typeof finished === "function");
