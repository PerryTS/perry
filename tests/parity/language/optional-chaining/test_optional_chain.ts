// Test optional chaining

// Simple class for testing
class Person {
    name: string;
    age: number;
    constructor(name: string, age: number) {
        this.name = name;
        this.age = age;
    }
}

// Test basic optional chaining on potentially null value
let person: Person | null = new Person("Alice", 30);

// This should work - person is not null
let name1: string = person?.name ?? "unknown";
console.log(name1);  // Alice

// Create null person
let nullPerson: Person | null = null;

// This should return null, then use default
let name2: string = nullPerson?.name ?? "unknown";
console.log(name2);  // unknown

// Test optional chaining on array access
let arr: number[] = [1, 2, 3];
let val1: number = arr?.[1] ?? -1;
console.log(val1);  // 2

// Test with number property access
let age1: number = person?.age ?? -1;
console.log(age1);  // 30

let age2: number = nullPerson?.age ?? -1;
console.log(age2);  // -1

console.log("Optional chaining tests passed!");
