function typeOfThrow(fn: () => void): string { try { fn(); return "no throw"; } catch (e: any) { return e && e.name; } }
console.log("time symbol:", typeOfThrow(() => console.time(Symbol("test") as any)));
console.log("timeEnd symbol:", typeOfThrow(() => console.timeEnd(Symbol("test") as any)));
