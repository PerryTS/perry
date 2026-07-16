import * as vm from "node:vm";

function shape(label: string, fn: () => unknown) {
  try {
    fn();
    console.log(label + ": ok");
  } catch (error: any) {
    console.log(label + ":", error.name, error.code || "-");
  }
}

shape("unexpected token", () => vm.compileFunction("return )"));
shape("unterminated body", () => vm.compileFunction("if (true) {"));
