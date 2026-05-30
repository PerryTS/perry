import { deprecate } from "node:util";

function check(label: string, value: any) {
  try {
    const wrapped = deprecate(value, "deprecated");
    console.log(label, "ok", typeof wrapped);
  } catch (err: any) {
    console.log(label, "throw", err?.name, err instanceof TypeError);
  }
}

check("undefined", undefined);
check("null", null);
check("number", 1);
check("string", "fn");

const wrapped = deprecate((x: number) => x + 1, "deprecated", "DEP_PERRY_TEST");
console.log("callable", typeof wrapped, wrapped(1));
