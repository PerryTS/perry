// The entry decides by argv[2] which init it reaches (dispatch.ts:8,14 shape).
if (process.argv[2] === "B") {
  await import("./b.ts");
}
await import("./a.ts");
console.log("K2A_F3", (globalThis as any).seen);
