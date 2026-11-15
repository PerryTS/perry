const tag = process.env.SNAPK2B_ENTRY;
const { uid } = await import("./m.ts");
gc();
console.log("K2B_F8", typeof uid, tag ?? "-");
