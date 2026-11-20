const set: any = new Set();
for (let i = 0; i < 64; i++) set.add({ value: i });
for (let i = 0; i < 64; i++) set.add("key" + i);
for (let i = 0; i < 64; i++) set.add(i);
for (let i = 0; i < 40000; i++) { const x = { value: i }; if (x.value !== i) throw new Error("allocation"); }
let sum = 0;
for (const value of set) if (typeof value === "object") sum += value.value;
console.log("set-owned", set.size, sum, set.has("key37"), set.has(37));
set.delete(37); set.add(37);
console.log("fresh", new Set().has("key37"), set.size, set.has(37));
try { Set.prototype.has.call({}, 37); console.log("bad brand"); }
catch (error) { console.log("brand", error instanceof TypeError); }
