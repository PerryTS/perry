// Integration Test 1: Full-Stack Application Simulation
// Tests: Classes, private fields, file I/O, strings, arrays
import * as fs from 'fs';

// Counter class with private fields
class Counter {
    #count: number;

    constructor(initial: number) {
        this.#count = initial;
    }

    increment(): void {
        this.#count = this.#count + 1;
    }

    getCount(): number {
        return this.#count;
    }
}

// Test 1: Private fields
console.log("=== Test 1: Private Fields ===");
let counter = new Counter(10);
console.log(counter.getCount());  // 10
counter.increment();
counter.increment();
console.log(counter.getCount());  // 12

// Test 3: File I/O
console.log("=== Test 3: File I/O ===");
fs.writeFileSync("/tmp/compilets_test.txt", "Hello, Perry!");
console.log(fs.existsSync("/tmp/compilets_test.txt"));  // 1
let content = fs.readFileSync("/tmp/compilets_test.txt");
console.log(content);  // Hello, Perry!
fs.unlinkSync("/tmp/compilets_test.txt");
console.log(fs.existsSync("/tmp/compilets_test.txt"));  // 0

// Test 4: String methods
console.log("=== Test 4: String Methods ===");
let text = "  Hello, World!  ";
let trimmed = text.trim();
console.log(trimmed);
console.log(trimmed.toLowerCase());
console.log(trimmed.toUpperCase());

// Test 5: Array operations
console.log("=== Test 5: Arrays ===");
let numbers: number[] = [1, 2, 3, 4, 5];
let doubled = numbers.map((x: number) => x * 2);
console.log(doubled[0]);  // 2
console.log(doubled[4]);  // 10

let evens = numbers.filter((x: number) => x % 2 === 0);
console.log(evens.length);  // 2
console.log(evens[0]);      // 2
console.log(evens[1]);      // 4

let sum = numbers.reduce((acc: number, x: number) => acc + x, 0);
console.log(sum);  // 15

console.log("=== Integration Test 1 PASSED ===");
