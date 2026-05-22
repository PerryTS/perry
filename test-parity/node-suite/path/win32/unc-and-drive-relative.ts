import path from "node:path";

const w = path.win32;
for (const p of ["C:foo", "C:\\foo\\..\\bar", "\\\\server\\share\\dir\\..\\file", "//server/share/a/b"]) {
  console.log("normalize:", p, "=>", w.normalize(p));
}
console.log("resolve drive relative:", w.resolve("C:foo"));
console.log("relative drives:", w.relative("C:\\a\\b", "D:\\a\\b"));
