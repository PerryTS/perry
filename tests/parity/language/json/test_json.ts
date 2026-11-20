// Test JSON.parse and JSON.stringify

// Test JSON.stringify with different types
console.log(JSON.stringify("hello"));  // "\"hello\""
console.log(JSON.stringify(42));       // "42"
console.log(JSON.stringify(3.14));     // "3.14"

// Test with variables
let str: string = "world";
console.log(JSON.stringify(str));      // "\"world\""

let num = 100;
console.log(JSON.stringify(num));      // "100"

// Test JSON.parse with number
let parsed = JSON.parse("42");
console.log(parsed);  // 42

// Test JSON.parse with float
let parsedFloat = JSON.parse("3.14");
console.log(parsedFloat);  // 3.14

console.log("JSON test passed!");
