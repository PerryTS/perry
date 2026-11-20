import { deprecate } from "node:util";

function show(label: string, value: any): void {
  try {
    const wrapped = deprecate(value, "deprecated fn", "DEP_PERRY_TARGET");
    console.log(label, "ok", typeof wrapped);
  } catch (err) {
    const e = err as Error & { code?: string };
    console.log(label, "throw", e.name, e.code ?? "no-code", err instanceof TypeError);
  }
}

show("number", 1);
show("string", "x");
show("boolean", true);
show("null", null);
show("undefined", undefined);

const wrapped = deprecate((x: number) => x + 1, "deprecated fn", "DEP_PERRY_VALID");
console.log("valid", typeof wrapped);
