import { pathToFileURL } from "node:url";

for (const p of ["relative/path", "./relative/../target #?.txt", "relative/", ""]) {
  console.log("pathToFileURL:", JSON.stringify(p), "=>", pathToFileURL(p).href);
}
