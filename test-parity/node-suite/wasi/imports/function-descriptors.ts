import { WASI } from "node:wasi";

const W: any = WASI;

const wasiImport: any = new W({ version: "preview1" }).wasiImport;
for (
  const name of [
    "args_get",
    "clock_time_get",
    "fd_write",
    "proc_exit",
    "random_get",
  ]
) {
  const value = wasiImport[name];
  const descriptor = Object.getOwnPropertyDescriptor(wasiImport, name)!;
  console.log(
    name + ":",
    value.name,
    value.length,
    descriptor.enumerable,
    descriptor.configurable,
    descriptor.writable,
  );
  console.log(
    name + " prototype:",
    Object.prototype.hasOwnProperty.call(value, "prototype"),
    typeof value.prototype,
  );
}

try {
  const value = Reflect.construct(wasiImport.args_get, [0, 0]);
  console.log("args_get construct: ok", typeof value);
} catch (error: any) {
  console.log(
    "args_get construct: throw",
    error?.name,
    error?.code || "no-code",
  );
}
