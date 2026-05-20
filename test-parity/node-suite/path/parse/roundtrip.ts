import * as path from "node:path";

for (const value of ["/home/user/file.txt", "foo/bar", ".gitignore", "/"]) {
  const parsed = path.parse(value);
  console.log(value, "=>", path.format(parsed));
}
