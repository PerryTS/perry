// Test default parameters

function greet(name: string, greeting: string = "Hello"): void {
    console.log(greeting);
    console.log(name);
}

// Call with both arguments
greet("World", "Hi");

// Call with just required argument (should use default)
greet("Alice");

// Test with number defaults
function add(a: number, b: number = 10): number {
    return a + b;
}

console.log(add(5, 3));  // 8
console.log(add(5));     // 15 (5 + 10)

// Test multiple defaults
function multiDefault(a: number, b: number = 2, c: number = 3): number {
    return a + b + c;
}

console.log(multiDefault(1, 2, 3));  // 6
console.log(multiDefault(1, 2));     // 6 (1 + 2 + 3)
console.log(multiDefault(1));        // 6 (1 + 2 + 3)

console.log("Default params tests passed!");
