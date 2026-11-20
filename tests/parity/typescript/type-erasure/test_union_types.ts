// Test union types - basic support for local variables

// Basic union type - assign string first, then number
let value: string | number = "hello";
console.log(value);  // Should print: hello

value = 42;
console.log(value);  // Should print: 42

// Union type reassignment test
let x: string | number = 5;
console.log(x);  // Should print: 5
x = "five";
console.log(x);  // Should print: five
x = 10;
console.log(x);  // Should print: 10

// Multiple reassignments
let y: string | number = "start";
console.log(y);  // Should print: start
y = 100;
console.log(y);  // Should print: 100
y = "middle";
console.log(y);  // Should print: middle
y = 200;
console.log(y);  // Should print: 200
y = "end";
console.log(y);  // Should print: end

console.log(999);  // Test completed marker
