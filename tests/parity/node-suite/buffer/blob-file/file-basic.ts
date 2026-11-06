import { File } from "node:buffer";

const f = new File(["data"], "hello.txt", { type: "text/plain", lastModified: 1700000000000 });
console.log("name:", f.name);
console.log("size:", f.size);
console.log("type:", f.type);
console.log("lastModified:", f.lastModified);

const text = await f.text();
console.log("text:", text);
