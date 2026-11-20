import { WASI } from "node:wasi";

const W: any = WASI;
class DerivedWASI extends W {}

try {
  const value: any = new DerivedWASI({ version: "preview1" });
  console.log("construct: ok");
  console.log(
    "instances:",
    value instanceof DerivedWASI,
    value instanceof WASI,
  );
  console.log(
    "prototypes:",
    Object.getPrototypeOf(value) === DerivedWASI.prototype,
    Object.getPrototypeOf(DerivedWASI.prototype) === WASI.prototype,
  );
  console.log(
    "inherited API:",
    typeof value.wasiImport,
    typeof value.getImportObject,
  );
} catch (error: any) {
  console.log("construct: throw", error?.name, error?.code || "no-code");
}
