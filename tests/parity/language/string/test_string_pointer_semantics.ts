// Test string as i64 pointer: equality, comparison, passing through boundaries
// Critical for static typing migration: strings must work as raw pointers without NaN-boxing

// === String equality (content comparison, not pointer identity) ===
const s1 = "hello";
const s2 = "hel" + "lo";  // dynamically constructed
console.log(s1 === s2);   // true (content equal)
console.log(s1 !== s2);   // false

// === String comparison (relational) ===
console.log("apple" < "banana");  // true
console.log("zebra" > "apple");   // true
console.log("abc" <= "abc");      // true
console.log("abc" >= "abd");      // false

// === String in conditional ===
const empty = "";
const nonempty = "hi";
if (empty) {
  console.log("WRONG");
} else {
  console.log("empty is falsy");  // empty is falsy
}
if (nonempty) {
  console.log("nonempty is truthy"); // nonempty is truthy
}

// === String concatenation with different types ===
console.log("num: " + 42);        // num: 42
console.log("bool: " + true);     // bool: true
console.log("str: " + "hello");   // str: hello
console.log("null: " + null);     // null: null
console.log("undef: " + undefined); // undef: undefined

// === String methods preserve type ===
const upper: string = "hello".toUpperCase();
console.log(upper);              // HELLO
console.log(upper === "HELLO");  // true

const trimmed: string = "  hi  ".trim();
console.log(trimmed);            // hi
console.log(trimmed.length);     // 2

// === String passed to function and returned ===
function echo(s: string): string { return s; }
function greet(name: string): string { return "Hi, " + name + "!"; }

console.log(echo("test"));       // test
console.log(greet("Perry"));     // Hi, Perry!

// === String in template literals ===
const name = "world";
const num = 42;
console.log(`hello ${name}`);    // hello world
console.log(`value: ${num}`);    // value: 42

// === String comparison after method call ===
const input = "Hello World";
const lower: string = input.toLowerCase();
console.log(lower === "hello world"); // true

// === String in switch ===
function getLabel(code: string): string {
  if (code === "a") return "alpha";
  if (code === "b") return "beta";
  return "unknown";
}
console.log(getLabel("a")); // alpha
console.log(getLabel("b")); // beta
console.log(getLabel("c")); // unknown

// === String length and indexing ===
const str = "abcdef";
console.log(str.length);     // 6
console.log(str.charAt(0));  // a
console.log(str.charAt(5));  // f
console.log(str.slice(1, 4)); // bcd

// === String split and join round-trip ===
const csv = "one,two,three";
const parts = csv.split(",");
console.log(parts.length);   // 3
console.log(parts[0]);       // one
console.log(parts[2]);       // three
const rejoined = parts.join("-");
console.log(rejoined);       // one-two-three
