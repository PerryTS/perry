import * as path from "node:path";

console.log("root only:", path.format({ root: "/" }));
console.log("dir slash base:", path.format({ dir: "/", base: "file" }));
console.log("relative dir base:", path.format({ dir: "foo", base: "bar" }));
console.log("dir trailing slash:", path.format({ dir: "/foo/", base: "bar" }));
