import { readdir, stat } from "node:fs";
import { fileURLToPath } from "node:url";

// The callbacks below complete only in the event loop's poll phase.
const here = fileURLToPath(new URL(".", import.meta.url));
console.log("m1");
const names = await new Promise<string[]>((resolve, reject) =>
  readdir(here, (error, entries) => (error ? reject(error) : resolve(entries.sort()))),
);
console.log("m2", names.join(","));
const info = await new Promise<boolean>((resolve) => stat(here, (_error, s) => resolve(s.isDirectory())));
console.log("m3", info);
export const files = names.length;
