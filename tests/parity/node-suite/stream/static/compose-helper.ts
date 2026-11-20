import * as stream from "node:stream";
// stream.compose(...) chains streams into one composite Duplex.
console.log("is function:", typeof (stream as any).compose === "function");
