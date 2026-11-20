import * as path from "node:path";

// #1740: dynamic sub-namespace METHOD dispatch — `path[k].method(...)` in the
// direct chained `ns[dynamicKey].staticMember` shape that #1723 allowed past
// the #503 lockdown guard (`k` is a variable; the method name is plaintext, so
// nothing is hidden). `path[k]` resolves to a runtime sub-namespace object;
// property reads (`sep`/`delimiter`) already worked, but method calls returned
// `undefined` until the runtime dispatch table learned `path.win32` /
// `path.posix`. Surfaced by the #800 node-core radar (`test-path-glob.js`).
//
// (`.normalize`/`.format` — the prototype-name-colliding methods that #1760
// fixed — are exercised in the companion `normalize-format.ts`. `.resolve` is
// still omitted here because it is cwd-dependent.)
const w = "win32";
console.log("win32 sep:", (path as any)[w].sep, "delim:", (path as any)[w].delimiter);
console.log("win32 join:", (path as any)[w].join("a", "b"));
console.log("win32 basename:", (path as any)[w].basename("C:\\x\\y.txt"));
console.log("win32 dirname:", (path as any)[w].dirname("C:\\x\\y.txt"));
console.log("win32 extname:", (path as any)[w].extname("y.txt"));
console.log("win32 isAbsolute:", (path as any)[w].isAbsolute("C:\\x"));
console.log("win32 matchesGlob:", (path as any)[w].matchesGlob("foo\\bar\\baz", "foo\\[bcr]ar\\baz"));

const po = "posix";
console.log("posix sep:", (path as any)[po].sep, "delim:", (path as any)[po].delimiter);
console.log("posix join:", (path as any)[po].join("a", "b"));
console.log("posix basename:", (path as any)[po].basename("/x/y.txt"));
console.log("posix dirname:", (path as any)[po].dirname("/x/y.txt"));
console.log("posix isAbsolute:", (path as any)[po].isAbsolute("/x"));
console.log("posix matchesGlob:", (path as any)[po].matchesGlob("foo/bar/baz", "foo/[bcr]ar/baz"));
