// Edge-case tests for closures and variable capture
// Tests: mutable captures, nested closures, loop captures, IIFE, closure identity

// --- Mutable capture in nested scope ---
function makeAccumulator(): (n: number) => number {
    let total = 0;
    return (n: number): number => {
        total = total + n;
        return total;
    };
}

const acc = makeAccumulator();
console.log(acc(5));   // 5
console.log(acc(3));   // 8
console.log(acc(10));  // 18

// --- Multiple closures sharing the same captured variable ---
function makeShared(): { inc: () => number; dec: () => number; get: () => number } {
    let value = 0;
    return {
        inc: () => { value = value + 1; return value; },
        dec: () => { value = value - 1; return value; },
        get: () => value
    };
}

const shared = makeShared();
console.log(shared.inc());  // 1
console.log(shared.inc());  // 2
console.log(shared.dec());  // 1
console.log(shared.get());  // 1

// --- Nested closures (closure returning closure) ---
function outer(x: number): (y: number) => (z: number) => number {
    return (y: number) => {
        return (z: number) => {
            return x + y + z;
        };
    };
}

console.log(outer(1)(2)(3));  // 6
console.log(outer(10)(20)(30));  // 60

// --- Closure capturing loop variable (classic JS pitfall) ---
const fns: Array<() => number> = [];
for (let i = 0; i < 5; i++) {
    const captured = i;
    fns.push(() => captured);
}
console.log(fns[0]());  // 0
console.log(fns[1]());  // 1
console.log(fns[2]());  // 2
console.log(fns[3]());  // 3
console.log(fns[4]());  // 4

// --- IIFE (Immediately Invoked Function Expression) ---
const iife_result = ((x: number) => x * x)(7);
console.log(iife_result);  // 49

// --- Closure over string values ---
function greetFactory(greeting: string): (name: string) => string {
    return (name: string) => greeting + " " + name;
}

const hello = greetFactory("Hello");
const hi = greetFactory("Hi");
console.log(hello("World"));  // Hello World
console.log(hi("there"));     // Hi there

// --- Closure capturing object reference ---
function makeObjTracker(): { add: (k: string, v: number) => void; sum: () => number } {
    const data: Record<string, number> = {};
    return {
        add: (k: string, v: number) => { data[k] = v; },
        sum: () => {
            let s = 0;
            const keys = Object.keys(data);
            for (let i = 0; i < keys.length; i++) {
                s = s + data[keys[i]];
            }
            return s;
        }
    };
}

const tracker = makeObjTracker();
tracker.add("a", 10);
tracker.add("b", 20);
tracker.add("c", 30);
console.log(tracker.sum());  // 60

// --- Closure as callback parameter ---
function applyTwice(fn: (x: number) => number, val: number): number {
    return fn(fn(val));
}

console.log(applyTwice((x: number) => x + 1, 0));    // 2
console.log(applyTwice((x: number) => x * 2, 3));    // 12

// --- Recursive closure (closure that calls itself via captured reference) ---
const factorial = (n: number): number => {
    if (n <= 1) return 1;
    return n * factorial(n - 1);
};
console.log(factorial(10));  // 3628800

// --- Closure capturing boolean and toggling ---
function makeToggle(): () => boolean {
    let state = false;
    return () => {
        state = !state;
        return state;
    };
}

const toggle = makeToggle();
console.log(toggle());  // true
console.log(toggle());  // false
console.log(toggle());  // true

// --- Higher-order: function returning function returning function ---
function adder(a: number): (b: number) => (c: number) => number {
    return (b: number) => (c: number) => a + b + c;
}

const add10 = adder(10);
const add10_20 = add10(20);
console.log(add10_20(30));  // 60
console.log(add10_20(5));   // 35

// --- Closure with default parameter ---
function makeMultiplier(factor: number = 2): (x: number) => number {
    return (x: number) => x * factor;
}

console.log(makeMultiplier()(5));   // 10
console.log(makeMultiplier(3)(5));  // 15

// --- Closure capturing array and mutating it ---
function makeStack(): { push: (v: number) => void; pop: () => number; size: () => number } {
    const items: number[] = [];
    return {
        push: (v: number) => { items.push(v); },
        pop: () => { const v = items[items.length - 1]; items.pop(); return v; },
        size: () => items.length
    };
}

const stack = makeStack();
stack.push(1);
stack.push(2);
stack.push(3);
console.log(stack.size());  // 3
console.log(stack.pop());   // 3
console.log(stack.size());  // 2
