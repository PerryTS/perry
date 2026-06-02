import * as path from "node:path";

// #1750: a path sub-namespace bound to a local must dispatch its methods
// exactly like the direct `path.win32.<m>(...)` / `path.posix.<m>(...)` form
// (previously segfaulted). Only deterministic, host-independent inputs here.
const w = path.win32;
console.log("w.normalize bare-drive:", w.normalize("C:"));
console.log("w.normalize trailing:", w.normalize(".\\"));
console.log("w.basename:", w.basename("C:\\foo\\bar\\baz.txt"));
console.log("w.basename unc:", w.basename("\\\\server\\share\\"));
console.log("w.dirname:", w.dirname("C:\\foo\\bar\\baz.txt"));
console.log("w.extname:", w.extname("C:\\foo\\bar.txt"));
console.log("w.isAbsolute:", w.isAbsolute("C:\\foo"));
console.log("w.join:", w.join("C:\\a", "b", "c"));
console.log("w.toNamespacedPath:", w.toNamespacedPath("C:\\foo"));

const p = path.posix;
console.log("p.normalize:", p.normalize("/a/./b/../c"));
console.log("p.basename:", p.basename("/a/b/c.txt"));
console.log("p.dirname:", p.dirname("/a/b/c.txt"));
console.log("p.extname:", p.extname("/a/b.txt"));
console.log("p.isAbsolute:", p.isAbsolute("/a"));
console.log("p.join:", p.join("/a", "b", "c"));

// direct forms must still work (regression guard for the refactor).
console.log("direct win32:", path.win32.normalize("C:\\x\\..\\y"));
console.log("direct posix:", path.posix.normalize("/x//y"));
