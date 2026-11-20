import { WASI } from "node:wasi";

const W: any = WASI;
const accesses: string[] = [];
const values: Record<string, any> = {
  version: "preview1",
  args: ["tool"],
  env: { A: "one" },
  preopens: {},
  stdin: 0,
  stdout: 1,
  stderr: 2,
  returnOnExit: true,
};
const options: any = {};

for (const key of Object.keys(values)) {
  Object.defineProperty(options, key, {
    configurable: true,
    enumerable: true,
    get() {
      accesses.push(key);
      return values[key];
    },
  });
}

new W(options);

console.log("accesses:", accesses.join("|"));
console.log(
  "counts:",
  Object.keys(values).map((key) =>
    `${key}=${accesses.filter((value) => value === key).length}`
  ).join("|"),
);
