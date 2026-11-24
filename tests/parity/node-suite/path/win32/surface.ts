// Every path.win32 function on drive, UNC and relative inputs whose answer does not
// depend on the working directory.
import * as path from "node:path";

console.log("sep delimiter:", JSON.stringify([path.win32.sep, path.win32.delimiter]));
console.log("join:", path.win32.join("C:\\a", "b", "..\\c"), path.win32.join("\\\\srv\\share", "x"), path.win32.join("a", "", "b/"));
console.log("normalize:", path.win32.normalize("C:\\a\\..\\b"), path.win32.normalize("\\\\server\\share\\x\\..\\y"), path.win32.normalize("C:"), path.win32.normalize("a/b\\..\\c"));
console.log("dirname:", path.win32.dirname("C:\\a\\b.txt"), path.win32.dirname("C:\\"), path.win32.dirname("\\\\srv\\share\\f"));
console.log("basename:", path.win32.basename("C:\\a\\b.txt"), path.win32.basename("C:\\a\\b.txt", ".txt"), path.win32.basename("C:\\a\\"));
console.log("extname:", path.win32.extname("C:\\a\\b.txt"), JSON.stringify(path.win32.extname("C:\\a\\.rc")));
console.log("isAbsolute:", path.win32.isAbsolute("C:\\a"), path.win32.isAbsolute("C:a"), path.win32.isAbsolute("\\\\srv\\share"), path.win32.isAbsolute("a"));
console.log("relative:", path.win32.relative("C:\\a\\b", "C:\\a\\c"), path.win32.relative("C:\\A\\b", "c:\\a\\B\\d"), path.win32.relative("C:\\a", "D:\\b"));
console.log("resolve:", path.win32.resolve("C:\\a"), path.win32.resolve("C:\\a", "b", "..\\c"), path.win32.resolve("C:\\a", "D:\\b"), path.win32.resolve("\\\\srv\\share", "x"));
console.log("toNamespacedPath:", path.win32.toNamespacedPath("C:\\a"), path.win32.toNamespacedPath("\\\\srv\\share\\f"), path.win32.toNamespacedPath(""));
