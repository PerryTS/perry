// Edge-case tests for variable scoping, hoisting, shadowing,
// block scoping, function scoping, and temporal dead zone patterns

// --- Block scoping with let ---
let outer = 1;
{
    let inner = 2;
    console.log(outer);  // 1
    console.log(inner);  // 2
}
console.log(outer);  // 1

// --- Variable shadowing ---
let shadow = "outer";
{
    let shadow = "inner";
    console.log(shadow);  // inner
}
console.log(shadow);  // outer

// --- Nested block shadowing ---
let depth = "level0";
{
    let depth = "level1";
    {
        let depth = "level2";
        console.log(depth);  // level2
    }
    console.log(depth);  // level1
}
console.log(depth);  // level0

// --- For loop scoping ---
for (let i = 0; i < 3; i++) {
    // i is scoped to the loop
}
// i is not accessible here

// --- Function scope ---
function outerFn(): string {
    const x = "outer";

    function innerFn(): string {
        const x = "inner";
        return x;
    }

    return x + "-" + innerFn();
}

console.log(outerFn());  // outer-inner

// --- Closure over block-scoped variable ---
function makeClosures(): Array<() => number> {
    const result: Array<() => number> = [];
    for (let i = 0; i < 5; i++) {
        result.push(() => i);
    }
    return result;
}

const closures = makeClosures();
console.log(closures[0]());  // 0
console.log(closures[1]());  // 1
console.log(closures[4]());  // 4

// --- Shadowing in function parameters ---
const param = "global";

function usesParam(param: string): string {
    return param;
}

console.log(usesParam("local"));  // local
console.log(param);                // global

// --- Nested function access to outer variables ---
function counter(): () => number {
    let count = 0;
    return () => {
        count++;
        return count;
    };
}

const c = counter();
console.log(c());  // 1
console.log(c());  // 2
console.log(c());  // 3

// --- Multiple closures over same variable ---
function sharedState(): { get: () => number; set: (n: number) => void } {
    let state = 0;
    return {
        get: () => state,
        set: (n: number) => { state = n; }
    };
}

const s = sharedState();
console.log(s.get());   // 0
s.set(42);
console.log(s.get());   // 42

// --- Variable in if/else blocks ---
let ifVar = "before";
if (true) {
    ifVar = "modified";
}
console.log(ifVar);  // modified

let elseVar = "before";
if (false) {
    elseVar = "if-branch";
} else {
    elseVar = "else-branch";
}
console.log(elseVar);  // else-branch

// --- Const in block scope ---
{
    const blockConst = 42;
    console.log(blockConst);  // 42
}

// --- Function declaration scoping ---
function outerFunc(): number {
    function helper(): number {
        return 10;
    }
    return helper() + 5;
}

console.log(outerFunc());  // 15

// --- Nested function with outer variable mutation ---
function mutateOuter(): number {
    let x = 0;

    function addToX(n: number): void {
        x = x + n;
    }

    addToX(5);
    addToX(10);
    addToX(3);
    return x;
}

console.log(mutateOuter());  // 18

// --- Variable in try/catch scope ---
try {
    const tryVar = "in try";
    console.log(tryVar);  // in try
} catch (e) {
    // tryVar not accessible here
}

// --- Loop variable shadowing ---
let loopShadow = "outer";
for (let loopShadow = 0; loopShadow < 3; loopShadow++) {
    // loopShadow is number here
}
console.log(loopShadow);  // outer (string)

// --- Complex scoping with closures and loops ---
function complexScope(): string[] {
    const results: string[] = [];

    for (let i = 0; i < 3; i++) {
        const prefix = "item" + i.toString();
        for (let j = 0; j < 2; j++) {
            const suffix = "_" + j.toString();
            results.push(prefix + suffix);
        }
    }

    return results;
}

console.log(complexScope().join(","));  // item0_0,item0_1,item1_0,item1_1,item2_0,item2_1

// --- Function parameter default using outer scope ---
const defaultVal = 42;
function withDefault(x: number = defaultVal): number {
    return x;
}

console.log(withDefault());    // 42
console.log(withDefault(10));  // 10

// --- Immediately invoked with scope ---
const iifeResult = (() => {
    const local = 100;
    return local + 1;
})();
console.log(iifeResult);  // 101

// --- Recursive function in scope ---
function scopedRecursion(): number {
    function fib(n: number): number {
        if (n <= 1) return n;
        return fib(n - 1) + fib(n - 2);
    }
    return fib(10);
}

console.log(scopedRecursion());  // 55

// --- Variable capture timing ---
function captureTest(): Array<() => number> {
    const fns: Array<() => number> = [];
    let x = 0;

    fns.push(() => x);
    x = 10;
    fns.push(() => x);
    x = 20;
    fns.push(() => x);

    return fns;
}

const captured = captureTest();
// All closures see the final value of x
console.log(captured[0]());  // 20
console.log(captured[1]());  // 20
console.log(captured[2]());  // 20

// --- Nested scope with same variable names ---
function nestedSame(): string {
    let result = "";
    const x = "a";
    result = result + x;

    {
        const x = "b";
        result = result + x;

        {
            const x = "c";
            result = result + x;
        }

        result = result + x;
    }

    result = result + x;
    return result;
}

console.log(nestedSame());  // abcba
