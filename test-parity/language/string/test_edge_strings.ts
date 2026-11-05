// Edge-case tests for string operations
// Targets bugs like: string comparison with concatenated strings, trimStart/trimEnd
// dispatch, replaceAll, template literals, string equality NaN-boxing

// --- Basic string operations ---
console.log("hello".length);        // 5
console.log("hello"[0]);            // h
console.log("hello"[4]);            // o
console.log("hello".charAt(1));     // e

// --- String concatenation ---
const a = "hello";
const b = " world";
console.log(a + b);          // hello world
console.log(a + " " + b);   // hello  world

// --- Template literals ---
const name = "World";
const age = 42;
console.log(`Hello ${name}`);               // Hello World
console.log(`Age: ${age}`);                  // Age: 42
console.log(`${name} is ${age} years old`);  // World is 42 years old
console.log(`Result: ${2 + 3}`);             // Result: 5

// --- String comparison (=== with computed strings) ---
const s1 = "hel" + "lo";
const s2 = "hello";
console.log(s1 === s2);     // true
console.log(s1 !== s2);     // false

const s3 = "abc";
const s4 = "def";
console.log(s3 === s4);     // false
console.log(s3 < s4);       // true

// --- trim, trimStart, trimEnd ---
console.log("  hello  ".trim());        // hello
console.log("  hello  ".trimStart());   // hello
console.log("  hello  ".trimEnd());     //   hello

// --- toUpperCase / toLowerCase ---
console.log("Hello World".toUpperCase());  // HELLO WORLD
console.log("Hello World".toLowerCase());  // hello world

// --- includes / startsWith / endsWith ---
console.log("hello world".includes("world"));      // true
console.log("hello world".includes("xyz"));         // false
console.log("hello world".startsWith("hello"));     // true
console.log("hello world".startsWith("world"));     // false
console.log("hello world".endsWith("world"));       // true
console.log("hello world".endsWith("hello"));       // false

// --- indexOf / lastIndexOf ---
console.log("hello world hello".indexOf("hello"));      // 0
console.log("hello world hello".lastIndexOf("hello"));   // 12
console.log("hello world".indexOf("xyz"));               // -1

// --- slice ---
console.log("hello world".slice(0, 5));    // hello
console.log("hello world".slice(6));       // world
console.log("hello world".slice(-5));      // world
console.log("hello world".slice(0, -6));   // hello

// --- substring ---
console.log("hello world".substring(0, 5));  // hello
console.log("hello world".substring(6));     // world

// --- split ---
console.log("a,b,c".split(",").join("|"));     // a|b|c
console.log("hello".split("").join("-"));       // h-e-l-l-o
console.log("one  two  three".split("  ").length);  // 3

// --- replace / replaceAll ---
console.log("hello world".replace("world", "there"));     // hello there
console.log("aabbaabb".replaceAll("aa", "xx"));            // xxbbxxbb

// --- repeat ---
console.log("ab".repeat(3));     // ababab
console.log("x".repeat(0));      // (empty string)
console.log("-".repeat(5));      // -----

// --- padStart / padEnd ---
console.log("5".padStart(3, "0"));   // 005
console.log("5".padEnd(3, "0"));     // 500
console.log("hi".padStart(5));       //    hi
console.log("hi".padEnd(5));         // hi

// --- String.fromCharCode ---
// (if supported)

// --- charCodeAt ---
console.log("A".charCodeAt(0));   // 65
console.log("a".charCodeAt(0));   // 97
console.log("0".charCodeAt(0));   // 48

// --- String conversion ---
console.log(String(42));        // 42
console.log(String(true));      // true
console.log(String(false));     // false
console.log(String(null));      // null
console.log(String(undefined)); // undefined

// --- toString ---
console.log((42).toString());       // 42
console.log((255).toString(16));    // ff
console.log((8).toString(2));       // 1000

// --- Concatenation with non-strings ---
console.log("value: " + 42);          // value: 42
console.log("flag: " + true);         // flag: true
console.log("list: " + [1, 2, 3]);    // list: 1,2,3

// --- Empty string edge cases ---
console.log("".length);           // 0
console.log("" === "");           // true
console.log("".trim());           // (empty)
console.log("".split("").length); // 0

// --- String comparison operators ---
console.log("a" < "b");    // true
console.log("b" < "a");    // false
console.log("abc" < "abd"); // true
console.log("abc" < "abc"); // false
console.log("abc" <= "abc"); // true

// --- Multi-line template literals ---
const multi = `line1
line2
line3`;
console.log(multi.split("\n").length);  // 3

// --- String used as object key ---
const key = "dynamic_key";
const obj: Record<string, number> = {};
obj[key] = 42;
console.log(obj[key]);        // 42
console.log(obj["dynamic_key"]); // 42

// --- Chained string methods ---
const chain = "  Hello, World!  ".trim().toLowerCase().replace("hello", "hi");
console.log(chain);  // hi, world!

// --- String from array join ---
const parts = ["hello", "world", "foo"];
console.log(parts.join(" "));   // hello world foo
console.log(parts.join(""));    // helloworldfoo
console.log(parts.join(", "));  // hello, world, foo

// --- String in ternary ---
const cond = true;
const str = cond ? "yes" : "no";
console.log(str);  // yes

// --- String equality with OR default ---
const maybeEmpty = "" || "default";
console.log(maybeEmpty);  // default

const notEmpty = "value" || "default";
console.log(notEmpty);  // value
