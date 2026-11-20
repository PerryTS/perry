// Edge-case tests for control flow: loops, switch, labeled breaks,
// try/catch/finally, early returns, nested conditions

// --- Nested for loops with break ---
let found = "";
for (let i = 0; i < 5; i++) {
    for (let j = 0; j < 5; j++) {
        if (i === 2 && j === 3) {
            found = i.toString() + "," + j.toString();
            break;
        }
    }
    if (found !== "") break;
}
console.log(found);  // 2,3

// --- Labeled break (break outer loop from inner) ---
let labelResult = "";
outer:
for (let i = 0; i < 5; i++) {
    for (let j = 0; j < 5; j++) {
        if (i * j > 6) {
            labelResult = i.toString() + "*" + j.toString();
            break outer;
        }
    }
}
console.log(labelResult);  // 2*4

// --- Continue in nested loops ---
const skipResult: string[] = [];
for (let i = 0; i < 3; i++) {
    for (let j = 0; j < 3; j++) {
        if (j === 1) continue;
        skipResult.push(i.toString() + j.toString());
    }
}
console.log(skipResult.join(","));  // 00,02,10,12,20,22

// --- Labeled continue ---
const labelCont: string[] = [];
outer2:
for (let i = 0; i < 3; i++) {
    for (let j = 0; j < 3; j++) {
        if (j === 1) continue outer2;
        labelCont.push(i.toString() + j.toString());
    }
}
console.log(labelCont.join(","));  // 00,10,20

// --- While loop with complex condition ---
let whileCount = 0;
let whileVal = 1;
while (whileVal < 100 && whileCount < 10) {
    whileVal = whileVal * 2;
    whileCount++;
}
console.log(whileVal);    // 128
console.log(whileCount);  // 7

// --- Do-while loop ---
let doWhileVal = 0;
do {
    doWhileVal++;
} while (doWhileVal < 5);
console.log(doWhileVal);  // 5

// --- Do-while executes at least once ---
let doOnce = 0;
do {
    doOnce++;
} while (false);
console.log(doOnce);  // 1

// --- Switch with fallthrough ---
function switchTest(x: number): string {
    let result = "";
    switch (x) {
        case 1:
            result = "one";
            break;
        case 2:
        case 3:
            result = "two-or-three";
            break;
        case 4:
            result = "four";
            break;
        default:
            result = "other";
    }
    return result;
}

console.log(switchTest(1));  // one
console.log(switchTest(2));  // two-or-three
console.log(switchTest(3));  // two-or-three
console.log(switchTest(4));  // four
console.log(switchTest(5));  // other

// --- Switch with string ---
function switchString(s: string): number {
    switch (s) {
        case "a": return 1;
        case "b": return 2;
        case "c": return 3;
        default: return -1;
    }
}

console.log(switchString("a"));  // 1
console.log(switchString("b"));  // 2
console.log(switchString("c"));  // 3
console.log(switchString("d"));  // -1

// --- Nested switch ---
function nestedSwitch(a: number, b: number): string {
    switch (a) {
        case 1:
            switch (b) {
                case 1: return "1,1";
                case 2: return "1,2";
                default: return "1,?";
            }
        case 2:
            return "2,*";
        default:
            return "?,*";
    }
}

console.log(nestedSwitch(1, 1));  // 1,1
console.log(nestedSwitch(1, 2));  // 1,2
console.log(nestedSwitch(1, 9));  // 1,?
console.log(nestedSwitch(2, 1));  // 2,*
console.log(nestedSwitch(9, 9));  // ?,*

// --- Try/catch with different error types ---
function safeDivide(a: number, b: number): string {
    try {
        if (b === 0) throw new Error("division by zero");
        return (a / b).toString();
    } catch (e: any) {
        return "error: " + e.message;
    }
}

console.log(safeDivide(10, 2));  // 5
console.log(safeDivide(10, 0));  // error: division by zero

