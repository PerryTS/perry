import * as vm from "node:vm";

function shape(label: string, fn: () => unknown) {
  try {
    fn();
    console.log(label + ": ok");
  } catch (error: any) {
    console.log(label + ":", error.name, error.code || "-");
  }
}

shape("call", () => (vm.Script as any)("1"));
shape("options", () => new vm.Script("1", 42 as any));
shape("filename", () => new vm.Script("1", { filename: 1 as any }));
shape("lineOffset type", () => new vm.Script("1", { lineOffset: "1" as any }));
shape("lineOffset range", () => new vm.Script("1", { lineOffset: 0.5 }));
shape(
  "columnOffset type",
  () => new vm.Script("1", { columnOffset: null as any }),
);
shape(
  "columnOffset range",
  () => new vm.Script("1", { columnOffset: 2 ** 32 }),
);
