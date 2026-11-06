import { PassThrough, Transform, Duplex, Readable, Writable } from "node:stream";
import { EventEmitter } from "node:events";
// PassThrough extends Transform extends Duplex; Duplex inherits from
// Readable and Writable (mixin-style); all eventually extend EventEmitter.
const p = new PassThrough();
console.log("instance PassThrough:", p instanceof PassThrough);
console.log("instance Transform:", p instanceof Transform);
console.log("instance Duplex:", p instanceof Duplex);
console.log("instance Readable:", p instanceof Readable);
console.log("instance Writable:", p instanceof Writable);
console.log("instance EventEmitter:", p instanceof EventEmitter);
