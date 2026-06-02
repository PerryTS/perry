import * as stream from "node:stream";
import { EventEmitter } from "node:events";
// The legacy Stream class (default export of node:stream) inherits from EE.
const Stream: any = (stream as any).default || (stream as any).Stream;
console.log("Stream defined:", typeof Stream === "function");
console.log("prototype chain has EE:", Stream.prototype instanceof EventEmitter);
