// Test string methods: trim, toLowerCase, toUpperCase
let s: string = "  Hello World  ";

// Test trim
let trimmed = s.trim();
console.log(trimmed);           // "Hello World" (no leading/trailing spaces)

// Test toLowerCase
let lower = s.toLowerCase();
console.log(lower);             // "  hello world  "

// Test toUpperCase
let upper = s.toUpperCase();
console.log(upper);             // "  HELLO WORLD  "

console.log("String methods test passed!");
