import path from "node:path";

for (const p of ["C:\\foo\\bar", "C:/foo/bar", "\\\\server\\share", "a\\b\\c"]) {
  console.log("posix basename:", p, "=>", path.posix.basename(p));
  console.log("posix dirname:", p, "=>", path.posix.dirname(p));
}
