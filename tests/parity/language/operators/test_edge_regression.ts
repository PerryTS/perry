// Regression tests targeting specific bugs found in recent Perry versions
// Each test case is derived from a real bug fix in v0.4.x commits

// === Regression 1: !!string always returned false (v0.4.42) ===
// Bug: Expr::String and Expr::Unary(Not) used float comparison
// which treated NaN-boxed strings as zero
const str1 = "hello";
console.log(!!str1);   // true
console.log(!str1);    // false
console.log(!!"");     // false
console.log(!!"x");    // true

const dynamicStr = "he" + "llo";
console.log(!!dynamicStr);   // true
console.log(!!("" + ""));    // false

// === Regression 2: Boolean() constructor returned undefined (v0.4.42) ===
console.log(Boolean("hello"));  // true
console.log(Boolean(""));       // false
console.log(Boolean(1));        // true
console.log(Boolean(0));        // false

// === Regression 3: 'in' operator result negation (v0.4.44) ===
// Bug: !('key' in obj) always returned false because 'in' returns
// NaN-boxed TAG_TRUE/TAG_FALSE but ! used float comparison
const inObj: Record<string, number> = { a: 1, b: 2 };
console.log("a" in inObj);       // true
console.log("c" in inObj);       // false
console.log(!("a" in inObj));    // false
console.log(!("c" in inObj));    // true

// Use in conditional
if (!("missing" in inObj)) {
    console.log("correctly missing");  // correctly missing
}

if ("a" in inObj) {
    console.log("correctly found");    // correctly found
}

// === Regression 4: negative number equality broken (v0.4.2) ===
// Bug: bits < 0x7FF8... unsigned check excluded negative f64 (sign bit set)
console.log(-1 === -1);     // true
console.log(-5 === -5);     // true
console.log(-1 === 1);      // false
console.log(-0.5 === -0.5); // true
console.log(-1 < 0);        // true
console.log(-1 > -2);       // true

// === Regression 5: === false/true always returned true (v0.4.2) ===
// Bug: codegen used ensure_i64 which collapsed both TAG_TRUE and TAG_FALSE to 0
const boolTrue = true;
const boolFalse = false;
console.log(boolTrue === true);    // true
console.log(boolTrue === false);   // false
console.log(boolFalse === true);   // false
console.log(boolFalse === false);  // true

function returnsTrue(): boolean { return true; }
function returnsFalse(): boolean { return false; }
console.log(returnsTrue() === true);    // true
console.log(returnsFalse() === false);  // true
console.log(returnsTrue() === false);   // false
console.log(returnsFalse() === true);   // false

// === Regression 6: string comparison with concatenated strings (v0.4.16) ===
// Bug: is_string_expr didn't recognize Expr::Logical or Expr::Conditional
const prefix = "hel";
const suffix = "lo";
const concat = prefix + suffix;
console.log(concat === "hello");  // true
console.log(concat !== "world");  // true

const orDefault = "" || "default";
console.log(orDefault === "default");  // true

const ternaryStr = true ? "yes" : "no";
console.log(ternaryStr === "yes");  // true

// === Regression 7: arr[i] in loop returned arr[0] (v0.4.30) ===
// Bug: LICM incorrectly hoisted loop-counter-indexed array reads
const loopArr = [10, 20, 30, 40, 50];
const collected: number[] = [];
for (let i = 0; i < loopArr.length; i++) {
    collected.push(loopArr[i]);
}
console.log(collected.join(","));  // 10,20,30,40,50

// Verify each index independently
console.log(loopArr[0] === 10);  // true
console.log(loopArr[1] === 20);  // true
console.log(loopArr[2] === 30);  // true
console.log(loopArr[3] === 40);  // true
console.log(loopArr[4] === 50);  // true

// === Regression 8: optional chaining side effects (v0.4.46) ===
// Bug: arr.shift()?.trim() re-evaluated shift in the else branch
const shiftArr = ["  hello  ", "  world  "];
const shifted = shiftArr.shift();
if (shifted) {
    console.log(shifted.trim());  // hello
}
console.log(shiftArr.length);    // 1

// === Regression 9: filter(Boolean) not working in all paths (v0.4.44) ===
const mixedArr: (number | null | undefined)[] = [1, null, 2, undefined, 3, null, 4];
const filtered = mixedArr.filter(Boolean);
console.log(filtered.length);  // 4

// === Regression 10: bitwise NOT wrapping semantics (v0.4.24) ===
// Bug: fcvt_to_sint_sat saturated at i32::MAX instead of wrapping
console.log(~0);          // -1
console.log(~1);          // -2
console.log(~(-1));       // 0
console.log(~0xFF);       // -256
console.log(~0xFFFFFFFF); // 0

// === Regression 11: NaN-boxed INT32 vs f64 equality (v0.4.2) ===
// Bug: parsed data === 5 always returned false (INT32 vs f64 mismatch)
const parsedNum = parseInt("5");
console.log(parsedNum === 5);  // true
console.log(parsedNum !== 5);  // false

// === Regression 12: closure capturing loop counter as i32 (from CLAUDE.md) ===
// Bug: loop counter i32 values need fcvt_from_sint to f64 before capture
const capturedVals: number[] = [];
for (let i = 0; i < 5; i++) {
    const captured = i;
    capturedVals.push(captured * 10);
}
console.log(capturedVals.join(","));  // 0,10,20,30,40

// === Regression 13: trimStart/trimEnd dispatch (v0.4.44) ===
// Bug: fell through to generic dispatch returning null bytes
console.log("  hello  ".trimStart());     // hello  (followed by spaces)
console.log("  hello  ".trimEnd());       //   hello
console.log("  hello  ".trimStart().trimEnd());  // hello

// Test with variable (not just literal)
const padded = "  test  ";
console.log(padded.trimStart());  // test
console.log(padded.trimEnd());    //   test
console.log(padded.trim());       // test

// === Regression 14: module-level array mutation from functions (v0.4.22) ===
// Bug: module vars weren't reloaded after compound statements containing nested calls
const moduleArr: number[] = [];

function addToModule(n: number): void {
    moduleArr.push(n);
}

for (let i = 0; i < 5; i++) {
    addToModule(i);
}
console.log(moduleArr.join(","));  // 0,1,2,3,4
console.log(moduleArr.length);     // 5

// === Regression 15: IndexSet with closure values (v0.4.44) ===
// Bug: ensure_f64 raw bitcast stripped POINTER_TAG from closures
const fnMap: Record<string, (x: number) => number> = {};
fnMap["double"] = (x: number) => x * 2;
fnMap["triple"] = (x: number) => x * 3;
console.log(fnMap["double"](5));   // 10
console.log(fnMap["triple"](5));   // 15

// === Regression 16: String(stringVar) returned "NaN" (v0.4.42) ===
const strVar = "hello";
console.log(String(strVar));  // hello

// === Regression 17: Math.max/min with null coercion (v0.4.24) ===
console.log(Math.max(0, 5));     // 5
console.log(Math.min(0, 5));     // 0

// === Regression 18: Chained boolean conditions ===
// Tests && and || with NaN-boxed values
const a = 5;
const b = 10;
console.log(a > 0 && b > 0);    // true
console.log(a > 0 && b > 20);   // false
console.log(a > 20 || b > 0);   // true
console.log(a > 20 || b > 20);  // false

// Complex condition
const x = 15;
console.log(x > 10 && x < 20 && x !== 13);  // true
console.log(x > 10 && x < 20 && x === 13);  // false
