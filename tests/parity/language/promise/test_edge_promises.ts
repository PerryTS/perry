// Edge-case tests for Promises and async/await
// Targets bugs like: cross-module await on Promise<[T,T]>, Promise.race,
// async closure capture, promise chaining

// --- Basic Promise.resolve ---
const p1 = Promise.resolve(42);
const r1 = await p1;
console.log(r1);  // 42

// --- Promise.resolve with string ---
const p2 = Promise.resolve("hello");
const r2 = await p2;
console.log(r2);  // hello

// --- Promise chaining with .then ---
const p3 = Promise.resolve(10)
    .then((x: number) => x * 2)
    .then((x: number) => x + 5);
const r3 = await p3;
console.log(r3);  // 25

// --- Async function ---
async function asyncAdd(a: number, b: number): Promise<number> {
    return a + b;
}

const r4 = await asyncAdd(3, 4);
console.log(r4);  // 7

// --- Async function with await inside ---
async function asyncChain(): Promise<number> {
    const a = await Promise.resolve(10);
    const b = await Promise.resolve(20);
    return a + b;
}

const r5 = await asyncChain();
console.log(r5);  // 30

// --- Promise.all ---
const all = await Promise.all([
    Promise.resolve(1),
    Promise.resolve(2),
    Promise.resolve(3)
]);
console.log(all[0]);  // 1
console.log(all[1]);  // 2
console.log(all[2]);  // 3

// --- Promise.all with different types ---
const mixed = await Promise.all([
    Promise.resolve(42),
    Promise.resolve("hello"),
    Promise.resolve(true)
]);
console.log(mixed[0]);  // 42
console.log(mixed[1]);  // hello
console.log(mixed[2]);  // true

// --- Async function returning object ---
async function fetchData(): Promise<{ value: number; label: string }> {
    return { value: 42, label: "answer" };
}

const data = await fetchData();
console.log(data.value);  // 42
console.log(data.label);  // answer

// --- Sequential awaits ---
async function sequential(): Promise<number> {
    let sum = 0;
    for (let i = 1; i <= 5; i++) {
        const val = await Promise.resolve(i);
        sum = sum + val;
    }
    return sum;
}

const r6 = await sequential();
console.log(r6);  // 15

// --- Async function with conditional ---
async function asyncConditional(flag: boolean): Promise<string> {
    if (flag) {
        return await Promise.resolve("yes");
    } else {
        return await Promise.resolve("no");
    }
}

console.log(await asyncConditional(true));   // yes
console.log(await asyncConditional(false));  // no

// --- Promise with .then chaining complex transforms ---
const chain = await Promise.resolve([1, 2, 3])
    .then((arr: number[]) => arr.map((x: number) => x * 2))
    .then((arr: number[]) => arr.reduce((a: number, b: number) => a + b, 0));
console.log(chain);  // 12

// --- Async function with try/catch ---
async function safeAsync(): Promise<string> {
    try {
        const val = await Promise.resolve("success");
        return val;
    } catch (e) {
        return "error";
    }
}

console.log(await safeAsync());  // success

// --- Multiple independent promises ---
async function parallel(): Promise<number> {
    const p1 = Promise.resolve(10);
    const p2 = Promise.resolve(20);
    const p3 = Promise.resolve(30);
    const [a, b, c] = await Promise.all([p1, p2, p3]);
    return a + b + c;
}

console.log(await parallel());  // 60

// --- Async function that calls another async function ---
async function inner(): Promise<number> {
    return await Promise.resolve(42);
}

async function outerAsync(): Promise<string> {
    const val = await inner();
    return "value=" + val.toString();
}

console.log(await outerAsync());  // value=42

// --- Promise returning array ---
async function getArray(): Promise<number[]> {
    return [10, 20, 30];
}

const arr = await getArray();
console.log(arr.length);     // 3
console.log(arr.join(","));  // 10,20,30

// --- Async with string operations ---
async function asyncString(): Promise<string> {
    const parts = await Promise.all([
        Promise.resolve("hello"),
        Promise.resolve("world")
    ]);
    return parts.join(" ");
}

console.log(await asyncString());  // hello world

// --- Nested async calls ---
async function level3(): Promise<number> { return 3; }
async function level2(): Promise<number> {
    const v = await level3();
    return v * 2;
}
async function level1(): Promise<number> {
    const v = await level2();
    return v * 2;
}

console.log(await level1());  // 12

// --- Async with map (creating array of promises) ---
const nums = [1, 2, 3, 4, 5];
const promises = nums.map((n: number) => Promise.resolve(n * n));
const squares = await Promise.all(promises);
console.log(squares.join(","));  // 1,4,9,16,25
