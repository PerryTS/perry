// Run Perry with PERRY_JSON_TAPE=1 to exercise original lazy wrappers.
let failures = 0;
let checksum = 0;
const retained: any[] = [];
for (let width = 0; width <= 17; width++) {
    const pieces: string[] = [];
    for (let i = 0; i < width; i++) pieces.push(String(i));
    const value: any = JSON.parse("[" + pieces.join(",") + "]");
    const alias: any = value;
    for (let i = width; i < 70; i++) value.push(i);
    if (alias.length !== 70 || alias[69] !== 69) failures++;
    value.length = 2;
    value.length = 6;
    if (alias[2] !== undefined || alias[5] !== undefined) failures++;
    value[4] = "filled";
    if (JSON.stringify(alias) !== '[0,1,null,null,"filled",null]') failures++;
    checksum += alias.length;
    retained.push(alias);
}
console.log("growth", failures, checksum, JSON.stringify(retained[0]));
const records: any[] = [];
for (let i = 0; i < 12000; i++) {
    const value: any = JSON.parse('["heap child survives",' + i + ']');
    for (let j = 0; j < 20; j++) value.push(j);
    records.push(value);
}
let sum = 0;
for (let i = 0; i < records.length; i++) {
    const value: any = records[i];
    if (value[0] !== "heap child survives" || value[1] !== i || value[21] !== 19 || value.length !== 22) failures++;
    sum += value[1] + value.length;
}
console.log("retained", failures, records.length, sum, JSON.stringify(records[11999]));
if (failures !== 0) throw new Error("lazy growth failure");
