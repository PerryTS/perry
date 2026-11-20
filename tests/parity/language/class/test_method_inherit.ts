// Test method inheritance

class Animal {
    name: number;

    constructor(name: number) {
        this.name = name;
    }

    speak(): number {
        return this.name;
    }
}

class Dog extends Animal {
    breed: number;

    constructor(name: number, breed: number) {
        super(name);
        this.breed = breed;
    }
}

// Create a dog
let dog = new Dog(42, 100);

// Call inherited method
console.log(dog.speak()); // Should print 42
console.log(dog.breed);   // Should print 100
