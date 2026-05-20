function typeOfThrow(fn: () => void): string { try { fn(); return "no throw"; } catch (e: any) { return e && e.name; } }
console.log("count symbol:", typeOfThrow(() => console.count(Symbol("test") as any)));
console.log("countReset symbol:", typeOfThrow(() => console.countReset(Symbol("test") as any)));
