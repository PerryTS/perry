// Test class inheritance

// Parent class
class Animal {
    legs: number;

    constructor(legs: number) {
        this.legs = legs;
    }
}

// Child class extends parent
class Dog extends Animal {
    name: number; // Using number since string handling in classes is limited

    constructor(name: number, legs: number) {
        super(legs);
        this.name = name;
    }
}

// Create a dog
let dog = new Dog(42, 4);

// Access inherited field
console.log(dog.legs); // Should print 4

// Access own field
console.log(dog.name); // Should print 42
