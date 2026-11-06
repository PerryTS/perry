// Size policy must preserve values, callbacks, exceptions, and live roots.
function textLeaf(left: string, right: string) {
    return left.toUpperCase() + ":" + right.toLowerCase();
}
function numberLeaf(left: number, right: number) {
    return Math.imul((left + 17) | 0, (right ^ 123) | 0) >>> 0;
}
function identityLeaf(value: any) {
    return value;
}
function throwLeaf(value: any): never {
    throw value;
}
function callbackLeaf(callback: (n: number) => number, value: number) {
    return callback(value) + callback(value + 1);
}

let checksum = 0;
for (let batch = 0; batch < 4; batch++) {
    const retained: any[] = [];
    const marker = { batch, text: textLeaf("héLLo", "WoRLD") };
    for (let i = 0; i < 1000; i++) {
        const row = { value: i, text: "row-" + i };
        const same = identityLeaf(row);
        if (same !== row) throw new Error("identity mismatch");
        retained.push(same);
        checksum = (checksum + numberLeaf(i, batch) + numberLeaf(i + 1, batch + 1)) >>> 0;
    }
    const captured = retained[999];
    const closureResult = callbackLeaf(n => numberLeaf(n, captured.value), batch);
    let caught = false;
    try {
        throwLeaf(marker);
    } catch (error) {
        caught = error === marker;
    }
    if (!caught) throw new Error("exception identity mismatch");
    console.log(batch, checksum, closureResult, textLeaf(captured.text, marker.text), caught);
}
console.log("inline-policy-native-complete", checksum);
