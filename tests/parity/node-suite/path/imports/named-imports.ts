import { basename, dirname, extname, join, normalize, resolve } from "node:path";

console.log("basename:", basename("/foo/bar.txt"));
console.log("dirname:", dirname("/foo/bar.txt"));
console.log("extname:", extname("/foo/bar.txt"));
console.log("join:", join("/foo", "bar"));
console.log("normalize:", normalize("/foo//bar"));
console.log("resolve suffix:", resolve("foo").endsWith("foo"));
