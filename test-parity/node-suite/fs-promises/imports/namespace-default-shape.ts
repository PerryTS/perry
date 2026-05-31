import * as fs from "node:fs";
import fspDefault, { constants, readFile } from "node:fs/promises";
import * as fsp from "node:fs/promises";

const expected = [
  "access",
  "copyFile",
  "cp",
  "glob",
  "open",
  "opendir",
  "readFile",
  "readdir",
  "rm",
  "stat",
  "writeFile",
] as const;

console.log("namespace default type:", typeof fsp.default);
console.log("default import identity:", fspDefault === fsp.default);
console.log("default is namespace:", fspDefault === fsp);
console.log(
  "default has own default:",
  Object.prototype.hasOwnProperty.call(fspDefault, "default"),
);
console.log("constants type:", typeof fsp.constants);
console.log("constants fs identity:", fsp.constants === fs.constants);
console.log("constants named identity:", constants === fsp.constants);

for (const name of expected) {
  console.log(
    `${name}:`,
    typeof fsp[name],
    typeof fspDefault[name],
    fsp[name] === fspDefault[name],
  );
}

console.log("named readFile identity:", readFile === fsp.readFile, readFile === fspDefault.readFile);
