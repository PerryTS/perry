// parse and format on both flavors, and format(parse(p)) for each input.
import * as path from "node:path";

for (const p of ["/home/u/file.txt", "./.env", "a/b.c.d", "/", "", "..", "dir/"]) {
  const parsed = path.posix.parse(p);
  console.log("posix", JSON.stringify(p), JSON.stringify(parsed), JSON.stringify(path.posix.format(parsed)));
}
for (const p of ["C:\\a\\b.txt", "C:\\", "\\\\srv\\share\\f.x", "C:rel.txt", "a\\.rc", "x/y.z"]) {
  const parsed = path.win32.parse(p);
  console.log("win32", JSON.stringify(p), JSON.stringify(parsed), JSON.stringify(path.win32.format(parsed)));
}
console.log(path.format({ root: "/", name: "n", ext: "e" }), path.format({ dir: "d", name: "n", ext: ".e" }));
console.log(path.win32.format({ root: "C:\\", base: "b" }), path.win32.format({ dir: "C:\\x", name: "y", ext: "js" }));
