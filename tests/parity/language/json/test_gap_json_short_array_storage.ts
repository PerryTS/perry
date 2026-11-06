let failures = 0;
let checksum = 0;
const retained: any[] = [];
for (let width = 0; width <= 17; width++) {
    const pieces: string[] = [];
    for (let i = 0; i < width; i++) pieces.push(String(i));
    const source = "[" + pieces.join(",") + "]";
    const array: any = JSON.parse(source);
    const alias: any = array;
    if (JSON.stringify(array) !== source) failures++;
    for (let i = width; i < 70; i++) array.push(i);
    if (alias.length !== 70 || alias[69] !== 69) failures++;
    array.length = 2;
    array.length = 6;
    if (2 in alias || 5 in alias) failures++;
    array[4] = "filled";
    delete array[1];
    if (1 in alias || alias[4] !== "filled") failures++;
    checksum += Object.keys(alias).length;
    retained.push(alias);
}
console.log("growth", failures, checksum, JSON.stringify(retained[0]));
const accessorArray: any = JSON.parse('[0,1]');
const accessorAlias: any = accessorArray;
for (let i = 2; i < 20; i++) accessorArray.push(i);
let getterCalls = 0;
Object.defineProperty(accessorArray, "1", {
    get: function () { getterCalls++; return "from getter"; },
    enumerable: true,
    configurable: true,
});
if (accessorAlias[1] !== "from getter") failures++;
console.log("getter", JSON.stringify(accessorAlias), getterCalls);
if (getterCalls !== 2) failures++;
for (const source of ['[]', '[true,false,null]', '["tag_a","tag_b"]', '[0,{"x":"heap child"},[true,false],"tail"]', '[1,2,3,4,5,6,7,8,9]']) {
    const value: any = JSON.parse(source);
    const alias: any = value;
    value.unshift("front");
    value.splice(1, 0, "inserted");
    value.reverse();
    value.push("end");
    console.log("mutated", JSON.stringify(alias), Object.keys(alias).join(","));
}
let rejected = 0;
for (const bad of ['[', '[1,]', '[1,,2]', '[01]', '[1]x', '[[0],]', '[1,2,3,4,5,6,7,8,]', '[1,2,3,4,5,6,7,8,9,]']) {
    try { JSON.parse(bad); }
    catch (error: any) { if (error.name === "SyntaxError") rejected++; }
}
console.log("invalid", rejected);
const records: any[] = [];
for (let i = 0; i < 12000; i++) {
    const text = '{"id":' + i + ',"tags":["tag_a","tag_b"],"data":[0,{"x":"heap child"}]}';
    records.push(JSON.parse(text));
}
let sum = 0;
for (let i = 0; i < records.length; i++) {
    const value: any = records[i];
    const tags: any = value.tags;
    const alias: any = tags;
    for (let j = 0; j < 20; j++) tags.push(j);
    value.data.push(value.id);
    if (value.id !== i || alias[0] !== "tag_a" || alias[21] !== 19 || value.data[1].x !== "heap child") failures++;
    sum = (sum + value.id + alias.length + value.data.length) % 1000000007;
}
console.log("retained", failures, records.length, sum, JSON.stringify(records[11999]));
console.log("zeros", Object.is(JSON.parse('[-0]')[0], -0), JSON.stringify(JSON.parse('[1e400,-1e400]')));
if (failures !== 0 || rejected !== 8) throw new Error("short array failure");
