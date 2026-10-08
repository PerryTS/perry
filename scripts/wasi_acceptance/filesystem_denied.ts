import * as fs from "node:fs";
try {
  fs.writeFileSync("denied.txt", "must require a preopen");
  console.log("unexpected write");
} catch (error: any) {
  console.log("denied", error.code === "EACCES" || error.code === "EPERM" || error.code === "ENOENT");
}
