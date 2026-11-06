// Edge-case tests for higher-order functions, function composition,
// callbacks, function references, currying, and method references
// Targets bugs like: .map(Number), .filter(Boolean), function as value

// --- Function as first-class value ---
function add(a: number, b: number): number { return a + b; }
function mul(a: number, b: number): number { return a * b; }

function apply(fn: (a: number, b: number) => number, x: number, y: number): number {
    return fn(x, y);
}

console.log(apply(add, 3, 4));  // 7
console.log(apply(mul, 3, 4));  // 12

// --- Array of functions ---
const ops: Array<(x: number) => number> = [
    (x: number) => x + 1,
    (x: number) => x * 2,
    (x: number) => x - 3
];

let val = 10;
for (let i = 0; i < ops.length; i++) {
    val = ops[i](val);
}
console.log(val);  // (10+1)*2-3 = 19

// --- Function composition ---
function compose<A, B, C>(f: (b: B) => C, g: (a: A) => B): (a: A) => C {
    return (a: A) => f(g(a));
}

const double = (x: number) => x * 2;
const increment = (x: number) => x + 1;
const doubleInc = compose(double, increment);  // double(increment(x))
console.log(doubleInc(5));  // 12

const incDouble = compose(increment, double);  // increment(double(x))
console.log(incDouble(5));  // 11

// --- Currying ---
function curry(fn: (a: number, b: number) => number): (a: number) => (b: number) => number {
    return (a: number) => (b: number) => fn(a, b);
}

const curriedAdd = curry(add);
console.log(curriedAdd(3)(4));  // 7

const add5 = curriedAdd(5);
console.log(add5(10));  // 15
console.log(add5(20));  // 25

// --- Partial application ---
function partial(fn: (a: number, b: number) => number, a: number): (b: number) => number {
    return (b: number) => fn(a, b);
}

const mulBy3 = partial(mul, 3);
console.log(mulBy3(7));   // 21
console.log(mulBy3(10));  // 30

// --- Callback chains ---
function pipeline(value: number, fns: Array<(x: number) => number>): number {
    let result = value;
    for (let i = 0; i < fns.length; i++) {
        result = fns[i](result);
    }
    return result;
}

console.log(pipeline(5, [
    (x: number) => x * 2,
    (x: number) => x + 10,
    (x: number) => x / 4
]));  // (5*2+10)/4 = 5

// --- .map with function reference ---
function square(x: number): number { return x * x; }
const squared = [1, 2, 3, 4, 5].map(square);
console.log(squared.join(","));  // 1,4,9,16,25

// --- .filter with predicate function ---
function isPositive(x: number): boolean { return x > 0; }
const positives = [-2, -1, 0, 1, 2].filter(isPositive);
console.log(positives.join(","));  // 1,2

// --- .map(Number) pattern (built-in as callback) ---
const strNums = ["1", "2", "3"];
const nums = strNums.map(Number);
console.log(nums.join(","));  // 1,2,3

// --- .map(String) pattern ---
const numArr = [1, 2, 3];
const strArr = numArr.map(String);
console.log(strArr.join("-"));  // 1-2-3

// --- .filter(Boolean) pattern ---
const withFalsy: (number | null | undefined | string)[] = [1, null, 2, undefined, 0, "hello", ""];
const truthyOnly = withFalsy.filter(Boolean);
console.log(truthyOnly.length);  // 3

// --- Predicate factory ---
function greaterThan(threshold: number): (x: number) => boolean {
    return (x: number) => x > threshold;
}

const gt5 = greaterThan(5);
const gt10 = greaterThan(10);
console.log([3, 7, 12, 2, 15].filter(gt5).join(","));   // 7,12,15
console.log([3, 7, 12, 2, 15].filter(gt10).join(","));  // 12,15

// --- Memoization ---
function memoize(fn: (n: number) => number): (n: number) => number {
    const cache = new Map<number, number>();
    return (n: number): number => {
        if (cache.has(n)) {
            return cache.get(n)!;
        }
        const result = fn(n);
        cache.set(n, result);
        return result;
    };
}

let callCount = 0;
const expensiveSquare = memoize((n: number) => {
    callCount++;
    return n * n;
});

console.log(expensiveSquare(5));  // 25
console.log(expensiveSquare(5));  // 25 (cached)
console.log(expensiveSquare(3));  // 9
console.log(callCount);           // 2 (only 2 unique calls)

// --- Function returning different functions based on condition ---
function getOperation(op: string): (a: number, b: number) => number {
    switch (op) {
        case "+": return (a: number, b: number) => a + b;
        case "-": return (a: number, b: number) => a - b;
        case "*": return (a: number, b: number) => a * b;
        default: return (a: number, b: number) => a / b;
    }
}

console.log(getOperation("+")(10, 3));  // 13
console.log(getOperation("-")(10, 3));  // 7
console.log(getOperation("*")(10, 3));  // 30
console.log(getOperation("/")(10, 4));  // 2.5

// --- Reduce with function ---
function sumReducer(acc: number, x: number): number { return acc + x; }
console.log([1, 2, 3, 4, 5].reduce(sumReducer, 0));  // 15

// --- Sort with comparator function ---
function descending(a: number, b: number): number { return b - a; }
const sortedDesc = [3, 1, 4, 1, 5, 9].sort(descending);
console.log(sortedDesc.join(","));  // 9,5,4,3,1,1

// --- Chained higher-order operations ---
const data = [
    { name: "Alice", score: 95 },
    { name: "Bob", score: 42 },
    { name: "Carol", score: 88 },
    { name: "Dave", score: 67 },
    { name: "Eve", score: 91 }
];

const topScorers = data
    .filter((d: { name: string; score: number }) => d.score >= 80)
    .map((d: { name: string; score: number }) => d.name);
console.log(topScorers.join(","));  // Alice,Carol,Eve

const avgScore = data
    .map((d: { name: string; score: number }) => d.score)
    .reduce((a: number, b: number) => a + b, 0) / data.length;
console.log(Math.round(avgScore));  // 77

// --- Nested callbacks ---
function transform(arr: number[], ...fns: Array<(arr: number[]) => number[]>): number[] {
    let result = arr;
    for (let i = 0; i < fns.length; i++) {
        result = fns[i](result);
    }
    return result;
}

const result = transform(
    [1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
    (arr: number[]) => arr.filter((x: number) => x % 2 === 0),
    (arr: number[]) => arr.map((x: number) => x * x)
);
console.log(result.join(","));  // 4,16,36,64,100
