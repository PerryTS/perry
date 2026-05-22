import path from "node:path";

for (const p of ["", ".", "..", ".bashrc", "file.", "file..", "a.b/c", "a/b.c.d", "a/."]) {
  console.log("ext:", JSON.stringify(p), "=>", JSON.stringify(path.extname(p)));
}
