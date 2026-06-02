import Stream, * as nsStream from "node:stream";
// The default export and the namespace import both resolve to the same
// stream module; the default has Readable/Writable hanging off it.
console.log("default is function:", typeof Stream === "function");
console.log("namespace is object:", typeof nsStream === "object");
console.log("namespace Readable:", typeof nsStream.Readable === "function");
