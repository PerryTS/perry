// Object.defineProperty on an index of a JSON.parse array must be honoured by
// later reads. Passes with PERRY_JSON_TAPE=0 (direct parse) and fails in the
// default lazy route: the accessor is installed but indexed reads keep
// returning the element, in both the sparse and the materialized state.
// Known gap: #10097.
const pieces: string[] = [];
for (let i = 0; i < 200; i++) {
    pieces.push('{"id":' + i + ',"name":"heap string for record ' + i + '"}');
}
const text = "[" + pieces.join(",") + "]";
let sum = 0;
for (let round = 0; round < 4; round++) {
    // sparse (never scanned) and materialized (scanned) both must honour it
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
