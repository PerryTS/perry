import * as stream from "node:stream";
// stream.Stream is the legacy base class (extends EventEmitter); modern
// classes hang off it as statics.
const StreamCtor = (stream as any).Stream;
console.log("Stream is function:", typeof StreamCtor === "function");
console.log("Stream === default:", StreamCtor === (stream as any).default);
