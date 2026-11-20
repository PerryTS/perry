// Edge-case tests for truthiness, boolean coercion, and equality
// Tests: NaN-boxing booleans, !! operator, Boolean(), ===, ==, typeof, falsy values
// These target bugs like: !!string always false, Boolean() returning undefined,
// 'in' operator result not recognized as NaN-boxed boolean

// --- Falsy values ---
console.log(!false);      // true
console.log(!0);          // true
console.log(!"");         // true
console.log(!null);       // true
console.log(!undefined);  // true

// --- Truthy values ---
console.log(!true);       // false
console.log(!1);          // false
console.log(!"hello");    // false
console.log(!42);         // false
console.log(!-1);         // false

// --- Double negation (!! coercion) ---
console.log(!!true);       // true
console.log(!!false);      // false
console.log(!!1);          // true
console.log(!!0);          // false
console.log(!!"hello");    // true
console.log(!!"");         // false
console.log(!!null);       // false
console.log(!!undefined);  // false
console.log(!!-1);         // true
console.log(!!0.1);        // true

// --- Boolean() constructor ---
console.log(Boolean(true));       // true
console.log(Boolean(false));      // false
console.log(Boolean(1));          // true
console.log(Boolean(0));          // false
console.log(Boolean("hello"));    // true
console.log(Boolean(""));         // false
console.log(Boolean(null));       // false
console.log(Boolean(undefined));  // false

// --- Strict equality (===) edge cases ---
console.log(1 === 1);           // true
console.log(1 === 2);           // false
console.log("a" === "a");       // true
console.log("a" === "b");       // false
console.log(true === true);     // true
console.log(true === false);    // false
console.log(null === null);     // true
console.log(undefined === undefined);  // true
console.log(null === undefined);       // false

// --- Strict inequality (!==) ---
console.log(1 !== 2);           // true
console.log(1 !== 1);           // false
console.log("a" !== "b");       // true
console.log("a" !== "a");       // false

// --- typeof operator ---
console.log(typeof 42);          // number
console.log(typeof "hello");     // string
console.log(typeof true);        // boolean
console.log(typeof undefined);   // undefined
console.log(typeof null);        // object
console.log(typeof {});          // object
console.log(typeof []);          // object

// --- Negating comparison results ---
const arr = [1, 2, 3];
const obj: Record<string, number> = { a: 1, b: 2 };
console.log("a" in obj);        // true
console.log("c" in obj);        // false
console.log(!("a" in obj));     // false
console.log(!("c" in obj));     // true

// --- Boolean in conditions ---
const x = 5;
if (x) {
    console.log("x is truthy");  // x is truthy
}

const empty = "";
if (!empty) {
    console.log("empty is falsy");  // empty is falsy
}

// --- Ternary with boolean coercion ---
const str = "hello";
console.log(str ? "yes" : "no");  // yes

const zero = 0;
console.log(zero ? "yes" : "no");  // no

// --- Equality with concatenated/computed strings ---
const a = "hel";
const b = "lo";
const combined = a + b;
console.log(combined === "hello");  // true
console.log(combined !== "world");  // true

// --- Boolean operations on comparison results ---
const n = 5;
console.log(n > 3 && n < 10);    // true
console.log(n > 3 && n < 4);     // false
console.log(n > 10 || n < 6);    // true
console.log(n > 10 || n < 4);    // false

// --- Nullish coalescing ---
const val1: string | null = null;
const val2: string | null = "found";
console.log(val1 ?? "default");  // default
console.log(val2 ?? "default");  // found

// --- Optional chaining with nullish ---
const obj2: { a?: { b?: number } } = { a: { b: 42 } };
console.log(obj2.a?.b);  // 42

const obj3: { a?: { b?: number } } = {};
console.log(obj3.a?.b);  // undefined

// --- Truthiness of negative numbers ---
console.log(!!(-1));   // true
console.log(!!(-0.5)); // true

// --- NaN is falsy ---
console.log(!!NaN);     // false
console.log(!NaN);      // true

// --- Infinity is truthy ---
console.log(!!Infinity);     // true
console.log(!!(-Infinity));  // true

// --- Comparison edge cases ---
console.log(0 === -0);     // true
console.log(NaN === NaN);  // false

// --- Chained comparisons with boolean intermediates ---
function isInRange(x: number, lo: number, hi: number): boolean {
    return x >= lo && x <= hi;
}
console.log(isInRange(5, 1, 10));   // true
console.log(isInRange(0, 1, 10));   // false
console.log(isInRange(10, 1, 10));  // true
console.log(isInRange(11, 1, 10));  // false

// --- Boolean as function return value ---
function isEven(n: number): boolean {
    return n % 2 === 0;
}
console.log(isEven(4));  // true
console.log(isEven(7));  // false

// --- Conditional assignment with || ---
const fallback = "" || "fallback";
console.log(fallback);  // fallback

const keep = "keep" || "fallback";
console.log(keep);  // keep

// --- === with variables (not just literals) ---
const s1 = "test";
const s2 = "te" + "st";
console.log(s1 === s2);  // true

// --- Boolean with if/else chains ---
function classify(n: number): string {
    if (n < 0) return "negative";
    else if (n === 0) return "zero";
    else if (n < 10) return "small";
    else return "large";
}
console.log(classify(-5));  // negative
console.log(classify(0));   // zero
console.log(classify(7));   // small
console.log(classify(100)); // large
