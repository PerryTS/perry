import * as vm from "node:vm";

for (
  const [label, value] of [
    ["undefined", undefined],
    ["null", null],
    ["string", "x"],
    ["number", 1],
    ["boolean", false],
  ] as const
) {
  try {
    console.log(label + ":", vm.isContext(value as any));
  } catch (error: any) {
    console.log(label + ":", error.name, error.code || "-");
  }
}
