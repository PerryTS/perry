// #3938: literal dynamic import of node:* submodules whose names contain "/"
// resolves at compile time without emitting invalid LLVM.
const utilTypes = await import("node:util/types");
const pathPosix = await import("node:path/posix");
console.log("util/types", typeof utilTypes.isMap, typeof utilTypes.isDataView);
console.log("path/posix", typeof pathPosix.join, pathPosix.join("a", "b"));
