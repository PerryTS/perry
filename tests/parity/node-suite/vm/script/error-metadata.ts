import * as vm from "node:vm";

function capture(label: string, fn: () => unknown, filename: string) {
  try {
    fn();
    console.log(label + ": ok");
  } catch (error: any) {
    console.log(
      label + ":",
      error.name,
      error.code || "-",
      String(error.stack).includes(filename),
    );
  }
}

capture(
  "syntax filename",
  () => new vm.Script(")", { filename: "syntax-fixture.vm" }),
  "syntax-fixture.vm",
);
capture(
  "syntax offsets",
  () =>
    new vm.Script(")", {
      filename: "offset-fixture.vm",
      lineOffset: 4,
      columnOffset: 3,
    }),
  "offset-fixture.vm",
);
const offsetStack = new vm.Script("new Error().stack", {
  filename: "offset-fixture.vm",
  lineOffset: 4,
  columnOffset: 3,
}).runInThisContext();
console.log(
  "offset position:",
  String(offsetStack).includes("offset-fixture.vm:5:4"),
);
const runtime = new vm.Script("throw new TypeError('boom')", {
  filename: "runtime-fixture.vm",
});
capture(
  "runtime filename",
  () => runtime.runInThisContext(),
  "runtime-fixture.vm",
);
