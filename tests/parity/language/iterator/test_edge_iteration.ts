// Edge-case tests for iteration: for-of, for-in, iterables,
// Array.from, spread with iterables, iterator protocol patterns

// --- for...of with arrays ---
const nums = [10, 20, 30];
const forOfResult: number[] = [];
for (const n of nums) {
    forOfResult.push(n);
}
console.log(forOfResult.join(","));  // 10,20,30

// --- for...of with strings ---
const chars: string[] = [];
for (const ch of "hello") {
    chars.push(ch);
}
console.log(chars.join(","));  // h,e,l,l,o

// --- for...in with objects ---
const obj: Record<string, number> = { a: 1, b: 2, c: 3 };
const keys: string[] = [];
for (const k in obj) {
    keys.push(k);
}
console.log(keys.length);  // 3

// --- for...of with Map entries ---
const map = new Map<string, number>();
map.set("x", 10);
map.set("y", 20);
map.set("z", 30);

const mapEntries: string[] = [];
map.forEach((v: number, k: string) => {
    mapEntries.push(k + "=" + v.toString());
});
console.log(mapEntries.join(","));  // x=10,y=20,z=30

// --- for...of with Set ---
const set = new Set([1, 2, 3, 4, 5]);
let setSum = 0;
set.forEach((v: number) => {
    setSum = setSum + v;
});
console.log(setSum);  // 15

// --- Array.from ---
const fromSet = Array.from(new Set([3, 1, 4, 1, 5, 9, 2, 6]));
console.log(fromSet.length);  // 6 (duplicates removed)

// --- Array.from with map function ---
const mapped = Array.from([1, 2, 3, 4, 5], (x: number) => x * x);
console.log(mapped.join(","));  // 1,4,9,16,25

// --- Spread into array ---
const setArr = [...new Set([1, 2, 3, 2, 1])];
console.log(setArr.length);     // 3
console.log(setArr.join(","));  // 1,2,3

// --- Nested iteration ---
const matrix = [[1, 2, 3], [4, 5, 6], [7, 8, 9]];
let matrixSum = 0;
for (const row of matrix) {
    for (const val of row) {
        matrixSum = matrixSum + val;
    }
}
console.log(matrixSum);  // 45

// --- for...of with break ---
const breakArr = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
let breakSum = 0;
for (const n of breakArr) {
    if (n > 5) break;
    breakSum = breakSum + n;
}
console.log(breakSum);  // 15

// --- for...of with continue ---
const contArr = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
let contSum = 0;
for (const n of contArr) {
    if (n % 2 === 0) continue;
    contSum = contSum + n;
}
console.log(contSum);  // 25

// --- for...of with destructuring ---
const pairs: [string, number][] = [["a", 1], ["b", 2], ["c", 3]];
const pairStrs: string[] = [];
for (const [key, val] of pairs) {
    pairStrs.push(key + ":" + val.toString());
}
console.log(pairStrs.join(","));  // a:1,b:2,c:3

// --- Iterating Object.keys/values/entries ---
const record: Record<string, number> = { x: 10, y: 20, z: 30 };

let keyStr = "";
for (const k of Object.keys(record)) {
    keyStr = keyStr + k;
}
console.log(keyStr);  // xyz

let valSum = 0;
for (const v of Object.values(record)) {
    valSum = valSum + v;
}
console.log(valSum);  // 60

const entryStrs: string[] = [];
for (const [k, v] of Object.entries(record)) {
    entryStrs.push(k + "=" + v.toString());
}
console.log(entryStrs.join(","));  // x=10,y=20,z=30

// --- Complex iteration pattern ---
function groupByLength(words: string[]): Record<number, string[]> {
    const groups: Record<number, string[]> = {};
    for (const word of words) {
        const len = word.length;
        if (!groups[len]) {
            groups[len] = [];
        }
        groups[len].push(word);
    }
    return groups;
}

const grouped = groupByLength(["hi", "hey", "hello", "yo", "sup", "greetings"]);
console.log(grouped[2].join(","));   // hi,yo
console.log(grouped[3].join(","));   // hey,sup
console.log(grouped[5].join(","));   // hello

// --- Counting with iteration ---
function countOccurrences(arr: string[]): Map<string, number> {
    const counts = new Map<string, number>();
    for (const item of arr) {
        const current = counts.get(item);
        counts.set(item, (current !== undefined ? current : 0) + 1);
    }
    return counts;
}

const counts = countOccurrences(["a", "b", "a", "c", "b", "a"]);
console.log(counts.get("a"));  // 3
console.log(counts.get("b"));  // 2
console.log(counts.get("c"));  // 1

// --- Reverse iteration ---
const revArr = [1, 2, 3, 4, 5];
const reversed: number[] = [];
for (let i = revArr.length - 1; i >= 0; i--) {
    reversed.push(revArr[i]);
}
console.log(reversed.join(","));  // 5,4,3,2,1

// --- While loop as iterator ---
function fibonacci(n: number): number[] {
    const result: number[] = [];
    let a = 0;
    let b = 1;
    let count = 0;
    while (count < n) {
        result.push(a);
        const temp = b;
        b = a + b;
        a = temp;
        count++;
    }
    return result;
}

console.log(fibonacci(10).join(","));  // 0,1,1,2,3,5,8,13,21,34

// --- Multi-level iteration with accumulation ---
const data = [
    { category: "A", values: [1, 2, 3] },
    { category: "B", values: [4, 5] },
    { category: "C", values: [6, 7, 8, 9] }
];

let totalValues = 0;
for (const item of data) {
    for (const v of item.values) {
        totalValues = totalValues + v;
    }
}
console.log(totalValues);  // 45
