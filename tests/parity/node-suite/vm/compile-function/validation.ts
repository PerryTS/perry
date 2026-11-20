import * as vm from "node:vm";

function shape(label: string, fn: () => unknown) {
  try {
    fn();
    console.log(label + ": ok");
  } catch (error: any) {
    console.log(label + ":", error.name, error.code || "-");
  }
}

shape("missing code", () => (vm.compileFunction as any)());
shape("params null", () => vm.compileFunction("", null as any));
shape("param type", () => vm.compileFunction("", [1 as any]));
shape("options null", () => vm.compileFunction("", [], null as any));
shape("filename", () => vm.compileFunction("", [], { filename: null as any }));
shape(
  "lineOffset",
  () => vm.compileFunction("", [], { lineOffset: "1" as any }),
);
shape(
  "columnOffset",
  () => vm.compileFunction("", [], { columnOffset: null as any }),
);
shape(
  "extensions null",
  () => vm.compileFunction("", [], { contextExtensions: null as any }),
);
shape(
  "extensions member",
  () => vm.compileFunction("", [], { contextExtensions: [1 as any] }),
);
