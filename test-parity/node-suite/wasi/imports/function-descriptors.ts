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
}
