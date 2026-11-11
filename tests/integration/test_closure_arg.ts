// Test passing closures as function arguments

// Simple higher-order function that takes a callback
function applyTwice(x: number, fn: (n: number) => number): number {
    return fn(fn(x));
}

// Test with arrow function
let result = applyTwice(5, (n: number) => n * 2);
console.log(result); // Expected: 20

// Test with named function
function double(n: number): number {
    return n * 2;
}
let result2 = applyTwice(3, double);
console.log(result2); // Expected: 12
