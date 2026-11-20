let matches = 0;
process.on("warning", (warning: any) => {
  if (
    warning?.name === "ExperimentalWarning" &&
    String(warning?.message).startsWith("WASI is an experimental feature")
  ) {
    matches++;
  }
});

const module = await import("node:wasi");
await import("node:wasi");
await new Promise<void>((resolve) => setImmediate(resolve));
console.log("after import:", matches);

const W: any = module.WASI;
new W({ version: "preview1" });
new W({ version: "preview1" });
await new Promise<void>((resolve) => setImmediate(resolve));
console.log("after construction:", matches);
