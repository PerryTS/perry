import * as path from "node:path";

for (const value of ["/foo/bar/", "foo/bar/", "/foo/bar//"]) {
  const parsed = path.parse(value);
  console.log(value, "=>", parsed.root, parsed.dir, parsed.base, parsed.ext, parsed.name);
}
