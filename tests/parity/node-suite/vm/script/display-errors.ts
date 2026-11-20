import * as vm from "node:vm";

for (const displayErrors of [true, false]) {
  try {
    vm.runInThisContext("throw new RangeError('stable')", {
      filename: "display-errors.vm",
      displayErrors,
    });
    console.log("display " + displayErrors + ": ok");
  } catch (error: any) {
    console.log(
      "display " + displayErrors + ":",
      error.name,
      error.code || "-",
      String(error.stack).includes("display-errors.vm"),
    );
  }
}
