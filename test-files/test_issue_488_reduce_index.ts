// Refs #488 drizzle-sqlite: Array.prototype.reduce was passing only 2
// args to the callback (accumulator, currentValue) instead of the
// spec's (accumulator, currentValue, currentIndex, array). Drizzle's
// `mapResultRow` callback signature `(result, { path, field }, columnIndex)`
// got `columnIndex === undefined` — every column projection collapsed
// onto `row[undefined]` (perry returns `row[0]`), so `alice.age` came
// back as `1` (the id) instead of `30`.

const arr = [10, 20, 30];

// Basic: index arg present
const indices = arr.reduce((acc: number[], val: number, idx: number) => {
    acc.push(idx);
    return acc;
}, []);
console.log("indices:", JSON.stringify(indices));

// Index-based aggregation (drizzle's pattern)
const objs = [
    { name: "a", val: 100 },
    { name: "b", val: 200 },
    { name: "c", val: 300 },
];
const map = objs.reduce((acc: Record<string, number>, obj, idx) => {
    acc[obj.name] = idx * 10 + obj.val;
    return acc;
}, {} as Record<string, number>);
console.log("map:", JSON.stringify(map));

// reduceRight also gets index
const rev = arr.reduceRight((acc: number[], val: number, idx: number) => {
    acc.push(idx);
    return acc;
}, []);
console.log("reduceRight indices:", JSON.stringify(rev));
