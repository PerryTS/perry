import * as path from "node:path";

for (const value of [".", "..", "...", "foo..", "foo...", "foo.bar.", "foo..bar", "/foo/.bar.baz"]) {
  console.log(value, "=>", path.extname(value));
}
