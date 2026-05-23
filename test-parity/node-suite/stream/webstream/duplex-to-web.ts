import { Duplex } from "node:stream";
// Duplex.toWeb returns { readable, writable } — a pair of Web streams.
const d = new Duplex({ read() {}, write(_c, _e, cb) { cb(); } });
const pair = (Duplex as any).toWeb(d);
console.log("readable:", typeof pair.readable === "object" && typeof pair.readable.getReader === "function");
console.log("writable:", typeof pair.writable === "object" && typeof pair.writable.getWriter === "function");
