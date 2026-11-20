// Classes: fields, methods, inheritance, getters, statics.
class Animal {
  name: string;
  constructor(name: string) {
    this.name = name;
  }
  speak() {
    return `${this.name} makes a sound`;
  }
  get upper() {
    return this.name.toUpperCase();
  }
}
class Dog extends Animal {
  static count = 0;
  constructor(name: string) {
    super(name);
    Dog.count++;
  }
  speak() {
    return `${super.speak()} (woof)`;
  }
}
const d = new Dog("rex");
new Dog("fido");
console.log(d.speak(), d.upper, Dog.count, d instanceof Animal);
