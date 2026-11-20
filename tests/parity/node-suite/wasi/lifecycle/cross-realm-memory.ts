import vm from "node:vm";
import { WASI } from "node:wasi";

const W: any = WASI;
const memory: any = vm.runInNewContext(
  "new WebAssembly.Memory({ initial: 1 })",
);
for (
  const [label, instance, method] of [
    ["start", { exports: { memory, _start() {} } }, "start"],
    [
      "initialize",
      { exports: { memory, _initialize() {} } },
      "initialize",
    ],
  ] as const
) {
  try {
    console.log(
      label + ": ok",
      String(new W({ version: "preview1" })[method](instance)),
    );
  } catch (error: any) {
    console.log(label + ": throw", error?.name, error?.code || "no-code");
  }
}
