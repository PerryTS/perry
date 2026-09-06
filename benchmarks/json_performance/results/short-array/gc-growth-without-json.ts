const records: any[] = [];
for (let i = 0; i < 12000; i++) records.push({ id: i, tags: ["tag_a", "tag_b"] });
let sum = 0;
let failures = 0;
for (let i = 0; i < records.length; i++) {
    const value: any = records[i];
    const tags: any = value.tags;
    for (let j = 0; j < 70; j++) tags.push(j);
    if (value.id !== i || value.tags[0] !== "tag_a" || value.tags[71] !== 69) failures++;
    sum += value.id + value.tags.length;
}
console.log("result", failures, records.length, sum);
if (failures !== 0) throw new Error("array growth failure");
