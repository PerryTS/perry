import * as path from "node:path";

console.log("posix basename:", path.posix.basename("/foo/bar/baz.txt"));
console.log("posix basename ext:", path.posix.basename("/foo/bar/baz.txt", ".txt"));
console.log("posix dirname:", path.posix.dirname("/foo/bar/baz.txt"));
