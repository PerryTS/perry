// Edge-case tests for error handling: try/catch/finally, Error types,
// nested exceptions, throw in different contexts, error properties

// --- Basic throw and catch ---
try {
    throw new Error("test error");
} catch (e: any) {
    console.log(e.message);  // test error
}

// --- Error types ---
try {
    throw new TypeError("type error");
} catch (e: any) {
    console.log(e.message);  // type error
}

try {
    throw new RangeError("out of range");
} catch (e: any) {
    console.log(e.message);  // out of range
}

// --- Catch all and rethrow ---
function riskyOp(shouldFail: boolean): number {
    if (shouldFail) throw new Error("failure");
    return 42;
}

try {
    console.log(riskyOp(false));  // 42
} catch (e) {
    console.log("should not reach");
}

try {
    riskyOp(true);
    console.log("should not reach");
} catch (e: any) {
    console.log("caught: " + e.message);  // caught: failure
}

// --- Finally always runs ---
function withFinally(doThrow: boolean): string {
    let log = "";
    try {
        log = log + "try";
        if (doThrow) throw new Error("boom");
        log = log + "-ok";
    } catch (e) {
        log = log + "-catch";
    } finally {
        log = log + "-finally";
    }
    return log;
}

console.log(withFinally(false));  // try-ok-finally
console.log(withFinally(true));   // try-catch-finally

// --- Nested try/catch ---
function nestedErrors(): string {
    let log = "";
    try {
        log = log + "outer-try,";
        try {
            log = log + "inner-try,";
            throw new Error("inner");
        } catch (e: any) {
            log = log + "inner-catch(" + e.message + "),";
        } finally {
            log = log + "inner-finally,";
        }
        log = log + "after-inner";
    } catch (e: any) {
        log = log + "outer-catch(" + e.message + ")";
    }
    return log;
}

console.log(nestedErrors());
// outer-try,inner-try,inner-catch(inner),inner-finally,after-inner

// --- Error in catch block ---
function errorInCatch(): string {
    try {
        try {
            throw new Error("first");
        } catch (e) {
            throw new Error("second");
        }
    } catch (e: any) {
        return e.message;
    }
    return "unreachable";
}

console.log(errorInCatch());  // second

// --- Try/catch in loop ---
function tryInLoop(): number {
    let successes = 0;
    for (let i = 0; i < 5; i++) {
        try {
            if (i % 2 === 0) throw new Error("even");
            successes++;
        } catch (e) {
            // skip
        }
    }
    return successes;
}

console.log(tryInLoop());  // 2

// --- Error with custom data ---
function divideChecked(a: number, b: number): number {
    if (b === 0) {
        throw new Error("Division by zero: " + a.toString() + " / " + b.toString());
    }
    return a / b;
}

try {
    console.log(divideChecked(10, 2));   // 5
} catch (e) { /* */ }

try {
    divideChecked(10, 0);
} catch (e: any) {
    console.log(e.message);  // Division by zero: 10 / 0
}

// --- Multiple catch scenarios ---
function multiThrow(which: number): string {
    try {
        switch (which) {
            case 1: throw new Error("error one");
            case 2: throw new Error("error two");
            case 3: throw new Error("error three");
            default: return "no error";
        }
    } catch (e: any) {
        return "caught: " + e.message;
    }
}

console.log(multiThrow(1));  // caught: error one
console.log(multiThrow(2));  // caught: error two
console.log(multiThrow(3));  // caught: error three
console.log(multiThrow(4));  // no error

// --- Throw string (non-Error) ---
try {
    throw "raw string error";
} catch (e) {
    console.log(e);  // raw string error
}

// --- Throw number ---
try {
    throw 404;
} catch (e) {
    console.log(e);  // 404
}

// --- Error propagation through functions ---
function level3Err(): number {
    throw new Error("deep error");
}

function level2Err(): number {
    return level3Err();
}

function level1Err(): number {
    return level2Err();
}

try {
    level1Err();
} catch (e: any) {
    console.log(e.message);  // deep error
}

// --- Try/catch with return value ---
function safeParseInt(s: string): number {
    try {
        const n = parseInt(s);
        if (isNaN(n)) throw new Error("NaN");
        return n;
    } catch (e) {
        return -1;
    }
}

console.log(safeParseInt("42"));   // 42
console.log(safeParseInt("abc"));  // -1

// --- Try/catch around array operations ---
function safeGet(arr: number[], index: number): number {
    try {
        if (index < 0 || index >= arr.length) {
            throw new Error("index out of bounds");
        }
        return arr[index];
    } catch (e) {
        return -1;
    }
}

console.log(safeGet([10, 20, 30], 1));   // 20
console.log(safeGet([10, 20, 30], 5));   // -1
console.log(safeGet([10, 20, 30], -1));  // -1

// --- Finally with return (tricky behavior) ---
function finallyReturn(): string {
    let result = "start";
    try {
        result = result + "-try";
        return result;
    } finally {
        result = result + "-finally";
        // Note: in JS, finally runs before the return,
        // but the return value is already captured
    }
}

console.log(finallyReturn());  // start-try

// --- Error message concatenation ---
function validateAge(age: number): void {
    if (age < 0) throw new Error("Age cannot be negative: " + age.toString());
    if (age > 150) throw new Error("Age too large: " + age.toString());
}

try { validateAge(25); console.log("valid 25"); } catch (e: any) { console.log(e.message); }
// valid 25
try { validateAge(-5); } catch (e: any) { console.log(e.message); }
// Age cannot be negative: -5
try { validateAge(200); } catch (e: any) { console.log(e.message); }
// Age too large: 200
