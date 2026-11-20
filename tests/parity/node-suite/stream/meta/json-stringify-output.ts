import { Readable, Writable } from "node:stream";
// JSON.stringify(stream) — uses the default object serializer; the result
// is typically "{}" (no own enumerable props), or omitted (returns
// undefined) if the stream has custom toJSON returning undefined.
const r = new Readable({ read() {} });
const w = new Writable({ write(_c, _e, cb) { cb(); } });
const rJson = JSON.stringify(r);
const wJson = JSON.stringify(w);
console.log("R json:", rJson);
console.log("W json:", wJson);
console.log("R is string:", typeof rJson === "string");
console.log("W is string:", typeof wJson === "string");
