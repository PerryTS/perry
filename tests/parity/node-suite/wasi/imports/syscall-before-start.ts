import { WASI } from "node:wasi";

const W: any = WASI;

const wasi: any = new W({ version: "preview1" });
for (
  const [name, args] of [
    ["args_sizes_get", [0, 4]],
    ["environ_sizes_get", [0, 4]],
    ["random_get", [0, 0]],
  ] as const
) {
  try {
    console.log(name + ": ok", wasi.wasiImport[name](...args));
  } catch (error: any) {
    console.log(name + ": throw", error?.name, error?.code || "no-code");
  }
}
