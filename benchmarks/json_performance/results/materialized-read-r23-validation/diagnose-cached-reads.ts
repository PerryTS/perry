// Cache hits at bitmap boundaries, then mutation, growth, holes and getters.
// Run in auto/tape/direct modes, including scheduled moving GC.
const pieces: string[] = [];
for (let i = 0; i < 130; i++) {
    pieces.push('{"id":' + i + ',"name":"a heap string for record ' + i + '"}');
}
const text = "[" + pieces.join(",") + "]";
const probes: number[] = [0, 63, 64, 65, 127, 128, 129];
const retained: any[] = [];
let sum = 0;
for (let round = 0; round < 80; round++) {
    const rows: any = JSON.parse(text);
    const saved: any[] = [];
    for (let p = 0; p < probes.length; p++) saved.push(rows[probes[p]]);
    for (let repeat = 0; repeat < 8; repeat++) {
        for (let p = 0; p < probes.length; p++) {
            const index = probes[p];
            const value: any = rows[index];
            if (value !== saved[p] || value.id !== index ||
                value.name !== "a heap string for record " + index) {
                throw new Error("cached identity/value changed");
            }
            sum += value.id;
        }
        if (rows[130] !== undefined || rows[4294967295] !== undefined) {
            throw new Error("out-of-bounds cache read");
        }
    }
console.log("stage", 29);
    saved[2].id = round + 1000;
    if (rows[64] !== saved[2] || rows[64].id !== round + 1000) {
        throw new Error("cached child mutation lost");
    }
console.log("stage", 33);
    rows[64] = {id: -round, name: "replacement heap string"};
console.log("stage", 34);
    if (rows[64] === saved[2] || rows[64].id !== -round) {
        throw new Error("array replacement lost");
    }
console.log("stage", 37);
    rows.push({id: 130, name: "grown heap string"});
    if (rows[130].id !== 130 || rows.length !== 131) throw new Error("growth lost");
console.log("stage", 39);
    rows.length = 64;
console.log("stage", 40);
    rows.length = 132;
    if (rows[64] !== undefined || rows[130] !== undefined) throw new Error("stale cache after shrink");
console.log("stage", 42);
    Object.defineProperty(rows, "65", {get: () => ({id: 777}), configurable: true});
console.log("stage", 43);
    if (rows[65].id !== 777 || rows[65] === rows[65]) throw new Error("getter bypassed");
    retained.push(saved);
}
console.log("stage", 46);
for (let round = 0; round < retained.length; round++) {
    const saved: any = retained[round];
    if (saved[2].id !== round + 1000 || saved[6].name !== "a heap string for record 129") {
        throw new Error("retained cache value changed");
    }
    sum += saved[2].id;
}
console.log("cached-reads", retained.length, sum);
