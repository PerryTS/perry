import * as stream from "node:stream";
// stream.duplexPair() yields two paired Duplex streams (write side of one
// connects to read side of the other).
console.log("is function:", typeof (stream as any).duplexPair === "function");
