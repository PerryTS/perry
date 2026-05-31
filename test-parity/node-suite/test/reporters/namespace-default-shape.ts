import reportersDefault, { dot, junit, lcov, spec, tap } from "node:test/reporters";
import * as reporters from "node:test/reporters";

const expected = ["dot", "junit", "lcov", "spec", "tap"] as const;

console.log("namespace default type:", typeof reporters.default);
console.log("default import identity:", reportersDefault === reporters.default);
console.log("default is namespace:", reportersDefault === reporters);
console.log(
  "default has own default:",
  Object.prototype.hasOwnProperty.call(reportersDefault, "default"),
);

for (const name of expected) {
  console.log(
    `${name}:`,
    typeof reporters[name],
    typeof reportersDefault[name],
    reporters[name] === reportersDefault[name],
  );
}

console.log("named identities:", dot === reporters.dot, junit === reporters.junit);
console.log("more named identities:", lcov === reporters.lcov, spec === reporters.spec, tap === reporters.tap);
