import * as vm from "node:vm";

function shape(label: string, fn: () => unknown) {
  try {
    fn();
    console.log(label + ": ok");
  } catch (error: any) {
    console.log(label + ":", error.name, error.code || "-");
  }
}

for (
  const [label, value] of [["null", null], ["string", "x"], ["number", 1], [
    "boolean",
    true,
  ]]
) {
  shape("sandbox " + label, () => vm.createContext(value as any));
}
shape("options null", () => vm.createContext({}, null as any));
shape("options string", () => vm.createContext({}, "x" as any));
shape("name null", () => vm.createContext({}, { name: null as any }));
shape("origin number", () => vm.createContext({}, { origin: 1 as any }));
