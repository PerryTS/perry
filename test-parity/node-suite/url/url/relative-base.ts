console.log("path:", new URL("/path", "https://example.com/base").href);
console.log("relative:", new URL("b/c", "https://example.com/a/").href);
console.log("dotdot:", new URL("../up", "https://example.com/a/b/").href);
