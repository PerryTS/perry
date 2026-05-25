import * as path from "node:path";

// #1760: prototype-name-colliding methods on a dynamic sub-namespace receiver.
// `(path as any)[k].normalize(...)` resolves `path[k]` to a runtime
// sub-namespace object (`path.win32` / `path.posix`), so the `.normalize`
// call must route through the generic native-module dispatch. Codegen used
// to classify `normalize` as a `String.prototype` method purely by name and
// emit a string-method lowering — handing the namespace pointer to a string
// FFI and SIGSEGV-ing. This is the companion to #1740's method-dispatch.ts,
// which deliberately omitted these names while the codegen gap was open.
const w = "win32";
console.log("win32 normalize:", (path as any)[w].normalize("a\\..\\b"));
console.log("win32 normalize2:", (path as any)[w].normalize("C:\\foo\\..\\bar\\.\\baz"));
console.log("win32 format:", (path as any)[w].format({ dir: "C:\\a", base: "b.txt" }));

const po = "posix";
console.log("posix normalize:", (path as any)[po].normalize("a/./b/../c"));
console.log("posix normalize2:", (path as any)[po].normalize("/foo/bar//baz/../qux"));
console.log("posix format:", (path as any)[po].format({ dir: "/a", base: "b.txt" }));
