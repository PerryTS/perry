import * as path from "node:path";

for (const value of ["/", "foo/bar.txt", "foo", ""]) {
  const parsed = path.parse(value);
  console.log(value || "<empty>", "=>", parsed.root, parsed.dir, parsed.base, parsed.name, parsed.ext);
}
