// A core-module export that shares its value with a global (`buffer.Blob`,
// `stream/web.ReadableStream`, `url.URL`, `util.TextEncoder`, ...) is the
// realm's original constructor. Replacing the global binding first
// (`globalThis.Blob = X`) changes neither `require("buffer").Blob` nor a
// destructured export: node never reads the global object for them.
const g: any = globalThis;
const pairs: [string, string][] = [
  ["buffer", "Blob"],
  ["buffer", "File"],
  ["stream/web", "ReadableStream"],
  ["stream/web", "WritableStream"],
  ["stream/web", "TransformStream"],
  ["stream/web", "TextEncoderStream"],
  ["stream/web", "TextDecoderStream"],
  ["stream/web", "CompressionStream"],
  ["stream/web", "DecompressionStream"],
  ["url", "URL"],
  ["url", "URLSearchParams"],
  ["url", "URLPattern"],
  ["util", "TextEncoder"],
  ["util", "TextDecoder"],
  ["worker_threads", "MessageChannel"],
  ["worker_threads", "MessagePort"],
];
const extra: [string, string][] = [
  ["crypto.webcrypto.constructor", "Crypto"],
  ["crypto.subtle.constructor", "SubtleCrypto"],
  ["repl.Recoverable parent", "SyntaxError"],
  ["assert.AssertionError parent", "Error"],
];
const originals = new Map<string, any>();
for (const [, name] of [...pairs, ...extra]) {
  originals.set(name, g[name]);
  g[name] = class Replacement {};
}
const { Blob, File } = require("buffer");
const { ReadableStream } = require("node:stream/web");
const { URL, URLSearchParams } = require("url");
const { TextEncoder, TextDecoder } = require("node:util");
const destructured: Record<string, any> = { Blob, File, ReadableStream, URL, URLSearchParams, TextEncoder, TextDecoder };
for (const [mod, name] of pairs) {
  const ns = require(mod);
  const member = ns[name];
  const row = [
    mod + "." + name,
    typeof member,
    member === originals.get(name),
    member === g[name],
    name in destructured ? destructured[name] === originals.get(name) : "-",
  ];
  console.log(row.join(" "));
}
const crypto = require("crypto");
const extraValues: any[] = [
  crypto.webcrypto.constructor,
  crypto.subtle.constructor,
  Object.getPrototypeOf(require("repl").Recoverable),
  Object.getPrototypeOf(require("assert").AssertionError.prototype) === originals.get("Error").prototype
    ? originals.get("Error")
    : "other",
];
extra.forEach(([what, name], i) => console.log(what, extraValues[i] === originals.get(name)));
for (const [, name] of [...pairs, ...extra]) g[name] = originals.get(name);
// node loads http.WebSocket (undici) on first use, which itself needs the
// original TextDecoder & co., so WebSocket is replaced on its own.
const ws = g.WebSocket;
g.WebSocket = class Replacement {};
console.log("http.WebSocket", require("http").WebSocket === ws);
g.WebSocket = ws;
console.log("restored", g.Blob === require("buffer").Blob, g.URL === require("url").URL);
