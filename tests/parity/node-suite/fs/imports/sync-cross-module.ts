import fs from "node:fs";
import { syncBuiltinESMExports as sync } from "node:module";
import { read } from "./fixtures/sync-reader.ts";
const original = fs.readFileSync;
fs.readFileSync = (() => "cross-module patch") as typeof fs.readFileSync;
sync();
console.log(read("not-a-file"));
fs.readFileSync = original;
sync();
