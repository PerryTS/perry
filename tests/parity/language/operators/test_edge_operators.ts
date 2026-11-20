// Edge-case tests for operators: arithmetic, bitwise, comparison, logical,
// assignment, typeof, instanceof, nullish coalescing, optional chaining
// Targets bugs like: bitwise NOT wrapping, negative number equality,
// NaN-boxed INT32 vs f64 comparison

// --- Arithmetic basics ---
console.log(2 + 3);      // 5
console.log(10 - 4);     // 6
console.log(3 * 7);      // 21
console.log(15 / 4);     // 3.75
console.log(17 % 5);     // 2
console.log(2 ** 10);    // 1024

// --- Unary operators ---
console.log(-5);     // -5
console.log(-(-5));  // 5
console.log(+true);  // 1
console.log(+false); // 0

// --- Increment / Decrement ---
let inc = 5;
inc++;
console.log(inc);  // 6
inc--;
console.log(inc);  // 5

// Pre vs post (in expressions)
let pre = 5;
console.log(++pre);  // 6
console.log(pre);    // 6

let post = 5;
console.log(post++); // 5
console.log(post);   // 6

// --- Bitwise operators ---
console.log(5 & 3);    // 1
console.log(5 | 3);    // 7
console.log(5 ^ 3);    // 6
console.log(~0);        // -1
console.log(~5);        // -6
console.log(~(-1));     // 0
console.log(1 << 10);   // 1024
console.log(1024 >> 5); // 32
console.log(-1 >>> 0);  // 4294967295

// --- Bitwise NOT edge cases (bug: wrapping semantics) ---
console.log(~0xFF);         // -256
console.log(~0xFFFF);       // -65536
console.log(~0x7FFFFFFF);   // -2147483648
console.log(~(-2147483648)); // 2147483647

// --- Comparison operators ---
console.log(1 < 2);    // true
console.log(2 < 1);    // false
console.log(1 <= 1);   // true
console.log(1 > 2);    // false
console.log(2 > 1);    // true
console.log(1 >= 1);   // true

// --- Equality with negative numbers (bug: sign bit in NaN-boxing) ---
console.log(-1 === -1);   // true
console.log(-1 === 1);    // false
console.log(-0.5 === -0.5); // true
console.log(-5 < 0);      // true
console.log(-5 > -10);    // true

// --- Floating point comparison ---
console.log(0.1 + 0.2 === 0.30000000000000004);  // true
console.log(0.1 + 0.2 > 0.3);  // true (due to FP precision)

// --- Integer vs float comparison ---
const intVal = 5;
const floatVal = 5.0;
console.log(intVal === floatVal);  // true

// --- Compound assignment ---
let ca = 10;
ca += 5;
console.log(ca);  // 15
ca -= 3;
console.log(ca);  // 12
ca *= 2;
console.log(ca);  // 24
ca /= 4;
console.log(ca);  // 6
ca %= 4;
console.log(ca);  // 2
ca **= 3;
console.log(ca);  // 8

// --- Bitwise compound assignment ---
let ba = 0xFF;
ba &= 0x0F;
console.log(ba);  // 15
ba |= 0xF0;
console.log(ba);  // 255
ba ^= 0xFF;
console.log(ba);  // 0

// --- Logical operators ---
console.log(true && true);    // true
console.log(true && false);   // false
console.log(false || true);   // true
console.log(false || false);  // false
console.log(!true);           // false
console.log(!false);          // true

// --- Short-circuit: && returns first falsy or last truthy ---
console.log(1 && 2);        // 2
console.log(0 && 2);        // 0
console.log("" && "hello"); // (empty)
console.log("a" && "b");    // b

// --- Short-circuit: || returns first truthy or last falsy ---
console.log(1 || 2);        // 1
console.log(0 || 2);        // 2
console.log("" || "hello"); // hello
console.log("a" || "b");    // a

// --- Nullish coalescing (??) ---
const nc1: number | null = null;
const nc2: number | null = 42;
console.log(nc1 ?? 0);    // 0
console.log(nc2 ?? 0);    // 42

// Difference from ||: ?? only triggers on null/undefined, not falsy
const nc3 = 0;
console.log(nc3 ?? 99);   // 0 (0 is not null/undefined)
console.log(nc3 || 99);   // 99 (0 is falsy)

const nc4 = "";
console.log(nc4 ?? "default");  // (empty string, not null)
console.log(nc4 || "default");  // default (empty string is falsy)

// --- Optional chaining (?.) ---
interface DeepObj {
    a?: {
        b?: {
            c?: number;
        };
    };
}

const deep1: DeepObj = { a: { b: { c: 42 } } };
const deep2: DeepObj = { a: {} };
const deep3: DeepObj = {};

console.log(deep1.a?.b?.c);  // 42
console.log(deep2.a?.b?.c);  // undefined
console.log(deep3.a?.b?.c);  // undefined

// --- Optional chaining with method call ---
const maybeStr: string | null = "hello";
const maybeNull: string | null = null;
console.log(maybeStr?.toUpperCase());  // HELLO
console.log(maybeNull?.toUpperCase()); // undefined

// --- typeof results ---
console.log(typeof 42);          // number
console.log(typeof "hello");     // string
console.log(typeof true);        // boolean
console.log(typeof undefined);   // undefined
console.log(typeof null);        // object
console.log(typeof []);          // object
console.log(typeof {});          // object

// --- Operator precedence ---
console.log(2 + 3 * 4);       // 14
console.log((2 + 3) * 4);     // 20
console.log(2 ** 3 ** 2);     // 512 (right-associative: 2^(3^2) = 2^9)
console.log(true || false && false);  // true (&& has higher precedence)

// --- Numeric edge cases ---
console.log(Number.MAX_SAFE_INTEGER);  // 9007199254740991
console.log(Number.MIN_SAFE_INTEGER);  // -9007199254740991
console.log(Infinity + 1);      // Infinity
console.log(-Infinity - 1);     // -Infinity
console.log(Infinity - Infinity); // NaN
console.log(1 / 0);              // Infinity
console.log(-1 / 0);             // -Infinity
console.log(0 / 0);              // NaN
console.log(isNaN(NaN));         // true
console.log(isNaN(42));          // false
console.log(isFinite(42));       // true
console.log(isFinite(Infinity)); // false

// --- Math functions ---
console.log(Math.abs(-42));       // 42
console.log(Math.floor(3.7));     // 3
console.log(Math.ceil(3.2));      // 4
console.log(Math.round(3.5));     // 4
console.log(Math.round(3.4));     // 3
console.log(Math.max(1, 5, 3));   // 5
console.log(Math.min(1, 5, 3));   // 1
console.log(Math.pow(2, 10));     // 1024
console.log(Math.sqrt(144));      // 12
console.log(Math.trunc(3.9));     // 3
console.log(Math.trunc(-3.9));    // -3
console.log(Math.sign(42));       // 1
console.log(Math.sign(-42));      // -1
console.log(Math.sign(0));        // 0

// --- String to number coercion ---
console.log(Number("42"));       // 42
console.log(Number("3.14"));     // 3.14
console.log(Number(""));         // 0
console.log(Number("abc"));      // NaN
console.log(parseInt("42"));     // 42
console.log(parseInt("42.9"));   // 42
console.log(parseInt("0xFF", 16)); // 255
console.log(parseFloat("3.14")); // 3.14

// --- Comma operator in for loop ---
let commaResult = 0;
for (let i = 0, j = 10; i < 5; i++, j--) {
    commaResult = commaResult + i + j;
}
console.log(commaResult);  // 50
