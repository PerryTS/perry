import path from "node:path";

console.log("base wins:", path.format({ dir: "/tmp", name: "name", ext: ".txt", base: "base.md" }));
console.log("root used:", path.format({ root: "/", name: "x", ext: ".js" }));
console.log("dot ext:", path.format({ dir: "/a", name: "b", ext: "txt" }));
console.log("win base wins:", path.win32.format({ dir: "C:\\x", name: "a", ext: ".b", base: "c.d" }));
