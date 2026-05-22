import os from "node:os";

for (const name of ["availableParallelism", "machine", "version", "endianness"] as const) {
  try {
    const value = (os as any)[name]();
    console.log(name + ":", typeof value, String(value).length > 0);
  } catch (err: any) { console.log(name + ":", err?.name, err?.code || "no-code"); }
}
