// Edge-case tests for rest parameters, spread operator, and default values
// Targets bugs like: cross-module default array parameter, optional params
// with imported_func_param_counts, spread in various contexts

// --- Rest parameters ---
function sum(...nums: number[]): number {
    let total = 0;
    for (let i = 0; i < nums.length; i++) {
        total = total + nums[i];
    }
    return total;
}

console.log(sum(1));           // 1
console.log(sum(1, 2));       // 3
console.log(sum(1, 2, 3));   // 6
console.log(sum(1, 2, 3, 4, 5));  // 15

// --- Rest with leading params ---
function format(prefix: string, ...parts: string[]): string {
    return prefix + ": " + parts.join(", ");
}

console.log(format("Items", "a", "b", "c"));  // Items: a, b, c
console.log(format("Single", "only"));          // Single: only

// --- Default parameters ---
function greet(name: string, greeting: string = "Hello"): string {
    return greeting + ", " + name + "!";
}

console.log(greet("World"));           // Hello, World!
console.log(greet("World", "Hi"));     // Hi, World!

// --- Default with expression ---
function createId(prefix: string = "id", num: number = 0): string {
    return prefix + "-" + num.toString();
}

console.log(createId());            // id-0
console.log(createId("user"));      // user-0
console.log(createId("user", 42));  // user-42

// --- Default array parameter (regression: SIGSEGV) ---
function processItems(items: number[] = []): number {
    let sum = 0;
    for (let i = 0; i < items.length; i++) {
        sum = sum + items[i];
    }
    return sum;
}

console.log(processItems());           // 0
console.log(processItems([1, 2, 3]));  // 6

// --- Default object parameter ---
function configure(opts: { debug: boolean; verbose: boolean } = { debug: false, verbose: false }): string {
    return (opts.debug ? "debug" : "") + (opts.verbose ? "verbose" : "");
}

console.log(configure());                               // (empty)
console.log(configure({ debug: true, verbose: false })); // debug
console.log(configure({ debug: true, verbose: true }));  // debugverbose

// --- Spread in array literal ---
const arr1 = [1, 2, 3];
const arr2 = [0, ...arr1, 4, 5];
console.log(arr2.join(","));  // 0,1,2,3,4,5

// --- Spread combining arrays ---
const left = [1, 2];
const right = [3, 4];
const combined = [...left, ...right];
console.log(combined.join(","));  // 1,2,3,4

// --- Spread in function call ---
function add3(a: number, b: number, c: number): number {
    return a + b + c;
}

const args: [number, number, number] = [10, 20, 30];
console.log(add3(...args));  // 60

// --- Spread in object literal ---
const base = { x: 1, y: 2 };
const extended = { ...base, z: 3 };
console.log(extended.x);  // 1
console.log(extended.y);  // 2
console.log(extended.z);  // 3

// --- Spread override ---
const defaults = { color: "red", size: 10, bold: false };
const custom = { ...defaults, color: "blue", bold: true };
console.log(custom.color);  // blue
console.log(custom.size);   // 10
console.log(custom.bold);   // true

// --- Array copy via spread ---
const original = [1, 2, 3];
const copy = [...original];
copy.push(4);
console.log(original.length);  // 3 (not modified)
console.log(copy.length);      // 4

// --- Rest in destructuring ---
const [first, ...remaining] = [10, 20, 30, 40];
console.log(first);                // 10
console.log(remaining.join(","));  // 20,30,40

// --- Object rest in destructuring ---
const { a: da, ...others } = { a: 1, b: 2, c: 3 };
console.log(da);  // 1
console.log(others.b);  // 2
console.log(others.c);  // 3

// --- Default with boolean ---
function toggle(flag: boolean = false): string {
    return flag ? "on" : "off";
}

console.log(toggle());      // off
console.log(toggle(true));  // on

// --- Multiple defaults ---
function range(start: number = 0, end: number = 10, step: number = 1): number[] {
    const result: number[] = [];
    for (let i = start; i < end; i = i + step) {
        result.push(i);
    }
    return result;
}

console.log(range().join(","));           // 0,1,2,3,4,5,6,7,8,9
console.log(range(5).join(","));          // 5,6,7,8,9
console.log(range(0, 5).join(","));       // 0,1,2,3,4
console.log(range(0, 10, 3).join(","));   // 0,3,6,9

// --- Rest params with zero args ---
function joinAll(...parts: string[]): string {
    return parts.join("-");
}

console.log(joinAll());                    // (empty)
console.log(joinAll("a"));                 // a
console.log(joinAll("a", "b", "c"));       // a-b-c

// --- Spread string into array ---
const chars = [..."hello"];
console.log(chars.join(","));  // h,e,l,l,o
console.log(chars.length);     // 5

// --- Default parameter using previous parameter ---
function createRange(start: number, end: number = start + 10): string {
    return start.toString() + "-" + end.toString();
}

console.log(createRange(5));      // 5-15
console.log(createRange(5, 20));  // 5-20

// --- Nested spread ---
const matrix = [[1, 2], [3, 4]];
const flat = [...matrix[0], ...matrix[1]];
console.log(flat.join(","));  // 1,2,3,4

// --- Rest in arrow function ---
const sumArrow = (...nums: number[]): number => {
    let total = 0;
    for (let i = 0; i < nums.length; i++) {
        total = total + nums[i];
    }
    return total;
};

console.log(sumArrow(1, 2, 3));  // 6
