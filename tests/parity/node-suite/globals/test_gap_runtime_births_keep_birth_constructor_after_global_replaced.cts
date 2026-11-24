// An object the runtime builds itself (a web stream, a MessageChannel and its
// ports, a BroadcastChannel, a boxed primitive, a CryptoKey, a child_process
// error) is born with the realm's original constructor and prototype.
// Replacing the global binding first (`globalThis.MessagePort = X`) changes
// neither: node never reads the global object to build them.
const g: any = globalThis;
const names = [
  "TextEncoderStream", "TextDecoderStream", "CompressionStream", "DecompressionStream",
  "MessageChannel", "MessagePort", "BroadcastChannel",
  "Number", "String", "Boolean", "Symbol", "BigInt", "CryptoKey", "Error", "RangeError",
];
const saved: any = {};
for (const n of names) saved[n] = g[n];
const toObject = Object;
const getProto = Object.getPrototypeOf;
const wt = require("worker_threads");
const cp = require("child_process");
const subtle = require("crypto").webcrypto.subtle;
for (const n of names) g[n] = class Replaced {};

function row(label: string, value: any, ctor: any) {
  console.log(label, value.constructor === ctor, getProto(value) === ctor.prototype);
}
// node builds these through an internal subclass: only `constructor` is checked.
function ctorRow(label: string, value: any, ctor: any) {
  console.log(label, value.constructor === ctor);
}
row("TextEncoderStream", new saved.TextEncoderStream(), saved.TextEncoderStream);
row("TextDecoderStream", new saved.TextDecoderStream(), saved.TextDecoderStream);
row("CompressionStream", new saved.CompressionStream("gzip"), saved.CompressionStream);
row("DecompressionStream", new saved.DecompressionStream("gzip"), saved.DecompressionStream);
const ch = new saved.MessageChannel();
row("MessageChannel", ch, saved.MessageChannel);
row("MessageChannel.port1", ch.port1, saved.MessagePort);
ch.port1.close();
const wch = new wt.MessageChannel();
row("worker_threads.MessageChannel", wch, saved.MessageChannel);
row("worker_threads.port2", wch.port2, saved.MessagePort);
wch.port1.close();
const bc = new saved.BroadcastChannel("rtread");
row("BroadcastChannel", bc, saved.BroadcastChannel);
bc.close();
row("boxed number", toObject(1), saved.Number);
row("boxed string", toObject("s"), saved.String);
row("boxed boolean", toObject(true), saved.Boolean);
row("boxed symbol", toObject(saved.Symbol.iterator), saved.Symbol);
row("boxed bigint", toObject(1n), saved.BigInt);
const missing = cp.spawnSync("/nonexistent/rtread-binary");
ctorRow("spawnSync ENOENT error", missing.error, saved.Error);
cp.exec("echo hello", { maxBuffer: 1 }, (err: any) => ctorRow("exec maxBuffer error", err, saved.RangeError));
subtle
  .generateKey({ name: "HMAC", hash: "SHA-256" }, true, ["sign"])
  .then((key: any) => ctorRow("CryptoKey", key, saved.CryptoKey));
