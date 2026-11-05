import path from "node:path";

for (const p of ["/foo/bar", "/foo/bar/", "foo/bar/baz", "//server/share"]) {
  console.log("win basename:", p, "=>", path.win32.basename(p));
  console.log("win dirname:", p, "=>", path.win32.dirname(p));
}
