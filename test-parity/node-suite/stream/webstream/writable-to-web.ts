import { Writable } from "node:stream";
// Writable.toWeb(node-writable) converts to a WHATWG WritableStream.
const w = new Writable({ write(_c, _e, cb) { cb(); } });
const web = (Writable as any).toWeb(w);
console.log("is WritableStream:", typeof web === "object" && typeof web.getWriter === "function");
