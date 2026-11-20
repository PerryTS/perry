import * as readline from "node:readline";

console.log((readline as any).Interface?.name ?? "missing");
