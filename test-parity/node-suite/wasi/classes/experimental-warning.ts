let matches = 0;
process.on("warning", (warning: any) => {
  if (
    warning?.name === "ExperimentalWarning" &&
    String(warning?.message).startsWith("WASI is an experimental feature")
  ) {
    matches++;
  }
});

await import("node:wasi");
await import("node:wasi");
await new Promise<void>((resolve) => setImmediate(resolve));
console.log("normalized WASI warnings:", matches);
