// Test method overriding

class Animal {
    sound(): number {
        return 1; // generic sound
    }

    move(): number {
        return 10; // walk
    }
}

class Dog extends Animal {
    sound(): number {
        return 2; // bark
    }
    // inherits move()
}

class Cat extends Animal {
    sound(): number {
        return 3; // meow
    }

    move(): number {
        return 20; // pounce
    }
}

let animal = new Animal();
let dog = new Dog();
let cat = new Cat();

console.log(animal.sound()); // 1
console.log(animal.move());  // 10

console.log(dog.sound());    // 2 (overridden)
console.log(dog.move());     // 10 (inherited)

console.log(cat.sound());    // 3 (overridden)
console.log(cat.move());     // 20 (overridden)
