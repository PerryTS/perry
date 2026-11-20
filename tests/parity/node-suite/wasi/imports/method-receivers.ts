import { WASI } from "node:wasi";

const W: any = WASI;

const wasi = new W({ version: "preview1" });
for (
  const [label, fn] of [
    ["getImportObject plain", () => WASI.prototype.getImportObject.call({})],
    ["start plain", () => WASI.prototype.start.call({}, {})],
    ["initialize plain", () => WASI.prototype.initialize.call({}, {})],
    ["finalize plain", () => WASI.prototype.finalizeBindings.call({}, {})],
    [
      "getImportObject instance",
      () => WASI.prototype.getImportObject.call(wasi),
    ],
  ] as const
) {
  try {
    const value: any = fn();
    console.log(label + ": ok", Object.keys(value).join(","));
  } catch (error: any) {
    console.log(label + ": throw", error?.name, error?.code || "no-code");
  }
}
