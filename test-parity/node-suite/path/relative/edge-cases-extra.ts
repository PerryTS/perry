import path from "node:path";

console.log("same:", JSON.stringify(path.relative("/a/b", "/a/b")));
console.log("parent:", path.relative("/a/b/c", "/a/d"));
console.log("win same drive:", path.win32.relative("C:\\a\\b", "C:\\a\\c"));
console.log("win unc:", path.win32.relative("\\\\s\\share\\a", "\\\\s\\share\\b"));
