import path from "node:path";

for (const p of ["/foo/bar//", "/foo/./bar/", "/foo/bar/..", "//server//share//"]) {
  console.log("posix:", p, "=>", path.posix.normalize(p));
}
for (const p of ["C:\\foo\\bar\\\\", "C:\\foo\\.\\bar\\", "C:\\foo\\bar\\..", "\\\\server\\share\\\\"]) {
  console.log("win32:", p, "=>", path.win32.normalize(p));
}
