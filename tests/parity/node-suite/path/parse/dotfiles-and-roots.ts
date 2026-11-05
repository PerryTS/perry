import path from "node:path";

for (const p of ["/.bashrc", "/", "/tmp/.", "/tmp/..", "C:\\foo\\bar.txt", "\\\\server\\share\\file"]) {
  const parsed = p.includes("\\") ? path.win32.parse(p) : path.parse(p);
  console.log("parse:", p, "=>", JSON.stringify(parsed));
}
