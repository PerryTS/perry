import path from "node:path";

for (const [p, suffix] of [["/a/b.txt", ".txt"], ["/a/b.txt", "txt"], ["/a/b.txt", ".md"], ["/a/.bashrc", ".bashrc"], ["/a/b/", ""]] as any[]) {
  console.log("base:", p, suffix, "=>", path.basename(p, suffix));
}
