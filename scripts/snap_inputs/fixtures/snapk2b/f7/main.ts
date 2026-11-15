// a.ts is reached by import() (R2) and imports b.ts statically (R1): at
// b.ts's init both the entry and a.ts are open frames.
await import("./a.ts");
gc();
console.log("K2B_F7", (globalThis as any).ab);
