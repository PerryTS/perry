// === Phase 0: Bug fixes ===
let sum = 0;
for (let i = 0; i < 10; i++) { sum = sum + i; }
console.log(sum);              // 45
console.log(7 % 3);            // 1
console.log(5 | 3);            // 7
console.log(5 & 3);            // 1
console.log(5 ^ 3);            // 6
console.log(1 << 3);           // 8
console.log(0 ?? "default");   // 0
console.log(null ?? "fallback"); // fallback

// === Phase 1: Objects ===
const obj = { name: "Perry", version: 2, active: true };
console.log(obj.name);         // Perry
console.log(obj.version);      // 2

// === Phase 1: Arrays ===
const arr = [10, 20, 30, 40, 50];
console.log(arr.length);       // 5
console.log(arr.join("-"));    // 10-20-30-40-50

// === Phase 1: String methods ===
console.log("Hello".toUpperCase());   // HELLO
console.log("WORLD".toLowerCase());   // world
console.log("  spaces  ".trim());     // spaces
console.log("foobar".includes("bar")); // true
console.log("hello".startsWith("hel")); // true
console.log("hello".endsWith("llo"));   // true
console.log("abcabc".replace("abc", "XYZ")); // XYZabc

// === Phase 2: Closures ===
const multiply = (a: number, b: number) => a * b;
console.log(multiply(6, 7));   // 42

// === Phase 2: Higher-order array methods ===
const nums = [1, 2, 3, 4, 5];
const evens = nums.filter((n: number) => n % 2 === 0);
console.log(evens.join(", ")); // 2, 4

const doubled = nums.map((n: number) => n * 2);
console.log(doubled.join(", ")); // 2, 4, 6, 8, 10

const total = nums.reduce((acc: number, n: number) => acc + n, 0);
console.log(total);            // 15

// === Phase 3: Switch ===
function greet(lang: string): string {
  switch (lang) {
    case "en": return "Hello";
    case "es": return "Hola";
    case "fr": return "Bonjour";
    default: return "Hi";
  }
}
console.log(greet("es"));     // Hola
console.log(greet("fr"));     // Bonjour
console.log(greet("de"));     // Hi

// === Phase 4: Math ===
console.log(Math.floor(3.7));  // 3
console.log(Math.ceil(3.2));   // 4
console.log(Math.abs(-5));     // 5
console.log(Math.pow(2, 10));  // 1024

// === Fibonacci ===
function fibonacci(n: number): number {
  if (n <= 1) return n;
  return fibonacci(n - 1) + fibonacci(n - 2);
}
console.log(fibonacci(10));    // 55
