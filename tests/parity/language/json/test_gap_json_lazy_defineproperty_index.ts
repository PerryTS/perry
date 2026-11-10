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

let redefineFailures = "";
for (const mode of ["ordinary", "unscanned", "scanned"]) {
    const locked: any = mode === "ordinary" ? [{id: 0}, {id: 1}] : JSON.parse(text);
    const converted: any = mode === "ordinary" ? [{id: 0}, {id: 1}] : JSON.parse(text);
    if (mode === "scanned") {
        let scanSum = 0;
        for (let i = 0; i < locked.length; i++) scanSum += locked[i].id + converted[i].id;
        if (scanSum !== 39800) throw new Error("scan read broken");
    }
    Object.defineProperty(locked, 1, {get: function () { return 61; }, configurable: false});
    let rejected = false;
    try {
        Object.defineProperty(locked, 1, {get: function () { return 92; }});
    } catch (error) {
        rejected = error instanceof TypeError;
    }
    let reflectRejected = false;
    try {
        reflectRejected = Reflect.defineProperty(locked, 1, {get: function () { return 92; }}) === false;
    } catch (error) {
        redefineFailures += mode + ":reflect-threw;";
    }
    if (!reflectRejected || locked[1] !== 61) redefineFailures += mode + ":reflect-locked;";
    const reflectDeleted = Reflect.deleteProperty(locked, 1);
    if (reflectDeleted || locked[1] !== 61) redefineFailures += mode + ":reflect-delete;";
    console.log("reflect-redefine-index", mode, reflectRejected, reflectDeleted, locked[1]);
    const lockedValue = locked[1];
    if (!rejected || lockedValue !== 61) redefineFailures += mode + ":locked;";

    let convertedGetterCalls = 0;
    Object.defineProperty(converted, 1, {
        get: function () { convertedGetterCalls++; return 61; },
        configurable: true,
    });
    Object.defineProperty(converted, 1, {enumerable: false});
    if (converted[1] !== 61 || convertedGetterCalls !== 1) redefineFailures += mode + ":generic;";
    Object.defineProperty(converted, 1, {value: 73, writable: false, configurable: false});
    if (converted[1] !== 73 || convertedGetterCalls !== 1) redefineFailures += mode + ":data;";
    let dataRejected = false;
    try {
        Object.defineProperty(converted, 1, {value: 74});
    } catch (error) {
        dataRejected = error instanceof TypeError;
    }
    if (!dataRejected || converted[1] !== 73) redefineFailures += mode + ":attrs;";
    if (converted[0].id !== 0) redefineFailures += mode + ":neighbour;";
    console.log("redefine-index", mode, rejected, lockedValue, dataRejected, converted[1], convertedGetterCalls);
}
if (redefineFailures !== "") throw new Error("index redefinitions: " + redefineFailures);
