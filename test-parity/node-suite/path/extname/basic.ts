import * as path from "node:path";

console.log("ext txt:", path.extname("/foo/bar/baz.txt"));
console.log("ext gz:", path.extname("foo.tar.gz"));
console.log("ext trailing dot:", path.extname("foo."));
