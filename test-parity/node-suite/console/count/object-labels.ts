const obj = { toString() { return "obj-label"; } };
console.count(obj as any);
console.count(obj as any);
console.countReset(obj as any);
console.count(obj as any);
try { console.count(Symbol("s") as any); } catch (err: any) { console.log("symbol count:", err?.name, err?.code || "no-code"); }
