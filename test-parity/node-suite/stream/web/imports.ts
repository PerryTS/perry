import {
  ReadableStream,
  WritableStream,
  TransformStream,
  ByteLengthQueuingStrategy,
  CountQueuingStrategy,
} from "node:stream/web";
// node:stream/web exposes the WHATWG Web Streams API.
console.log("ReadableStream:", typeof ReadableStream === "function");
console.log("WritableStream:", typeof WritableStream === "function");
console.log("TransformStream:", typeof TransformStream === "function");
console.log("ByteLengthQS:", typeof ByteLengthQueuingStrategy === "function");
console.log("CountQS:", typeof CountQueuingStrategy === "function");
