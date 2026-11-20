// Edge-case tests for type coercion: implicit conversions, explicit casts,
// string/number/boolean coercion, toString, valueOf-like behavior
// Targets bugs like: null arithmetic coercion, String(x) on string locals,
// concatenation with non-strings

// --- String + number coercion ---
console.log("value: " + 42);          // value: 42
console.log("count: " + 0);           // count: 0
console.log("negative: " + (-5));     // negative: -5
console.log("float: " + 3.14);        // float: 3.14

// --- String + boolean coercion ---
console.log("flag: " + true);    // flag: true
console.log("flag: " + false);   // flag: false

// --- String + null/undefined ---
console.log("val: " + null);       // val: null
console.log("val: " + undefined);  // val: undefined

// --- Number coercion ---
console.log(Number("42"));       // 42
console.log(Number("3.14"));     // 3.14
console.log(Number(""));         // 0
console.log(Number("abc"));      // NaN
console.log(Number(true));       // 1
console.log(Number(false));      // 0
console.log(Number(null));       // 0

// --- String coercion ---
console.log(String(42));        // 42
console.log(String(3.14));      // 3.14
console.log(String(true));      // true
console.log(String(false));     // false
console.log(String(null));      // null
console.log(String(undefined)); // undefined
console.log(String(0));         // 0
console.log(String(-0));        // 0
console.log(String(NaN));       // NaN
console.log(String(Infinity));  // Infinity

// --- Boolean coercion ---
console.log(Boolean(1));          // true
console.log(Boolean(0));          // false
console.log(Boolean(-1));         // true
console.log(Boolean(""));         // false
console.log(Boolean("hello"));    // true
console.log(Boolean(null));       // false
console.log(Boolean(undefined));  // false
console.log(Boolean(NaN));        // false
console.log(Boolean(Infinity));   // true
console.log(Boolean([]));         // true
console.log(Boolean({}));         // true

// --- toString on numbers ---
console.log((42).toString());       // 42
console.log((255).toString(16));    // ff
console.log((7).toString(2));       // 111
console.log((8).toString(8));       // 10
console.log((-42).toString());      // -42
console.log((0).toString());        // 0
console.log((3.14).toString());     // 3.14

// --- toFixed ---
console.log((42.567).toFixed(2));  // 42.57
console.log((42.567).toFixed(0));  // 43
console.log((42).toFixed(2));      // 42.00
console.log((0.1 + 0.2).toFixed(1));  // 0.3

// --- Template literal coercion ---
const num = 42;
const bool = true;
const nul = null;
console.log(`num: ${num}`);    // num: 42
console.log(`bool: ${bool}`);  // bool: true
console.log(`null: ${nul}`);   // null: null

// --- Arithmetic with booleans ---
console.log(true + true);    // 2
console.log(true + false);   // 1
console.log(false + false);  // 0

// --- Comparison with different types ---
console.log(1 == 1);        // true
console.log("1" == 1 as any);      // true (loose equality)
console.log(null == undefined);     // true
console.log(null == 0 as any);     // false
console.log("" == 0 as any);       // true
console.log("" == false as any);   // true

// --- parseInt edge cases ---
console.log(parseInt("42"));        // 42
console.log(parseInt("42.9"));      // 42
console.log(parseInt("0xFF", 16));  // 255
console.log(parseInt("111", 2));    // 7
console.log(parseInt("77", 8));     // 63
console.log(parseInt(""));          // NaN
console.log(parseInt("abc"));       // NaN

// --- parseFloat edge cases ---
console.log(parseFloat("3.14"));      // 3.14
console.log(parseFloat("3.14abc"));   // 3.14
console.log(parseFloat(".5"));        // 0.5
console.log(parseFloat(""));          // NaN

// --- Unary + for number conversion ---
console.log(+"42");     // 42
console.log(+"3.14");   // 3.14
console.log(+"");        // 0
console.log(+true);     // 1
console.log(+false);    // 0
console.log(+null);     // 0

// --- Array to string ---
console.log([1, 2, 3].toString());         // 1,2,3
console.log([1, 2, 3].join(","));          // 1,2,3
console.log(String([1, 2, 3]));            // 1,2,3
console.log("items: " + [1, 2, 3]);       // items: 1,2,3

// --- Concatenation building ---
let built = "";
built = built + "a";
built = built + "b";
built = built + "c";
console.log(built);  // abc

// --- Number from boolean in arithmetic ---
function boolToNum(b: boolean): number {
    return b ? 1 : 0;
}
console.log(boolToNum(true));   // 1
console.log(boolToNum(false));  // 0

// --- Coercion in comparisons ---
function safeCompare(a: number | null, b: number | null): number {
    const va = a !== null ? a : 0;
    const vb = b !== null ? b : 0;
    return va - vb;
}

console.log(safeCompare(5, 3));     // 2
console.log(safeCompare(null, 3));  // -3
console.log(safeCompare(5, null));  // 5
console.log(safeCompare(null, null)); // 0

// --- String representation of special values ---
console.log(NaN.toString());       // NaN
console.log(Infinity.toString());  // Infinity

// --- Mixed type array to string ---
const mixedArr: any[] = [1, "two", true, null];
console.log(mixedArr.join("-"));  // 1-two-true-

// --- Number formatting patterns ---
function formatCurrency(amount: number): string {
    return "$" + amount.toFixed(2);
}

console.log(formatCurrency(42));       // $42.00
console.log(formatCurrency(9.99));     // $9.99
console.log(formatCurrency(0.5));      // $0.50
console.log(formatCurrency(1234.5));   // $1234.50
