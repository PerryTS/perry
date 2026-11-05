import { resolve } from "node:url";

console.log("absolute path:", resolve("/a/b/c", "/d"));
console.log("relative url:", resolve("https://example.com/a/", "b/c"));
