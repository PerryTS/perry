import * as util from "node:util";

const diff = (util as any).diff;

function run(label: string, actual: any, expected: any) {
  try {
    console.log(label + ":", JSON.stringify(diff(actual, expected)));
  } catch (err) {
    const e = err as any;
    console.log(
      label + " error:",
      e.name,
      e.code,
      e.message.split("\n")[0],
    );
  }
}

console.log("typeof:", typeof diff);
console.log("length/name:", diff.length, diff.name);
console.log("keys includes:", Object.keys(util).includes("diff"));

run("string replace", "abc", "adc");
run("string add", "ab", "abc");
run("string same", "same", "same");
run("array replace", ["a", "b", "c"], ["a", "d", "c"]);
run("array same", ["a", "b"], ["a", "b"]);
run("mixed actual string", "ab", ["a", "b"]);
run("mixed expected string", ["a", "b"], "ab");
run("invalid actual", 123, "x");
run("invalid expected", "x", 123);
run("invalid actual element", ["a", 1], ["a", "1"]);
run("invalid expected element", ["x"], ["x", false]);
