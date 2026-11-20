// Edge-case tests for const patterns, let/var scoping, constant objects as
// enum replacements, and constant folding

// --- Const object as enum replacement (numeric) ---
const Direction = { Up: 0, Down: 1, Left: 2, Right: 3 } as const;

console.log(Direction.Up);     // 0
console.log(Direction.Down);   // 1
console.log(Direction.Left);   // 2
console.log(Direction.Right);  // 3

// --- Const in switch ---
function directionName(d: number): string {
    switch (d) {
        case Direction.Up: return "up";
        case Direction.Down: return "down";
        case Direction.Left: return "left";
        case Direction.Right: return "right";
        default: return "unknown";
    }
}

console.log(directionName(Direction.Up));    // up
console.log(directionName(Direction.Left));  // left

// --- String const object (like string enum) ---
const Color = { Red: "RED", Green: "GREEN", Blue: "BLUE" } as const;

console.log(Color.Red);    // RED
console.log(Color.Green);  // GREEN
console.log(Color.Blue);   // BLUE

// --- Const with explicit values (like valued enum) ---
const HttpStatus = { OK: 200, NotFound: 404, ServerError: 500 } as const;

console.log(HttpStatus.OK);          // 200
console.log(HttpStatus.NotFound);    // 404
console.log(HttpStatus.ServerError); // 500

// --- Const comparison ---
const status = HttpStatus.OK;
console.log(status === HttpStatus.OK);        // true
console.log(status === HttpStatus.NotFound);  // false

// --- const declarations ---
const PI = 3.14159265358979;
const MAX_SIZE = 100;
const GREETING = "Hello";

console.log(PI);        // 3.14159265358979
console.log(MAX_SIZE);  // 100
console.log(GREETING);  // Hello

// --- const in different scopes ---
const outerConst = "outer";
{
    const innerConst = "inner";
    console.log(outerConst);  // outer
    console.log(innerConst);  // inner
}
console.log(outerConst);  // outer

// --- let scoping in blocks ---
let blockVar = "before";
{
    let blockVar = "inside";
    console.log(blockVar);  // inside
}
console.log(blockVar);  // before

// --- let in for loop (each iteration gets own binding) ---
const closures: (() => number)[] = [];
for (let i = 0; i < 5; i++) {
    closures.push(() => i);
}
console.log(closures[0]());  // 0
console.log(closures[2]());  // 2
console.log(closures[4]());  // 4

// --- Const arrays (mutable contents) ---
const constArr = [1, 2, 3];
constArr.push(4);
console.log(constArr.length);      // 4
console.log(constArr.join(","));   // 1,2,3,4

// --- Const objects (mutable contents) ---
const constObj: Record<string, number> = { a: 1 };
constObj["b"] = 2;
console.log(Object.keys(constObj).length);  // 2

// --- Constant folding patterns ---
console.log(2 + 3);        // 5
console.log(10 * 20);      // 200
console.log(100 / 4);      // 25
console.log("a" + "b");    // ab

// --- Const used as object key ---
const constMap: Record<number, string> = {};
constMap[Direction.Up] = "going up";
constMap[Direction.Down] = "going down";
console.log(constMap[Direction.Up]);  // going up

// --- Complex const expressions ---
const BASE = 10;
const MULTIPLIER = 5;
const COMPUTED = BASE * MULTIPLIER;
console.log(COMPUTED);  // 50

// --- Type aliases with const ---
type Pair = [number, number];
const myPair: Pair = [10, 20];
console.log(myPair[0]);  // 10
console.log(myPair[1]);  // 20

// --- Readonly array pattern ---
function sumArray(arr: readonly number[]): number {
    let total = 0;
    for (let i = 0; i < arr.length; i++) {
        total = total + arr[i];
    }
    return total;
}

console.log(sumArray([1, 2, 3, 4, 5]));  // 15

// --- Const values in array ---
function getConstValues(): number[] {
    const values: number[] = [];
    values.push(Direction.Up);
    values.push(Direction.Down);
    values.push(Direction.Left);
    values.push(Direction.Right);
    return values;
}

const constVals = getConstValues();
console.log(constVals.join(","));  // 0,1,2,3

// --- Multiple const declarations ---
const [ca, cb, cc] = [100, 200, 300];
console.log(ca + cb + cc);  // 600

// --- Const in function ---
function useConst(): number {
    const local = 42;
    const doubled = local * 2;
    return doubled;
}

console.log(useConst());  // 84

// --- String const comparison ---
function isRed(c: string): boolean {
    return c === Color.Red;
}

console.log(isRed(Color.Red));   // true
console.log(isRed(Color.Blue));  // false

// --- Const object with methods pattern ---
const MathUtils = {
    square: (x: number) => x * x,
    cube: (x: number) => x * x * x,
    abs: (x: number) => x < 0 ? -x : x
};

console.log(MathUtils.square(5));  // 25
console.log(MathUtils.cube(3));    // 27
console.log(MathUtils.abs(-7));    // 7

// --- Frozen-like const pattern ---
const CONFIG = {
    maxRetries: 3,
    timeout: 5000,
    baseUrl: "https://api.example.com"
} as const;

console.log(CONFIG.maxRetries);  // 3
console.log(CONFIG.timeout);     // 5000
console.log(CONFIG.baseUrl);     // https://api.example.com

// --- Const with conditional ---
const THRESHOLD = 50;
function classify(n: number): string {
    if (n < THRESHOLD) return "low";
    return "high";
}

console.log(classify(30));  // low
console.log(classify(70));  // high
