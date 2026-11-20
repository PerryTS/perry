import * as path from "node:path";

for (const value of [".gitignore", "/home/user/.env", "/home/user/.profile.js"]) {
  const parsed = path.parse(value);
  console.log(value, "=>", parsed.dir, parsed.base, parsed.name, parsed.ext);
}
