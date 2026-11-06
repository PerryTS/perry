import * as os from "node:os";

const release = os.release();
const version = os.version();

console.log("release string:", typeof release === "string" && release.length > 0);
console.log("version string:", typeof version === "string" && version.length > 0);
console.log("release equals version:", release === version);

if (os.platform() === "linux") {
  console.log("linux version starts hash:", version.startsWith("#"));
  console.log("linux version includes release:", version.includes(release));
} else if (os.platform() === "darwin") {
  console.log("darwin version includes kernel label:", version.includes("Darwin Kernel Version"));
}
