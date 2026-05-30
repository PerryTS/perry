import path from "node:path";
import * as posixNs from "node:path/posix";
import * as win32Ns from "node:path/win32";
import posixDefault from "node:path/posix";
import win32Default from "node:path/win32";

console.log("posix default identity:", path.posix === posixDefault);
console.log("win32 default identity:", path.win32 === win32Default);
console.log("posix join:", posixNs.join("/a", "b"));
console.log("win32 basename:", win32Ns.basename("C:\\a\\b.txt"));
console.log("posix sep delimiter:", posixNs.sep, posixNs.delimiter);
console.log("win32 sep delimiter:", win32Ns.sep, win32Ns.delimiter);
console.log(
  "submodule alias identities:",
  posixNs.posix === posixDefault,
  posixNs.win32 === win32Default,
  win32Ns.posix === posixDefault,
  win32Ns.win32 === win32Default,
);
console.log("default methods:", posixDefault.normalize("/a//b"), win32Default.join("C:\\a", "b"));
