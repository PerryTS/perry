import path, { _makeLong, toNamespacedPath } from "node:path";

const top = path as any;
const posix = path.posix as any;
const win32 = path.win32 as any;

console.log("keys top:", Object.keys(path).includes("_makeLong"));
console.log("keys posix:", Object.keys(path.posix).includes("_makeLong"));
console.log("keys win32:", Object.keys(path.win32).includes("_makeLong"));
console.log("typeof top:", typeof top._makeLong);
console.log("named equality:", _makeLong === toNamespacedPath);
console.log("default equality:", top._makeLong === path.toNamespacedPath);
console.log("posix equality:", posix._makeLong === path.posix.toNamespacedPath);
console.log("win32 equality:", win32._makeLong === path.win32.toNamespacedPath);
console.log("named result:", _makeLong("/tmp/a"));
console.log("default result:", top._makeLong("/tmp/a"));
console.log("posix result:", posix._makeLong("/tmp/a"));
console.log("win32 result:", win32._makeLong("C:\\tmp\\a"));
