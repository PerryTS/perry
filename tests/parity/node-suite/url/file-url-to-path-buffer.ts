// url.fileURLToPathBuffer(url[, options]) — Buffer-returning counterpart to
// fileURLToPath (Node 22+). Regression cover for #2541.
import { fileURLToPath, fileURLToPathBuffer } from "node:url";

console.log("is function:", typeof fileURLToPathBuffer === "function");
const b = fileURLToPathBuffer(new URL("file:///tmp/a%20b"));
console.log("isBuffer:", Buffer.isBuffer(b));
console.log("value:", b.toString());
const u = "file:///home/user/file.txt";
console.log(
  "matches fileURLToPath:",
  fileURLToPathBuffer(u).equals(Buffer.from(fileURLToPath(u))),
);
console.log("spaces:", fileURLToPathBuffer(new URL("file:///a%20b/c")).toString());
