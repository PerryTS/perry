import { promises as streamP } from "node:stream";
// stream.promises is an object exposing pipeline/finished as Promise-returning
// helpers (used widely for `await pipeline(...)`).
console.log("typeof:", typeof streamP);
console.log("pipeline:", typeof streamP.pipeline === "function");
console.log("finished:", typeof streamP.finished === "function");
