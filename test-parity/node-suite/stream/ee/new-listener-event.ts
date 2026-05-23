import { PassThrough } from "node:stream";
// Adding a listener emits 'newListener'; removing emits 'removeListener'.
const p = new PassThrough();
let added = false;
let removed = false;
p.on("newListener", (name) => { if (name === "data") added = true; });
p.on("removeListener", (name) => { if (name === "data") removed = true; });
const fn = () => {};
p.on("data", fn);
p.removeListener("data", fn);
console.log("added:", added);
console.log("removed:", removed);
