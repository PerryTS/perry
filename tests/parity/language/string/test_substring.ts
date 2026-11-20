// Test string.substring
let s: string = "Hello World";

// substring(start, end) - similar to slice but with slightly different behavior
console.log(s.substring(0, 5));   // "Hello"
console.log(s.substring(6));      // "World"
console.log(s.substring(0, 11));  // "Hello World"

console.log("substring test passed!");
