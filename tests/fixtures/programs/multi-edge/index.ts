// Multi-module edge case tests: imports, cross-module calls, class usage,
// default parameters across modules, re-exports, module-level state
// Targets bugs like: cross-module default array param SIGSEGV, wrapper
// function linkage, exported function dispatch, module init order

import {
    add, multiply, greet, processItems,
    Counter, Shape, Circle, Rectangle,
    PI, MAX_SIZE, GREETING,
    identity, firstOrDefault, makeMultiplier, makePoint,
    sharedItems, getCallCount, range,
    isPositive, isEven
} from "./helpers.ts";

// --- Cross-module function calls ---
console.log(add(3, 4));        // 7
console.log(multiply(5, 6));   // 30

// --- Cross-module function with defaults ---
console.log(greet("World"));          // Hello, World!
console.log(greet("World", "Hi"));    // Hi, World!

// --- Cross-module default array parameter (regression) ---
console.log(processItems());           // 0
console.log(processItems([1, 2, 3]));  // 6

// --- Cross-module class instantiation ---
const counter = new Counter();
counter.increment();
counter.increment();
counter.increment();
console.log(counter.get());  // 3
counter.decrement();
console.log(counter.get());  // 2

// --- Cross-module class with default constructor param ---
const counter2 = new Counter(10);
console.log(counter2.get());  // 10

// --- Cross-module class inheritance ---
const circle = new Circle(5);
console.log(Math.round(circle.area()));  // 79
console.log(circle.kind);                // circle

const rect = new Rectangle(3, 4);
console.log(rect.area());    // 12
console.log(rect.kind);      // rect

// --- Cross-module polymorphism ---
const shapes: Shape[] = [new Circle(10), new Rectangle(5, 6)];
for (let i = 0; i < shapes.length; i++) {
    console.log(shapes[i].describe());
}
// circle:314.15926535897927 (approximately)
// rect:30

// --- Cross-module constants ---
console.log(PI > 3.14);     // true
console.log(MAX_SIZE);       // 100
console.log(GREETING);       // Hello World

// --- Cross-module generic functions ---
console.log(identity(42));       // 42
console.log(identity("hello"));  // hello

console.log(firstOrDefault([10, 20], 0));         // 10
console.log(firstOrDefault([] as number[], 99));   // 99

// --- Cross-module closure-returning function ---
const times3 = makeMultiplier(3);
const times7 = makeMultiplier(7);
console.log(times3(5));    // 15
console.log(times7(5));    // 35

// --- Cross-module object-returning function ---
const pt = makePoint(10, 20);
console.log(pt.x);  // 10
console.log(pt.y);  // 20

// --- Cross-module array access ---
console.log(sharedItems.length);      // 5
console.log(sharedItems[0]);          // 10
console.log(sharedItems[4]);          // 50
console.log(sharedItems.join(","));   // 10,20,30,40,50

// --- Cross-module state (call counter) ---
console.log(getCallCount());  // 1
console.log(getCallCount());  // 2
console.log(getCallCount());  // 3

// --- Cross-module function returning array ---
const r = range(1, 6);
console.log(r.join(","));  // 1,2,3,4,5

// --- Cross-module functions as callbacks ---
const nums = [1, 2, 3, 4, 5, 6, 7, 8];
const positiveEvens = nums.filter(isPositive).filter(isEven);
console.log(positiveEvens.join(","));  // 2,4,6,8

// --- Cross-module function reference in map ---
const arr = [-3, -1, 0, 2, 5];
const pos = arr.filter(isPositive);
console.log(pos.join(","));  // 2,5

// --- Complex cross-module composition ---
const data = range(1, 11);  // [1..10]
const evenSquares = data
    .filter(isEven)
    .map((n: number) => multiply(n, n));
console.log(evenSquares.join(","));  // 4,16,36,64,100

// --- Cross-module class in array ---
const counters: Counter[] = [];
for (let i = 0; i < 3; i++) {
    counters.push(new Counter(i * 10));
}
for (let i = 0; i < counters.length; i++) {
    counters[i].increment();
}
const counts = counters.map((c: Counter) => c.get());
console.log(counts.join(","));  // 1,11,21
