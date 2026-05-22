const label = { toString() { return "timer-object"; } };
console.time(label as any);
console.timeLog(label as any, "mid");
console.timeEnd(label as any);
try { console.time(Symbol("t") as any); } catch (err: any) { console.log("symbol time:", err?.name, err?.code || "no-code"); }
