import { readFileSync } from "node:fs";
import { load } from "./generated/bindings";

async function main() {
  const component = load(readFileSync(process.argv[2]), {}, {
    fuel: 1000000000, memoryBytes: 268435456, wasi: {},
  });
  const value = await component.exports.ping(41);
  if (value !== 42) throw new Error("public async-only result");
  component.dispose();
  console.log("PUBLIC ALL-ASYNC P3 PASS");
}
main();
