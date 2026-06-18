// Regressions for the "read of undefined" cluster surfaced by native-compiling
// bluebird / execa / winston (branch fix/read-undefined-bluebird-winston).
//
// 1. Promise `.constructor` is the global `Promise` — execa's
//    `(async () => {})().constructor.prototype` captured the native promise
//    prototype; pre-fix `.constructor` read `undefined` and `.prototype` threw
//    `Cannot read properties of undefined (reading 'prototype')`.
//
// 2. An accessor installed via `Object.defineProperty(Class.prototype, name,
//    { get })` must fire when the property is read on an INSTANCE (not only via
//    `Class.prototype.name`). winston's `transports` getter
//    (`Object.defineProperty(Logger.prototype, 'transports', { get })`, read as
//    `this.transports`) returned `undefined` and `.length` threw. Covered for a
//    plain class AND a class that `extends` a native node:stream base (the
//    latter previously stack-overflowed once the getter was found, because the
//    decl-prototype object carries the instance class_id and its data-field
//    walk re-entered the same proto-chain resolution).

import { Transform } from "node:stream";

// --- 1. Promise.constructor ---
const p0 = Promise.resolve(1);
console.log("Promise.resolve().constructor === Promise:", p0.constructor === Promise);
const p1 = (async () => {})();
console.log("async-arrow promise .constructor typeof:", typeof p1.constructor);
console.log("async-arrow promise .constructor.prototype typeof:", typeof p1.constructor.prototype);
console.log("native promise then is fn:", typeof p1.constructor.prototype.then === "function");

// --- 2a. defineProperty getter on a plain class prototype, read on instance ---
class Plain {}
Object.defineProperty(Plain.prototype, "tag", {
  configurable: true,
  enumerable: true,
  get() {
    return "plain-getter";
  },
});
const plain = new Plain();
console.log("plain instance getter:", (plain as any).tag);
console.log("plain computed read:", (plain as any)["tag"]);

// --- 2b. defineProperty getter on a native-stream subclass, read on instance ---
class Logger extends Transform {
  constructor() {
    super({ objectMode: true });
  }
  readTransports() {
    return (this as any).transports.length;
  }
}
Object.defineProperty(Logger.prototype, "transports", {
  configurable: false,
  enumerable: true,
  get() {
    const pipes = (this as any)._readableState ? (this as any)._readableState.pipes : undefined;
    return !Array.isArray(pipes) ? [pipes].filter(Boolean) : pipes;
  },
});
const logger = new Logger();
console.log("stream subclass getter is array:", Array.isArray((logger as any).transports));
console.log("stream subclass getter .length via method:", logger.readTransports());
