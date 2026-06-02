import * as path from "node:path";

console.log("absolute second not reset:", path.join("/foo", "/bar"));
console.log("absolute third not reset:", path.join("foo", "/bar", "/baz"));
console.log("root plus root:", path.join("/", "/"));
