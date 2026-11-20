import Stream from "node:stream";
// `require('stream')` / default import returns the legacy `Stream`
// constructor (which extends EventEmitter); class statics like
// `Stream.Readable` hang off it.
console.log("typeof Stream:", typeof Stream);
console.log("has Readable:", typeof Stream.Readable === "function");
console.log("has Writable:", typeof Stream.Writable === "function");
