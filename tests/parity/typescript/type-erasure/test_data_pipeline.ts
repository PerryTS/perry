// Integration Test 2: Data Processing Pipeline
// Tests: Generics, Map/Set, array methods, union types, type guards, Math

// Generic container class
class Container<T> {
    value: T;

    constructor(v: T) {
        this.value = v;
    }

    get(): T {
        return this.value;
    }

    set(v: T): void {
        this.value = v;
    }
}

// Test 1: Generic containers with numbers
console.log("=== Test 1: Generics ===");
let numContainer = new Container<number>(42);
console.log(numContainer.get());  // 42
numContainer.set(100);
console.log(numContainer.get());  // 100

// Test 2: Map operations
console.log("=== Test 2: Map ===");
let scoreMap = new Map<string, number>();
scoreMap.set("Alice", 95);
scoreMap.set("Bob", 87);
scoreMap.set("Charlie", 92);

console.log(scoreMap.get("Alice"));   // 95
console.log(scoreMap.get("Bob"));     // 87
console.log(scoreMap.has("Charlie")); // 1
console.log(scoreMap.has("David"));   // 0
console.log(scoreMap.size);           // 3

scoreMap.delete("Bob");
console.log(scoreMap.size);           // 2

// Test 3: Set operations
console.log("=== Test 3: Set ===");
let uniqueNumbers = new Set<number>();
uniqueNumbers.add(1);
uniqueNumbers.add(2);
uniqueNumbers.add(3);
uniqueNumbers.add(2);  // Duplicate, should be ignored

console.log(uniqueNumbers.size);    // 3
console.log(uniqueNumbers.has(2));  // 1
console.log(uniqueNumbers.has(5));  // 0

uniqueNumbers.delete(2);
console.log(uniqueNumbers.size);    // 2
console.log(uniqueNumbers.has(2));  // 0

// Test 4: Array pipeline operations
console.log("=== Test 4: Array Pipeline ===");
let data: number[] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];

// Filter evens, double them, sum the result
let evens = data.filter((x: number) => x % 2 === 0);
console.log(evens.length);  // 5

let doubled = evens.map((x: number) => x * 2);
console.log(doubled[0]);  // 4
console.log(doubled[4]);  // 20

let total = doubled.reduce((acc: number, x: number) => acc + x, 0);
console.log(total);  // 60

// Test 5: Union types with type guards
console.log("=== Test 5: Union Types ===");
let mixedValue: string | number = "hello";
console.log(typeof mixedValue);  // string

mixedValue = 42;
console.log(typeof mixedValue);  // number

// Test 6: Math operations
console.log("=== Test 6: Math ===");
console.log(Math.floor(3.7));   // 3
console.log(Math.ceil(3.2));    // 4
console.log(Math.round(3.5));   // 4
console.log(Math.abs(-5));      // 5
console.log(Math.sqrt(16));     // 4
console.log(Math.pow(2, 8));    // 256
console.log(Math.min(5, 3, 8)); // 3
console.log(Math.max(5, 3, 8)); // 8

console.log("=== Integration Test 2 PASSED ===");
