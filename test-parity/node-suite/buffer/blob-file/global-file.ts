import { File as BufferFile } from "node:buffer";

console.log("typeof File:", typeof File);
console.log("File.name:", File.name);
console.log("typeof globalThis.File:", typeof globalThis.File);
console.log("global File identity:", globalThis.File === File);
console.log("buffer File identity:", BufferFile === File, BufferFile === globalThis.File);

const direct = new globalThis.File(["abc"], "x.txt", {
  type: "text/plain",
  lastModified: 123,
});
console.log("direct fields:", direct.name, direct.type, direct.size, direct.lastModified);
console.log("direct text:", await direct.text());

const FileCtor = globalThis.File;
const rebound = new FileCtor(["xy"], "y.txt", {
  type: "text/custom",
  lastModified: 456,
});
console.log("rebound fields:", rebound.name, rebound.type, rebound.size, rebound.lastModified);
console.log("rebound text:", await rebound.text());
