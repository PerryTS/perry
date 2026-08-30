// graceful-fs's polyfills run at module init in every esbuild-bundled CLI
// (pi, and anything else that pulls graceful-fs):
//
//   var chdir = process.chdir;
//   process.chdir = function (d) { ... };
//   if (Object.setPrototypeOf) Object.setPrototypeOf(process.chdir, chdir);
//
// A write to a builtin namespace member must win every subsequent read.
// Perry's name-keyed read entries handed back the canonical BOUND_METHOD
// closure regardless of the store, so the setPrototypeOf received the SAME
// closure twice and pi's boot died on the (correct) "Cyclic __proto__ value"
// self-set rejection.
import fs from "node:fs";

const chdir = process.chdir;
process.chdir = function (d: string) { chdir.call(process, d); };
console.log("process.chdir patched:", process.chdir !== chdir);
if (Object.setPrototypeOf) Object.setPrototypeOf(process.chdir, chdir);
console.log("process.chdir proto:", Object.getPrototypeOf(process.chdir) === chdir);

const fs$rename = fs.rename;
fs.rename = function rename(a: any, b: any, cb: any) { return fs$rename(a, b, cb); } as any;
console.log("fs.rename patched:", fs.rename !== fs$rename);
if (Object.setPrototypeOf) Object.setPrototypeOf(fs.rename, fs$rename);
console.log("fs.rename proto:", Object.getPrototypeOf(fs.rename) === fs$rename);

const fs$read = fs.read;
fs.read = function read(fd: any, buffer: any, offset: any, length: any, position: any, cb: any) {
  return (fs$read as any)(fd, buffer, offset, length, position, cb);
} as any;
console.log("fs.read patched:", fs.read !== fs$read);
if (Object.setPrototypeOf) Object.setPrototypeOf(fs.read, fs$read);
console.log("fs.read proto:", Object.getPrototypeOf(fs.read) === fs$read);
console.log("done");
