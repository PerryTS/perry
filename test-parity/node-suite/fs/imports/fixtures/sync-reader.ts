import { readFileSync } from "node:fs";
export function read(path: string) { return readFileSync(path, "utf8"); }
