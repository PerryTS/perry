// Test rest parameters (...args)

// Basic rest parameter function - sum all numbers
function sum(...numbers: number[]): number {
    let total = 0;
    for (let n of numbers) {
        total += n;
    }
    return total;
}

// Test calling with multiple arguments
console.log(sum(1, 2, 3, 4, 5));  // Should print 15

// Test calling with fewer arguments
console.log(sum(10, 20));  // Should print 30

// Test calling with single argument
console.log(sum(42));  // Should print 42

// Test calling with no arguments
console.log(sum());  // Should print 0

// Rest parameter with regular parameters before it
function greet(greeting: string, ...names: string[]): void {
    // For now, just print the count since string concatenation may vary
    console.log(names.length);
}

// Note: String arrays aren't fully supported yet, so test with numbers only
function average(first: number, ...rest: number[]): number {
    let total = first;
    let count = 1;
    for (let n of rest) {
        total += n;
        count += 1;
    }
    return total / count;
}

console.log(average(10, 20, 30));  // Should print 20

// Test with explicit type annotation
function multiply(...nums: number[]): number {
    let result = 1;
    for (let n of nums) {
        result = result * n;
    }
    return result;
}

console.log(multiply(2, 3, 4));  // Should print 24
console.log(multiply(5));  // Should print 5
console.log(multiply());  // Should print 1 (identity for multiplication)
