// Test class field layout: access ordering, inheritance, private fields at fixed offsets
// Critical for static typing migration: fields must be at correct struct offsets

// === Basic class with multiple fields ===
class Point {
  x: number;
  y: number;
  z: number;
  constructor(x: number, y: number, z: number) {
    this.x = x;
    this.y = y;
    this.z = z;
  }
  magnitude(): number {
    return Math.sqrt(this.x * this.x + this.y * this.y + this.z * this.z);
  }
}

const p = new Point(3, 4, 0);
console.log(p.x);  // 3
console.log(p.y);  // 4
console.log(p.z);  // 0

// Modify fields
p.x = 10;
p.y = 20;
console.log(p.x + p.y); // 30

// === Inheritance field ordering ===
class Animal {
  name: string;
  legs: number;
  constructor(name: string, legs: number) {
    this.name = name;
    this.legs = legs;
  }
  describe(): string {
    return this.name + " has " + this.legs + " legs";
  }
}

class Dog extends Animal {
  breed: string;
  constructor(name: string, breed: string) {
    super(name, 4);
    this.breed = breed;
  }
  fullDesc(): string {
    return this.describe() + ", breed: " + this.breed;
  }
}

const d = new Dog("Rex", "Labrador");
console.log(d.name);    // Rex
console.log(d.legs);    // 4
console.log(d.breed);   // Labrador
console.log(d.describe()); // Rex has 4 legs
console.log(d.fullDesc()); // Rex has 4 legs, breed: Labrador

// === Private fields don't corrupt public field offsets ===
class Account {
  #balance: number;
  owner: string;
  #pin: number;

  constructor(owner: string, balance: number, pin: number) {
    this.owner = owner;
    this.#balance = balance;
    this.#pin = pin;
  }

  getBalance(): number { return this.#balance; }
  deposit(amount: number): void { this.#balance += amount; }
  checkPin(pin: number): boolean { return this.#pin === pin; }
  getOwner(): string { return this.owner; }
}

const acct = new Account("Alice", 100, 1234);
console.log(acct.owner);        // Alice
console.log(acct.getBalance());  // 100
acct.deposit(50);
console.log(acct.getBalance());  // 150
console.log(acct.checkPin(1234)); // true
console.log(acct.checkPin(0)); // false

// === Multiple inheritance levels ===
class Base {
  a: number;
  constructor(a: number) { this.a = a; }
}

class Middle extends Base {
  b: string;
  constructor(a: number, b: string) {
    super(a);
    this.b = b;
  }
}

class Leaf extends Middle {
  c: boolean;
  constructor(a: number, b: string, c: boolean) {
    super(a, b);
    this.c = c;
  }
  show(): string {
    return this.a + " " + this.b + " " + this.c;
  }
}

const leaf = new Leaf(42, "hello", true);
console.log(leaf.a);     // 42
console.log(leaf.b);     // hello
console.log(leaf.c);     // true
console.log(leaf.show()); // 42 hello true

// === Mixed field types in one class ===
class Record {
  id: number;
  name: string;
  active: boolean;
  tags: string[];

  constructor(id: number, name: string, active: boolean) {
    this.id = id;
    this.name = name;
    this.active = active;
    this.tags = [];
  }

  addTag(tag: string): void { this.tags.push(tag); }

  summary(): string {
    return this.id + ": " + this.name + " (" + (this.active ? "active" : "inactive") + ") [" + this.tags.join(", ") + "]";
  }
}

const r = new Record(1, "Test", true);
r.addTag("important");
r.addTag("v2");
console.log(r.summary()); // 1: Test (active) [important, v2]

// === Class instance in array ===
const points: Point[] = [new Point(1, 2, 3), new Point(4, 5, 6)];
console.log(points[0].x); // 1
console.log(points[1].z); // 6
console.log(points.length); // 2

// === Class field mutation through method ===
class Counter {
  count: number;
  constructor() { this.count = 0; }
  increment(): void { this.count++; }
  getCount(): number { return this.count; }
}

const c = new Counter();
c.increment();
c.increment();
c.increment();
console.log(c.getCount()); // 3
