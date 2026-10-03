// Object.defineProperty on a JSON.parse array index must be honoured whether
// the lazy array was unscanned or already materialized before the definition.
const pieces: string[] = [];
for (let i = 0; i < 200; i++) {
    pieces.push('{"id":' + i + ',"name":"heap string for record ' + i + '"}');
}
const text = "[" + pieces.join(",") + "]";
let sum = 0;
for (let round = 0; round < 4; round++) {
    // Both unscanned and previously scanned arrays must honour the accessor.
    for (const scan of [false, true]) {
        const rows: any = JSON.parse(text);
        if (scan) { let seen = 0; for (let i = 0; i < rows.length; i++) seen += rows[i].id; sum += seen; }
        let getterCalls = 0;
        Object.defineProperty(rows, 5, {
            configurable: true,
            get: function () { getterCalls++; return {id: -5, name: "from getter"}; },
        });
        for (let repeat = 0; repeat < 8; repeat++) {
            const value: any = rows[5];
            if (value.id !== -5 || value.name !== "from getter") {
                throw new Error("descriptor read bypassed (scan=" + scan + ")");
            }
            if (rows[6].id !== 6) throw new Error("neighbour read broken");
            sum += value.id;
        }
        if (getterCalls !== 8) throw new Error("getter calls: " + getterCalls);
    }
}
console.log("lazy-defineproperty-index", sum);
