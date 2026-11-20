import path from "node:path";

for (const p of ["", "foo", "/tmp/x", "C:\\foo", "\\\\server\\share\\file", "\\\\?\\C:\\already"]) {
  console.log("ns:", JSON.stringify(p), "=>", path.win32.toNamespacedPath(p));
}
console.log("posix:", path.posix.toNamespacedPath("/tmp/x"));
