import streamPromises from "node:stream/promises";
import * as streamPromisesNs from "node:stream/promises";
// node:stream/promises exposes Node's CommonJS-style default namespace as
// well as named Promise helpers.
console.log("default pipeline:", typeof streamPromises.pipeline === "function");
console.log("default finished:", typeof streamPromises.finished === "function");
console.log("namespace default:", typeof streamPromisesNs.default.pipeline === "function");