// --- Try/catch/finally ---
function withFinally(shouldThrow: boolean): string {
    let log = "";
    try {
        log = log + "try,";
        if (shouldThrow) throw new Error("boom");
        log = log + "success,";
    } catch (e) {
        log = log + "catch,";
    } finally {
        log = log + "finally";
    }
    return log;
}

console.log(withFinally(false));  // try,success,finally
console.log(withFinally(true));   // try,catch,finally

// --- Nested try/catch ---
function nestedTry(): string {
    let log = "";
    try {
        log = log + "outer,";
        try {
            log = log + "inner,";
            throw new Error("inner error");
        } catch (e) {
            log = log + "inner-catch,";
        }
        log = log + "after-inner";
    } catch (e) {
        log = log + "outer-catch";
    }
    return log;
}

console.log(nestedTry());  // outer,inner,inner-catch,after-inner

// --- Early return ---
function earlyReturn(arr: number[]): number {
    for (let i = 0; i < arr.length; i++) {
        if (arr[i] < 0) return -1;
    }
    return 0;
}

console.log(earlyReturn([1, 2, 3]));      // 0
console.log(earlyReturn([1, -2, 3]));     // -1
console.log(earlyReturn([]));              // 0

// --- Multiple return paths ---
function multiReturn(x: number): string {
    if (x < 0) return "negative";
    if (x === 0) return "zero";
    if (x < 10) return "small";
    if (x < 100) return "medium";
    return "large";
}

console.log(multiReturn(-5));   // negative
console.log(multiReturn(0));    // zero
console.log(multiReturn(7));    // small
console.log(multiReturn(42));   // medium
console.log(multiReturn(999));  // large

// --- For loop with complex update ---
let complexFor = 0;
for (let i = 1; i <= 100; i = i * 2) {
    complexFor++;
}
console.log(complexFor);  // 7

// --- Nested if/else chains ---
function classify(a: number, b: number): string {
    if (a > 0) {
        if (b > 0) return "++";
        else if (b < 0) return "+-";
        else return "+0";
    } else if (a < 0) {
        if (b > 0) return "-+";
        else if (b < 0) return "--";
        else return "-0";
    } else {
        if (b > 0) return "0+";
        else if (b < 0) return "0-";
        else return "00";
    }
}

console.log(classify(1, 1));    // ++
console.log(classify(1, -1));   // +-
console.log(classify(-1, 1));   // -+
console.log(classify(-1, -1));  // --
console.log(classify(0, 0));    // 00
console.log(classify(0, 1));    // 0+

// --- Ternary chains ---
function ternaryChain(n: number): string {
    return n < 0 ? "neg" : n === 0 ? "zero" : n < 10 ? "small" : "big";
}

console.log(ternaryChain(-1));   // neg
console.log(ternaryChain(0));    // zero
console.log(ternaryChain(5));    // small
console.log(ternaryChain(100));  // big

// --- Short-circuit evaluation side effects ---
let sideEffect = 0;
function increment(): boolean {
    sideEffect++;
    return true;
}

false && increment();
console.log(sideEffect);  // 0 (short-circuited)

true && increment();
console.log(sideEffect);  // 1

true || increment();
console.log(sideEffect);  // 1 (short-circuited)

false || increment();
console.log(sideEffect);  // 2

// --- For...of with break ---
const forOfArr = [10, 20, 30, 40, 50];
let forOfSum = 0;
for (const val of forOfArr) {
    if (val > 30) break;
    forOfSum = forOfSum + val;
}
console.log(forOfSum);  // 60

// --- Deeply nested conditions ---
function deepNest(a: boolean, b: boolean, c: boolean, d: boolean): string {
    if (a) {
        if (b) {
            if (c) {
                if (d) return "all";
                return "abc";
            }
            return "ab";
        }
        return "a";
    }
    return "none";
}

console.log(deepNest(true, true, true, true));    // all
console.log(deepNest(true, true, true, false));   // abc
console.log(deepNest(true, true, false, false));  // ab
console.log(deepNest(true, false, false, false)); // a
console.log(deepNest(false, false, false, false)); // none
