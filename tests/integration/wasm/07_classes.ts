// Classes: constructors, methods, inheritance, instanceof
class Animal {
  name: string;
  constructor(name: string) { this.name = name; }
  speak(): string { return this.name + " makes a sound"; }
}

class Dog extends Animal {
  breed: string;
  constructor(name: string, breed: string) {
    super(name);
    this.breed = breed;
  }
  speak(): string { return this.name + " barks"; }
  info(): string { return this.name + " is a " + this.breed; }
}

const a = new Animal("Cat");
console.log(a.speak());
console.log(a.name);

const d = new Dog("Rex", "Husky");
console.log(d.speak());
console.log(d.info());
console.log(d.name);
console.log(d.breed);
console.log(d instanceof Dog);
console.log(d instanceof Animal);
